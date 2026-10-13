/*
 * Copyright 2026 Feifan He for CodeWeavers
 *
 * This library is free software; you can redistribute it and/or
 * modify it under the terms of the GNU Lesser General Public
 * License as published by the Free Software Foundation; either
 * version 2.1 of the License, or (at your option) any later version.
 *
 * This library is distributed in the hope that it will be useful,
 * but WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * Lesser General Public License for more details.
 *
 * You should have received a copy of the GNU Lesser General Public
 * License along with this library; if not, write to the Free Software
 * Foundation, Inc., 51 Franklin St, Fifth Floor, Boston, MA 02110-1301, USA
 */

#pragma once

#include "dxmt_scaler.hpp"
#include "dxmt_texture.hpp"
#include <cstdint>

namespace dxmt {

enum class EncoderType {
  Null,
  Clear,
  Render,
  Blit,
  Compute,
  Resolve,
  TemporalUpscale, // MacNeutron
};

struct EncoderData {
  EncoderType type;
  EncoderData *next = nullptr;
  uint64_t id;
  // Timestamps written at this encoder's end (MacNeutron; Metal samples counters only at stage boundaries).
  struct Sample {
    obj_handle_t buffer; // the query heap's counter buffer, and the query's index in it
    uint32_t index;
    uint64_t *value; // Task F4: the query's value in its heap, which the queue writes (null: the heap's buffer is read)
  } samples[4];
  uint8_t num_samples = 0;
  uint32_t barriers = 0; // the list's barrier calls before this encoder began (DXMT_STATS, ordering)
  // MacNeutron: what this encoder waits on (GPU overlap spec §3.1), decided as it closes; the queue turns it into
  // fence waits. Resources are DXMT Texture or Buffer objects, or a query heap's Metal results buffer.
  uint32_t barriers_last = 0;  // the barrier calls before its last command: one recorded inside it binds it all
  uint32_t position = 0;       // its index among its command list's encoders
  bool join = false;           // waits on all earlier work
  bool writes_unknown = true;  // may write anything (DXMT's own passes); cleared where an encoder lists its writes
  uint8_t write_count = 0;
  const void *writes[16];
  uint32_t dep_count = 0;
  const uint32_t *deps = nullptr; // positions of the encoders since its list's last join that it waits on
  bool timestamp_only = false; // MacNeutron: an empty blit holding timestamps alone (M3 moves them into a render pass)
};

struct ClearEncoderData : EncoderData {
  union {
    WMTClearColor color;
    std::pair<float, uint8_t> depth_stencil;
  };
  TextureViewRef attachment;
  unsigned clear_dsv;
  unsigned array_length;
  unsigned width;
  unsigned height;
  unsigned depth_plane;

  ClearEncoderData() {}
};

struct RenderEncoderColorAttachmentData {
  TextureViewRef attachment;
  enum WMTLoadAction load_action;
  enum WMTStoreAction store_action;
  uint16_t level;
  uint16_t slice;
  uint32_t depth_plane;
  struct WMTClearColor clear_color;
  TextureViewRef resolve_attachment;
  uint16_t resolve_level;
  uint16_t resolve_slice;
  uint32_t resolve_depth_plane;
};

struct RenderEncoderDepthAttachmentData {
  TextureViewRef attachment;
  enum WMTLoadAction load_action;
  enum WMTStoreAction store_action;
  uint16_t level;
  uint16_t slice;
  uint32_t depth_plane;
  float clear_depth;
};

struct RenderEncoderStencilAttachmentData {
  TextureViewRef attachment;
  enum WMTLoadAction load_action;
  enum WMTStoreAction store_action;
  uint16_t level;
  uint16_t slice;
  uint32_t depth_plane;
  uint8_t clear_stencil;
};

struct RenderEncoderData : EncoderData {
  std::array<RenderEncoderColorAttachmentData, 8> colors;
  RenderEncoderDepthAttachmentData depth;
  RenderEncoderStencilAttachmentData stencil;
  uint8_t default_raster_sample_count;
  uint16_t render_target_array_length;
  uint32_t render_target_height;
  uint32_t render_target_width;
  wmtcmd_render_nop cmd_head;
  wmtcmd_base *cmd_tail;
  uint8_t dsv_planar_flags;
  uint8_t dsv_readonly_flags;
  uint8_t render_target_count;
  bool use_visibility_result = 0;
  obj_handle_t visibility_buffer = 0; // MacNeutron: the occlusion query heap this pass counts samples into
  bool use_tessellation = 0;
  bool use_geometry = 0;
  bool uav_writes = false; // MacNeutron (B6): a draw's pipeline may write a UAV, which `writes` doesn't list
  // MacNeutron: compute commands run just before this pass (ExecuteIndirect's resolvers writing its ICBs).
  wmtcmd_compute_nop pre_head;
  wmtcmd_base *pre_tail = nullptr;
};

// A clear-only pass `c` as `r`'s load action (M4 at recording, E7 at execution): the attachment of `r` that is `c`'s
// view (each plane it clears), with `r`'s render area its size (a Metal load action clears the render area only).
// Returns the colour attachment's index, kClearDepthStencil for the depth and stencil planes, or -1.
constexpr int kClearDepthStencil = 8;
inline int
ClearSlot(const ClearEncoderData *c, const RenderEncoderData *r) {
  if (c->width != r->render_target_width || c->height != r->render_target_height ||
      c->array_length != r->render_target_array_length)
    return -1;
  if (c->clear_dsv)
    return ((c->clear_dsv & 1) && r->depth.attachment.ptr() != c->attachment.ptr()) ||
                   ((c->clear_dsv & 2) && r->stencil.attachment.ptr() != c->attachment.ptr())
               ? -1
               : kClearDepthStencil;
  for (unsigned i = 0; i < r->colors.size(); i++)
    if (r->colors[i].attachment && r->colors[i].attachment.ptr() == c->attachment.ptr() &&
        r->colors[i].depth_plane == c->depth_plane)
      return i;
  return -1;
}

struct BlitEncoderData : EncoderData {
  wmtcmd_blit_nop cmd_head;
  wmtcmd_base *cmd_tail;
};

struct ComputeEncoderData : EncoderData {
  wmtcmd_compute_nop cmd_head;
  wmtcmd_base *cmd_tail;
};
struct ResolveEncoderData : EncoderData {
  TextureViewRef src;
  TextureViewRef dst;
};

// MacNeutron (XeSS answered by MetalFX): a MetalFX temporal upscale, encoded between two blits that hand over on the
// upscaler's fence. The views are what MetalFX reads and writes: a scratch texture where the list copied an input in
// before (blits of its own) or copies the output out after. They and the upscaler stay alive until the allocator resets,
// however soon the upscaler's owner releases it.
struct TemporalUpscaleEncoderData : EncoderData {
  Rc<TemporalScaler> scaler;
  TextureViewRef color, depth, motion, exposure, reactive, output;
  Rc<Texture> depth_scratch, motion_scratch, output_scratch;
  WMTFXTemporalScalerProps props;
};

}; // namespace dxmt
