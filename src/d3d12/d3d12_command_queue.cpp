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

#include "com/com_guid.hpp"
#include "d3d12_stats.hpp"
#include "com/com_pointer.hpp"
#include "d3d12_device.hpp"
#include "d3d12_pageable.hpp"
#include "dxgi_interfaces.h"
#include "log/log.hpp"
#include "util_env.hpp"
#include "dxmt_capture.hpp"
#include "dxmt_format.hpp"
#include "d3d12_dxil_dump.hpp"
#include <chrono>
#include <cstdio>
#include <cstring>
#include <ctime>
#include <mutex>
#include <atomic>
#include <algorithm>
#include <array>
#include <type_traits>
#include <vector>
#include "d3d10_1.h"
#include "d3d11_4.h"

namespace dxmt {

// The in-flight command buffer pool's capacity, and the default number of slots the queue takes from it (B6). Each slot
// holds a Metal command buffer and its CPU-side bookkeeping; the queue waits in Open() when none is free. Palworld's
// submission thread stalled about 12 ms a frame on the old default of 32 (DXMT_STATS: "queue.(wait for a command
// buffer slot)") and about 0 at 64, so 64 is the default; DXMT_D3D12_CMDBUF_SLOTS lowers or raises the slots taken,
// never past the capacity. Metal's own concurrent limit is higher than either.
constexpr auto kCommandQueueSize = 64u;
constexpr auto kCommandQueueSlots = 64u;

const GUID kD3D12CommandQueueDownlevelUUID = {
    0x38a8c5ef, 0x7ccb, 0x4e81, {0x91, 0x4f, 0xa6, 0xe9, 0xd0, 0x72, 0xc4, 0x94}
};

// DXMT_STATS: names each Metal encoder "pass-<n> <kind>", n as in F9's passes.txt, for Instruments' encoder list.
// Render passes add their targets, "3c+d 1728x1120", to tell passes apart when their numbers shift between frames.
template <typename Encoder>
static void
LabelPass(Encoder &encoder, unsigned pass, const char *kind, RenderEncoderData *render = nullptr) {
  if (!g_stats_on)
    return;
  char name[64];
  int n = snprintf(name, sizeof(name), "pass-%u %s", pass, kind);
  if (render) {
    unsigned colors = 0;
    WMT::Texture first;
    for (auto &c : render->colors)
      if (c.attachment && colors++ == 0)
        first = c.attachment.texture();
    if (!first && render->depth.attachment)
      first = render->depth.attachment.texture();
    n += snprintf(name + n, sizeof(name) - n, " %uc%s", colors, render->depth.attachment ? "+d" : "");
    if (first)
      snprintf(name + n, sizeof(name) - n, " %llux%llu", (unsigned long long)first.width(),
               (unsigned long long)first.height());
  }
  encoder.setLabel(WMT::String::string(name, WMTUTF8StringEncoding));
}

// MacNeutron (frame generation on MetalFX, MACNEUTRON_FRAMEGEN=1): the last temporal upscale any queue encoded, for
// the next present to pair MetalFX's frame interpolator with: the upscaler and the depth and motion vectors it read
// (held until replaced, so the textures outlive the present that reads them). Process-wide: a game may present from
// another queue than the one it upscales on.
struct FrameGenSlot {
  std::mutex mutex;
  Rc<TemporalScaler> scaler;
  TextureViewRef depth, motion;
  WMTFXTemporalScalerProps props{};
  bool fresh = false; // an upscale since the last present
};
static FrameGenSlot g_framegen_slot;

static bool
FrameGenOn() {
  static const bool on = env::getEnvVar("MACNEUTRON_FRAMEGEN") == "1";
  return on;
}

class MTLD3D12CommandQueueImpl : public MTLD3D12Pageable<MTLD3D12CommandQueue, IMTLSwapChainFactory> {

  D3D12_COMMAND_QUEUE_DESC desc_;

  WMT::Reference<WMT::CommandQueue> queue_;
  // MacNeutron: encoder ordering (GPU overlap spec §3.3, §3.9). Each encoder updates a fence of its own from this ring
  // and waits on fences of earlier encoders: a join on every encoder since the last join (frontier_), any other
  // encoder on its group's fence (group_wait_: the join's early fence, updated after its first stage when it draws,
  // else the join's own fence) and on its dependencies in its command list. A fence is taken again only once no later
  // encoder can need its last update: an encoder that would take it sooner joins first.
  static constexpr unsigned kFences = 256;
  static constexpr uint16_t kNoFence = 0xffff;
  std::array<WMT::Reference<WMT::Fence>, kFences> fences_;
  std::array<uint64_t, kFences> fence_group_ = {}; // the join group that last updated each fence
  uint64_t group_ = 2;                             // fences of this group and the previous one are live
  uint16_t next_fence_ = 0;
  std::vector<uint16_t> frontier_, waits_;
  uint16_t group_wait_ = kNoFence, early_ = kNoFence; // early_: the early fence the last Order gave its join
  std::vector<uint16_t> list_fences_; // by position in the command list being encoded
  uint32_t list_join_ = 0;            // that list's last join, by position (UINT32_MAX: the queue's own work)
  // Strict order, every encoder a join, unless DXMT_D3D12_OVERLAP=1: on Apple GPUs overlap added more idle between
  // encoders than it saved (docs/testing/acceptance-dxmt-gpu-overlap.md, MacNeutron). Dumps and pixel history are
  // always strict; DXMT_D3D12_SERIAL=1 still forces it.
  bool serial_ = env::getEnvVar("DXMT_D3D12_OVERLAP") != "1" || env::getEnvVar("DXMT_D3D12_SERIAL") == "1";
  std::vector<wmtcmd_render_fence_op> render_ops_;
  std::vector<wmtcmd_blit_fence_op> blit_ops_;
  std::vector<wmtcmd_compute_fence_op> compute_ops_;

  // Takes the fence the next encoder updates and fills waits_ with the fences it waits on (and early_ with the early
  // fence a drawing join render pass also updates). `e` null: the queue's own work (pass dumps, pixel history,
  // presents), which joins.
  uint16_t
  Order(EncoderData *e, bool serial) {
    uint16_t fence = next_fence_;
    next_fence_ = (next_fence_ + 1) % kFences;
    bool join = !e || e->join || serial || fence_group_[fence] + 1 >= group_;
    waits_.clear();
    early_ = kNoFence;
    if (join) {
      waits_ = frontier_;
      frontier_.clear();
      group_++;
      list_join_ = e ? e->position : UINT32_MAX;
      group_wait_ = fence;
      if (!serial && e && e->type == EncoderType::Render && Draws(static_cast<RenderEncoderData *>(e))) {
        DXMT_STAT_COUNT("#encoder early fences", 1);
        early_ = group_wait_ = next_fence_; // live for the group; the join's own fence covers it for the next join
        next_fence_ = (next_fence_ + 1) % kFences;
        fence_group_[early_] = group_;
      }
    } else {
      if (list_join_ == UINT32_MAX) // after the queue's own join inside a list: earlier positions are behind it
        list_join_ = e->position;
      waits_.assign(1, group_wait_);
      for (uint32_t i = 0; i < e->dep_count; i++)
        if (e->deps[i] >= list_join_) // earlier ones are behind the join, so behind the group fence
          waits_.push_back(list_fences_[e->deps[i]]);
    }
    if (e && g_stats_on)
      CountOrder(e, join);
    fence_group_[fence] = group_;
    frontier_.push_back(fence);
    if (e) {
      if (e->position >= list_fences_.size())
        list_fences_.resize(e->position + 1);
      list_fences_[e->position] = fence;
    }
    return fence;
  }

  // A draw among a render pass's commands: then its first stage, after its waits, proves them met (spec §3.9).
  static bool
  Draws(RenderEncoderData *data) {
    for (auto *cmd = (wmtcmd_base *)data->cmd_head.next.get(); cmd; cmd = (wmtcmd_base *)cmd->next.get())
      switch (cmd->type) {
      case WMTRenderCommandDraw:
      case WMTRenderCommandDrawIndexed:
      case WMTRenderCommandDrawIndirect:
      case WMTRenderCommandDrawIndexedIndirect:
      case WMTRenderCommandDrawMeshThreadgroups:
      case WMTRenderCommandDrawMeshThreadgroupsIndirect:
      case WMTRenderCommandDXMTGeometryDraw:
      case WMTRenderCommandDXMTGeometryDrawIndexed:
      case WMTRenderCommandDXMTGeometryDrawIndirect:
      case WMTRenderCommandDXMTGeometryDrawIndexedIndirect:
      case WMTRenderCommandDXMTTessellationMeshDraw:
      case WMTRenderCommandDXMTTessellationMeshDrawIndexed:
      case WMTRenderCommandDXMTTessellationMeshDrawIndirect:
      case WMTRenderCommandDXMTTessellationMeshDrawIndexedIndirect:
      case WMTRenderCommandDispatchThreadsPerTile:
      case WMTRenderCommandExecuteCommandsInBuffer:
        return true;
      default:
        break;
      }
    return false;
  }

  // M3 (GPU overlap spec §3.5): passes encodable as one Metal render pass: the same attachments, the later loading
  // them all and the earlier storing them all, with the same size, sample count, geometry and query heap.
  static bool
  SameTarget(RenderEncoderData *a, RenderEncoderData *b) {
    auto same = [](auto &x, auto &y) {
      return x.attachment.ptr() == y.attachment.ptr() && x.level == y.level && x.slice == y.slice &&
             x.depth_plane == y.depth_plane && (!y.attachment || y.load_action == WMTLoadActionLoad) &&
             (!x.attachment || x.store_action == WMTStoreActionStore);
    };
    for (unsigned i = 0; i < a->colors.size(); i++)
      if (!same(a->colors[i], b->colors[i]))
        return false;
    return same(a->depth, b->depth) && same(a->stencil, b->stencil) &&
           a->render_target_width == b->render_target_width && a->render_target_height == b->render_target_height &&
           a->render_target_array_length == b->render_target_array_length &&
           a->default_raster_sample_count == b->default_raster_sample_count &&
           a->dsv_planar_flags == b->dsv_planar_flags && a->dsv_readonly_flags == b->dsv_readonly_flags &&
           a->visibility_buffer == b->visibility_buffer && a->use_geometry == b->use_geometry;
  }

  bool merge_ = env::getEnvVar("DXMT_D3D12_MERGE") != "0"; // M3 on, unless DXMT_D3D12_MERGE=0 (triage)
  // B7: timestamp-resolve blits deferred past a merged pass, unless DXMT_D3D12_DEFER_RESOLVES=0 (triage).
  bool deferred_resolves_ = env::getEnvVar("DXMT_D3D12_DEFER_RESOLVES") != "0";
  std::vector<EncoderData *> merged_, folds_;               // PlanMerge's result and scratch
  // B7: the timestamp-resolve blits among merged_ (query_resolve_only), encoded after the merged Metal render pass.
  std::vector<EncoderData *> deferred_;
  // PlanMerge's scratch (B6): the barrier calls (after, until] of a list that lie between base and the passes it merges.
  struct Crossing {
    MTLD3D12GraphicsCommandList *list;
    uint32_t after, until;
  };
  std::vector<Crossing> crossings_;

  // B6: barrier calls between merged passes may stay inside the merged pass in strict order (the default), which puts
  // all earlier work behind the base (Order), when they order nothing between the passes: none is opaque (an unknown
  // resource, a split barrier's end, simultaneous access, COMMON; a UAV barrier, UNORDERED_ACCESS or aliasing where one
  // of those passes writes a UAV). A barrier naming a resource the passes write (their attachments: `writes` lists
  // those, not their shaders' UAVs) orders their own writes of it: only null and timestamp-only encoders lie between
  // (a compute, clear or copy ends PlanMerge's walk), so nothing between reads it, and in strict order the base's join
  // already orders everything earlier, so their writes stay in order inside the merged Metal pass with the resource
  // left in the state the last of them needs: such a barrier crosses too. A pass that doesn't list its writes
  // (writes_unknown: DXMT's own passes) keeps every barrier apart, as a merge can't tell what it writes. The null names
  // of UAV and aliasing barriers are their opacity's to judge. -1 when they may; otherwise why not, for DXMT_STATS: the
  // first opaque barrier's BarrierOpacity, kCrossOverlap or kCrossWrite.
  enum { kCrossOverlap = MTLD3D12GraphicsCommandList::kOpaqueKinds, kCrossWrite };
  int
  Crossable(RenderEncoderData *base, RenderEncoderData *next) {
    if (crossings_.empty())
      return -1;
    if (!serial_)
      return kCrossOverlap;
    auto by_uav = [](EncoderData *e) {
      return e->type == EncoderType::Render && static_cast<RenderEncoderData *>(e)->uav_writes;
    };
    bool uav_writes = by_uav(base) || by_uav(next) || std::any_of(merged_.begin(), merged_.end(), by_uav);
    auto unlisted = [](EncoderData *e) { return e->writes_unknown; };
    bool opaque_writes = unlisted(base) || unlisted(next) || std::any_of(merged_.begin(), merged_.end(), unlisted) ||
                         std::any_of(folds_.begin(), folds_.end(), unlisted);
    auto writes = [&](const void *key) { // an attachment base, a merged pass, `next` or a folded blit writes
      auto by = [&](EncoderData *e) {
        for (unsigned i = 0; i < e->write_count; i++)
          if (e->writes[i] == key)
            return true;
        return false;
      };
      return by(base) || by(next) || std::any_of(merged_.begin(), merged_.end(), by) ||
             std::any_of(folds_.begin(), folds_.end(), by);
    };
    auto deferred_write = [&](const void *key) { // a resolve blit encoded after the passes writes it (B7)
      auto by = [&](EncoderData *e) {
        if (e->type != EncoderType::Blit || !e->query_resolve_only)
          return false;
        for (unsigned i = 0; i < e->write_count; i++)
          if (e->writes[i] == key)
            return true;
        return false;
      };
      return std::any_of(merged_.begin(), merged_.end(), by) || std::any_of(folds_.begin(), folds_.end(), by);
    };
    for (auto &c : crossings_) {
      for (auto [call, why] : c.list->barrier_opaque)
        if (call > c.after && call <= c.until && (why >= MTLD3D12GraphicsCommandList::kOpaqueAlways || uav_writes))
          return why;
      for (auto &[call, key] : c.list->barrier_names)
        if (call > c.after && call <= c.until && key) {
          if (opaque_writes || deferred_write(key))
            return kCrossWrite;
          if (writes(key))
            DXMT_STAT_COUNT("#render pass merges across a barrier naming their target", 1);
        }
    }
    return -1;
  }
  static void
  RefuseCrossing(int why) {
    if (!g_stats_on)
      return;
    static const unsigned ids[] = {StatId("#render pass merges refused (barrier): UNORDERED_ACCESS"),
                                   StatId("#render pass merges refused (barrier): a UAV barrier"),
                                   StatId("#render pass merges refused (barrier): an aliasing barrier"),
                                   StatId("#render pass merges refused (barrier): a split barrier's end"),
                                   StatId("#render pass merges refused (barrier): an unknown resource"),
                                   StatId("#render pass merges refused (barrier): simultaneous access"),
                                   StatId("#render pass merges refused (barrier): COMMON"),
                                   StatId("#render pass merges refused (barrier): overlap order"),
                                   StatId("#render pass merges refused (barrier): a resource the passes write")};
    StatCount(ids[why]);
  }
  unsigned merged_list_ = 0;                                // the list of merged_'s last pass
  std::vector<std::pair<EncoderData *, uint16_t>> skipped_; // encoders M3 already encoded, with their fence
  size_t skip_ = 0;

  // M3: the render passes encoded inside `base`'s Metal render pass, and the timestamp-only blits whose samples move
  // to its end, from the encoders after base in this ExecuteCommandLists call (lists[list] is base's). A pass joins
  // when only list boundaries, timestamp-only blits and barriers Crossable lets through come between, its targets are base's
  // (SameTarget), it resolves no indirect commands, and the samples fit (4 counter buffers, one sample each; any
  // number with the timestamp ring, which takes them all at one point). Base must be its group's join, which in strict
  // order every encoder is (Order). Fills merged_ in order, and `info` with the moved samples.
  // ponytail: one ExecuteCommandLists call; merging across calls B coalesced would defer encoding to commit time.
  // DXMT_STATS counts why a render pass that could have joined (the next encoder but for list boundaries and
  // timestamp-only blits) didn't, for the base-pass merge (B6, Task F4).
  enum MergeRefusal { kRefusedTimestamps, kRefusedResolver, kRefusedBarrier, kRefusedAttachments, kRefusedOther };
  static void
  RefuseMerge(EncoderData *e, MergeRefusal why) {
    if (!g_stats_on)
      return;
    static const unsigned ids[] = {StatId("#render pass merges refused (timestamps)"),
                                   StatId("#render pass merges refused (resolver)"),
                                   StatId("#render pass merges refused (barrier)"),
                                   StatId("#render pass merges refused (attachments)"),
                                   StatId("#render pass merges refused (other)")};
    while (e && (e->type == EncoderType::Null || (e->type == EncoderType::Blit && e->timestamp_only)))
      e = e->next; // ponytail: within its list; a pass that starts the next list is counted only if it's e
    if (e && e->type == EncoderType::Render)
      StatCount(ids[why]);
  }
  void
  PlanMerge(RenderEncoderData *base, unsigned list, ID3D12CommandList *const *lists, unsigned count,
            WMTRenderPassInfo &info) {
    merged_.clear();
    folds_.clear();
    deferred_.clear();
    // A pass counting into a query: its counting would reach the next. Overlap: a non-join base waits on its own
    // dependencies only, not the merged passes'. (SMITE 2's base-pass segments are each a list's pass after its start
    // timestamp, the list's join: B6.)
    if ((!base->join && !serial_) || base->visibility_buffer) {
      RefuseMerge(base->next ? base->next
                  : list + 1 < count ? static_cast<MTLD3D12GraphicsCommandList *>(lists[list + 1])->entry
                                     : nullptr,
                  kRefusedOther);
      return;
    }
    unsigned samples = base->num_samples;
    auto fits = [&](EncoderData *e, unsigned &n) {
      if (sampling_) // the ring (Task F4): the pass's samples are all taken at one point, in any number
        return true;
      for (unsigned k = 0; k < e->num_samples; k++) {
        if (n == std::size(info.sample_buffers))
          return false;
        for (unsigned j = 0; j < n; j++)
          if (info.sample_buffers[j].sample_buffer == e->samples[k].buffer)
            return false;
        info.sample_buffers[n++] = {e->samples[k].buffer, e->samples[k].index, ~0ull};
      }
      return true;
    };
    EncoderData *last = base, *cursor = base->next;
    unsigned l = list, last_list = list;
    auto barriers = [&]() { return l == last_list ? last->barriers_last : 0u; }; // allowed before cursor, in list l
    crossings_.clear();
    for (;;) {
      if (!cursor) { // the end of list l: its barriers after `last` (all of a list of timestamps alone) lie between
        auto *done = static_cast<MTLD3D12GraphicsCommandList *>(lists[l]);
        if (done->barrier_count != barriers())
          crossings_.push_back({done, barriers(), done->barrier_count});
        if (++l == count)
          break;
        cursor = static_cast<MTLD3D12GraphicsCommandList *>(lists[l])->entry;
        if (std::find(lists + list, lists + l, lists[l]) != lists + l) { // a list executed twice
          RefuseMerge(cursor, kRefusedOther);
          break;
        }
        continue;
      }
      if (cursor->type == EncoderType::Null) {
        cursor = cursor->next;
        continue;
      }
      if (cursor->barriers_last != barriers()) // barriers before it, or inside it, since `last`
        crossings_.push_back({static_cast<MTLD3D12GraphicsCommandList *>(lists[l]), barriers(), cursor->barriers_last});
      if (cursor->type == EncoderType::Blit && cursor->timestamp_only) {
        folds_.push_back(cursor);
        cursor = cursor->next;
        continue;
      }
      // B7: a blit of timestamp resolves alone (Unreal's, at the end of each base-pass segment's list) moves after the
      // merged pass. Timestamps into GPU memory resolve as zeros here (ResolveQueryData), so the move changes no
      // value; no merged pass reads its destination, as reading it needs a barrier naming it, which refuses (Crossable).
      if (deferred_resolves_ && cursor->type == EncoderType::Blit && cursor->query_resolve_only &&
          !cursor->num_samples && !cursor->writes_unknown) {
        folds_.push_back(cursor);
        cursor = cursor->next;
        continue;
      }
      if (cursor->type != EncoderType::Render)
        break;
      auto *next = static_cast<RenderEncoderData *>(cursor);
      unsigned n = samples;
      bool ok = !next->pre_tail && SameTarget(base, next);
      for (auto *f : folds_)
        ok = ok && fits(f, n);
      ok = ok && fits(next, n);
      int why = ok ? Crossable(base, next) : -1; // DXMT_STATS: barriers refuse only a pass that could join but for them
      if (!ok || why >= 0) { // put back what `fits` wrote past the samples kept
        for (unsigned j = samples; j < n; j++)
          info.sample_buffers[j] = {};
        if (why >= 0) {
          RefuseMerge(next, kRefusedBarrier);
          RefuseCrossing(why);
        } else
          RefuseMerge(next, next->pre_tail            ? kRefusedResolver
                            : !SameTarget(base, next) ? kRefusedAttachments
                                                      : kRefusedTimestamps);
        break;
      }
      samples = n;
      if (!crossings_.empty()) // they stay: each later pass is checked against all barriers since base
        DXMT_STAT_COUNT("#render pass merges across barriers", 1);
      merged_.insert(merged_.end(), folds_.begin(), folds_.end());
      for (auto *f : folds_)
        if (f->query_resolve_only)
          deferred_.push_back(f);
      merged_.push_back(next);
      folds_.clear();
      last = next;
      last_list = merged_list_ = l;
      cursor = next->next;
    }
  }

  void
  CountOrder(const EncoderData *e, bool join) { // DXMT_STATS (GPU overlap spec §3.8)
    static const unsigned joins = StatId("#encoder full joins"), listed = StatId("#encoders with a dependency list"),
                          dep_waits = StatId("#encoder dependency waits"),
                          fence_waits = StatId("#encoder fence waits"),
                          free = StatId("#encoder boundaries free to overlap");
    StatCount(fence_waits, waits_.size());
    if (join) {
      StatCount(joins);
      return;
    }
    size_t deps = waits_.size() - 1; // beyond the group fence
    if (deps) {
      StatCount(listed);
      StatCount(dep_waits, deps);
    }
    if (e->position && e->position - 1 < list_fences_.size() &&
        std::find(waits_.begin(), waits_.end(), list_fences_[e->position - 1]) == waits_.end())
      StatCount(free);
  }

  // GPU efficiency spec E7: clear-only passes folded, for this call, into a later render pass's load action:
  // {list index, target, clear}.
  struct ClearFold {
    unsigned list;
    EncoderData *target;
    ClearEncoderData *clear;
  };
  std::vector<ClearFold> clear_folds_;

  // A barrier call in (after, until] of `list` names `texture` (or may: an unknown resource).
  static bool
  BarrierNames(MTLD3D12GraphicsCommandList *list, uint32_t after, uint32_t until, const void *texture) {
    for (auto &[call, key] : list->barrier_names)
      if (call > after && call <= until && (!key || key == texture))
        return true;
    return false;
  }

  // A barrier call in (after, until] of `list` names an unknown resource (a resourceless aliasing barrier, or one DXMT
  // can't attribute): a clear can't fold across it. B6: one naming the clear's own texture doesn't refuse the fold. The
  // clear is deferred to a later pass's load action; only null and timestamp-only encoders and other textures' clears
  // lie between (a compute, clear or copy ends ClearTarget's walk), and in D3D12 a render target can only be read
  // through an encoder after a transition naming it, so nothing between can see the clear done late.
  static bool
  BarrierUnknown(MTLD3D12GraphicsCommandList *list, uint32_t after, uint32_t until) {
    for (auto &[call, key] : list->barrier_names)
      if (call > after && call <= until && !key)
        return true;
    return false;
  }

  // E7: the render pass in this call (from lists[l], after `clear`) that `clear` can be the load action of: the first
  // encoder after it but for list boundaries, timestamp-only blits and other textures' clears, with no barrier naming
  // an unknown resource between. B6: a barrier naming the clear's own texture doesn't refuse the fold (BarrierUnknown).
  // In D3D12 a render target can only be read through an encoder after a transition naming it, so nothing between can
  // see the clear done late. Null (counting why) if none.
  RenderEncoderData *
  ClearTarget(ClearEncoderData *clear, unsigned l, ID3D12CommandList *const *lists, unsigned count, unsigned &at) {
    auto texture = clear->write_count ? clear->writes[0] : nullptr;
    if (!texture)
      return nullptr;
    EncoderData *cursor = clear->next;
    uint32_t after = clear->barriers;
    bool named = false; // a barrier between names the clear's own texture: the fold crossed it (B6, counted)
    for (;;) {
      auto list = static_cast<MTLD3D12GraphicsCommandList *>(lists[l]);
      if (!cursor) {
        if (BarrierUnknown(list, after, UINT32_MAX)) {
          DXMT_STAT_COUNT("#clear folds refused (barrier)", 1);
          return nullptr;
        }
        if (BarrierNames(list, after, UINT32_MAX, texture))
          named = true;
        if (++l == count) {
          DXMT_STAT_COUNT("#clear folds refused (no target)", 1);
          return nullptr;
        }
        cursor = static_cast<MTLD3D12GraphicsCommandList *>(lists[l])->entry;
        after = 0;
        continue;
      }
      if (cursor->type == EncoderType::Null || (cursor->type == EncoderType::Blit && cursor->timestamp_only) ||
          (cursor->type == EncoderType::Clear && cursor->write_count && cursor->writes[0] != texture)) {
        cursor = cursor->next;
        continue;
      }
      if (BarrierUnknown(list, after, cursor->barriers)) {
        DXMT_STAT_COUNT("#clear folds refused (barrier)", 1);
        return nullptr;
      }
      if (BarrierNames(list, after, cursor->barriers, texture))
        named = true;
      if (cursor->type != EncoderType::Render) {
        DXMT_STAT_COUNT("#clear folds refused (no target)", 1);
        return nullptr;
      }
      auto r = static_cast<RenderEncoderData *>(cursor);
      if (ClearSlot(clear, r) < 0) {
        DXMT_STAT_COUNT("#clear folds refused (mismatch)", 1);
        return nullptr;
      }
      if (named)
        DXMT_STAT_COUNT("#clear folds across a barrier naming its texture", 1);
      at = l;
      return r;
    }
  }

  // E7: `clear` as the load action of the pass being encoded, unless the pass already clears that plane (a later
  // clear, M4's, wins).
  static void
  ApplyClear(const ClearEncoderData *c, const RenderEncoderData *r, WMTRenderPassInfo &info) {
    int slot = ClearSlot(c, r);
    if (slot == kClearDepthStencil) {
      if ((c->clear_dsv & 1) && r->depth.load_action != WMTLoadActionClear) {
        info.depth.load_action = WMTLoadActionClear;
        info.depth.clear_depth = c->depth_stencil.first;
      }
      if ((c->clear_dsv & 2) && r->stencil.load_action != WMTLoadActionClear) {
        info.stencil.load_action = WMTLoadActionClear;
        info.stencil.clear_stencil = c->depth_stencil.second;
      }
    } else if (slot >= 0 && r->colors[slot].load_action != WMTLoadActionClear) {
      info.colors[slot].load_action = WMTLoadActionClear;
      info.colors[slot].clear_color = c->color;
    }
  }

  // GPU efficiency spec E4: lone timestamps (timestamp-only blits) waiting, in order, for the start of the next encoder
  // the queue encodes; and whether one of them was its list's join (the encoder after them joins then).
  std::vector<EncoderData::Sample> pending_samples_;
  // Why a timestamp got a blit of its own (DXMT_STATS, Task F4): another counter buffer than the encoder's, the earlier
  // of several waiting for one encoder's start, a next encoder that takes none (a clear, a resolve, the upscale, a
  // pass's indirect resolvers, an encoder without commands), the end of the call.
  enum LoneReason { kLoneCounterBuffer, kLoneSeveral, kLoneNoStart, kLoneEnd };
  bool pending_join_ = false;

  // The latest pending timestamp, for the start of an encoder whose own samples are all in `own` (0: it has none):
  // Apple GPUs write one counter buffer per encoder. The earlier ones (or all of them, if it doesn't fit) get blits of
  // their own first, so the values stay in order. Returns {buffer, index}, or {0, ~0}.
  std::pair<obj_handle_t, uint64_t>
  TakePending(WMT::CommandBuffer &cmdbuf, obj_handle_t own) {
    if (pending_samples_.empty())
      return {0, ~0ull};
    auto last = pending_samples_.back();
    bool fits = !own || own == last.buffer;
    for (size_t k = 0; k + (fits ? 1 : 0) < pending_samples_.size(); k++)
      SampleAfter(cmdbuf, pending_samples_[k].buffer, pending_samples_[k].index,
                  k + 1 < pending_samples_.size() ? kLoneSeveral : kLoneCounterBuffer);
    pending_samples_.clear();
    if (!fits)
      return {0, ~0ull};
    DXMT_STAT_COUNT("#timestamps at the next encoder's start", 1);
    return {last.buffer, last.index};
  }

  // Waiting timestamps, each in a blit of its own: for an encoder whose start isn't ordered after earlier work (a clear
  // or resolve waits before its fragment stage only; a pass's indirect resolvers run first) or that Metal drops (no
  // commands), and after the call's last encoder. With the shared counter buffer (Task F4), one blit takes them all,
  // only where no earlier encoder in the call could (see Trailing).
  void
  FlushPending(WMT::CommandBuffer &cmdbuf, LoneReason why = kLoneNoStart) {
    if (!sampling_) {
      for (auto &p : pending_samples_)
        SampleAfter(cmdbuf, p.buffer, p.index, why);
    } else if (uint64_t slot = Point(pending_samples_.data(), pending_samples_.size()); slot != ~0ull) {
      SampleAfter(cmdbuf, sampling_->samples, slot, why);
    }
    pending_samples_.clear();
  }

  // Metal drops an encoder without commands, its samples and fence operations with it: a blit with nothing else to do
  // gets this, a 4-byte fill of a scratch buffer.
  WMT::Reference<WMT::Buffer> sample_scratch_;
  wmtcmd_blit_fillbuffer
  ScratchFill() {
    if (!sample_scratch_) {
      WMTBufferInfo info{};
      info.length = 16;
      info.options = WMTResourceOptions(WMTResourceStorageModePrivate | WMTResourceHazardTrackingModeUntracked);
      info.memory.set(nullptr);
      sample_scratch_ = device_->GetMTLDevice().newBuffer(info);
    }
    wmtcmd_blit_fillbuffer fill = {};
    fill.type = WMTBlitCommandFillBuffer;
    fill.next.set(nullptr);
    fill.buffer = sample_scratch_.handle;
    fill.length = 4;
    return fill;
  }

  // A timestamp taken right after the earlier work, in a blit of its own (with the scratch fill). For counter buffers
  // an encoder can't take beside another (GPU efficiency spec E2). DXMT_STATS counts why (LoneReason).
  void
  SampleAfter(WMT::CommandBuffer &cmdbuf, obj_handle_t sample_buffer, uint64_t index, LoneReason why) {
    DXMT_STAT_COUNT("#timestamps given their own encoder", 1);
    if (g_stats_on) {
      static const unsigned ids[] = {StatId("#lone timestamps (another counter buffer)"),
                                     StatId("#lone timestamps (several waiting)"),
                                     StatId("#lone timestamps (next encoder takes none)"),
                                     StatId("#lone timestamps (end of call)")};
      StatCount(ids[why]);
    }
    auto fill = ScratchFill();
    WMTSampleBufferAttachmentInfo attachment = {sample_buffer, ~0ull, index};
    auto blit = cmdbuf.blitCommandEncoderWithSampleBuffers(&attachment, 1);
    auto fence = Join(blit);
    blit.encodeCommands((const wmtcmd_blit_nop *)&fill);
    blit.updateFence(fences_[fence]);
    blit.endEncoding();
  }

  // MacNeutron: a blit or compute encoder left without commands (a Dispatch with no pipeline still opens one) goes as a
  // blit with the scratch fill, so its waits, its fence update and its samples happen.
  void
  EncodeEmpty(WMT::CommandBuffer &cmdbuf, EncoderData *current, unsigned pass, bool strict,
              WMTSampleBufferAttachmentInfo *samples, unsigned num_samples) {
    FlushPending(cmdbuf); // E4
    auto fill = ScratchFill();
    auto encoder = num_samples ? cmdbuf.blitCommandEncoderWithSampleBuffers(samples, num_samples)
                               : cmdbuf.blitCommandEncoder();
    LabelPass(encoder, pass, "empty");
    Encode(encoder, Order(current, strict), (wmtcmd_blit_nop *)&fill, (wmtcmd_base *)&fill);
    encoder.endEncoding();
  }

  // The queue's own encoders (pass dumps, pixel history, presents) join. Returns the fence the encoder then updates.
  template <typename Encoder, typename... Stage>
  uint16_t
  Join(Encoder &encoder, Stage... before) {
    uint16_t fence = Order(nullptr, true);
    for (auto wait : waits_)
      encoder.waitForFence(fences_[wait], before...);
    return fence;
  }

  // The waits in waits_, an encoder's commands and the update of `fence` (and of `early`, after the `before` stages,
  // for a render pass) go to winemetal as one chained command list: each call crosses from Windows code to the Metal
  // side (several us under Rosetta). `head` null: the waits and the updates alone. The chain is unlinked again, as
  // pixel history re-encodes the commands. Render passes wait before `before` and update after the fragment stage.
  template <typename FenceOp, auto Wait, auto Update, typename Encoder, typename Nop>
  void
  EncodeOrdered(Encoder &encoder, std::vector<FenceOp> &ops, uint16_t fence, Nop *head, wmtcmd_base *tail,
                WMTRenderStages before = WMTRenderStageVertex, uint16_t early = kNoFence) {
    size_t n = waits_.size(), all = n + (early == kNoFence ? 1 : 2);
    ops.assign(all, FenceOp{});
    for (size_t i = 0; i < all; i++) {
      ops[i].type = i < n ? Wait : Update;
      ops[i].fence = fences_[i < n ? waits_[i] : i == n ? fence : early].handle;
      if constexpr (std::is_same_v<FenceOp, wmtcmd_render_fence_op>)
        ops[i].stages = i == n ? WMTRenderStageFragment : before;
      if (i + 1 < all && i + 1 != n)
        ops[i].next.set(&ops[i + 1]);
    }
    void *body = head ? (void *)head : (void *)&ops[n];
    if (n)
      ops[n - 1].next.set(body);
    if (tail)
      tail->next.set(&ops[n]);
    encoder.encodeCommands((const Nop *)(n ? (void *)ops.data() : body));
    if (tail)
      tail->next.set(nullptr);
  }

  void
  Encode(WMT::RenderCommandEncoder &encoder, uint16_t fence, WMTRenderStages before,
         wmtcmd_render_nop *head = nullptr, wmtcmd_base *tail = nullptr, uint16_t early = kNoFence) {
    EncodeOrdered<wmtcmd_render_fence_op, WMTRenderCommandWaitForFence, WMTRenderCommandUpdateFence>(
        encoder, render_ops_, fence, head, tail, before, early);
  }

  void
  Encode(WMT::BlitCommandEncoder &encoder, uint16_t fence, wmtcmd_blit_nop *head, wmtcmd_base *tail) {
    EncodeOrdered<wmtcmd_blit_fence_op, WMTBlitCommandWaitForFence, WMTBlitCommandUpdateFence>(encoder, blit_ops_,
                                                                                               fence, head, tail);
  }

  void
  Encode(WMT::ComputeCommandEncoder &encoder, uint16_t fence, wmtcmd_compute_nop *head, wmtcmd_base *tail) {
    EncodeOrdered<wmtcmd_compute_fence_op, WMTComputeCommandWaitForFence, WMTComputeCommandUpdateFence>(
        encoder, compute_ops_, fence, head, tail);
  }

  // Metal frame capture (MacNeutron; as DXMT's D3D11 queue: MTL_CAPTURE_ENABLED=1 and DXMT_CAPTURE_EXECUTABLE=<exe>,
  // then F10 or DXMT_CAPTURE_FRAME=<n>). Frames are counted by presents.
  CaptureState capture_state_;
  uint64_t presented_frames_ = 0;

  // Pass dump (MacNeutron capture mode, DXMT_DXIL_DUMP=<folder>): with DXMT_DUMP_FRAME=<n>, frame n's render pass
  // attachments, copied into shared buffers and saved as pass-<index>-<attachment>-<width>x<height>-<pixel format>.raw
  // eight frames later, with passes.txt listing every pass of the frame (and its queue). For finding which pass renders
  // wrong. DXMT_DUMP_FRAME=F9 dumps the frame after each press of F9 instead, over the previous dump. Shared by the
  // queues and counted by any queue's presents: Unreal presents through AMD's FSR 3 proxy, from the proxy's own queue.
  struct PassDump {
    WMT::Reference<WMT::Buffer> buffer;
    WMTBufferInfo info;
    std::string name;
  };
  struct PassDumps {
    std::mutex mutex;
    std::atomic_uint64_t frames = 0;
    std::atomic_uint64_t frame = ~0ull;
    uint64_t count = 1; // DXMT_DUMP_FRAMES: consecutive frames per dump (a one-frame glitch is hard to catch with F9)
    bool on_key = false;
    uint32_t saved = 0; // the last f9-<n> folder used; the next F9 takes the first free one after it
    uint32_t passes = 0, queues = 0;
    uint64_t bytes = 0;
    std::vector<PassDump> dumps;
    std::string log;
    // Pixel history (DXMT_DUMP_PIXEL=<x>,<y>[+<x>,<y>...][,<first pass>,<last pass>[,seq]]): each draw of the dumped frame's render
    // passes redrawn into scratch attachments, from the pass's starting state; pixels.txt lists the draws that change
    // the pixel, with their pipeline, and draws.txt every draw of the passes redrawn. Alone by default (a draw an
    // earlier draw would hide is listed too); with seq, each draw on top of the pass's earlier ones, as the pass draws
    // them (quadratic in the pass's draws). The redraws are full size: narrow the passes for a big frame.
    bool pixel_on = false, pixel_seq = false;
    std::string pixel_notes, pixel_draws;
    std::vector<std::pair<uint32_t, uint32_t>> pixels; // the pixels watched
    uint32_t pixel_first = 0, pixel_last = ~0u;
    struct PixelPass {
      std::string pass;
      bool seq = false;
      std::vector<std::pair<uint32_t, uint32_t>> points; // the watched pixels inside this pass
      std::vector<std::string> draws;       // each draw's pipeline
      std::vector<uint32_t> colors, texels; // the color attachments watched, and their texel sizes
      WMT::Reference<WMT::Buffer> buffer;   // 16 bytes per color: the pass without draws, then each draw alone
      WMTBufferInfo info;
    };
    std::vector<PixelPass> pixel_passes;
    std::vector<WMT::Reference<WMT::Texture>> pixel_textures; // scratch, kept until the dumped frame completes

    bool
    Dumping() { // the frame now encoding is one of the dumped ones
      uint64_t f = frame;
      return f != ~0ull && frames - f < count;
    }

    PassDumps() {
      auto value = env::getEnvVar("DXMT_DUMP_FRAME");
      if (value.empty() || !DXILCaptureMode())
        return;
      on_key = value == "F9";
      try {
        frame = std::stoull(value);
      } catch (const std::invalid_argument &) {
      }
      try {
        count = std::max<uint64_t>(1, std::stoull(env::getEnvVar("DXMT_DUMP_FRAMES")));
      } catch (const std::exception &) {
      }
      // Points joined by '+'; the pass range and mode follow the last one.
      std::string spec = env::getEnvVar("DXMT_DUMP_PIXEL");
      for (size_t start = 0; start < spec.size();) {
        size_t plus = spec.find('+', start);
        std::string token = spec.substr(start, plus == std::string::npos ? std::string::npos : plus - start);
        unsigned x, y, first, last;
        char mode[4] = {};
        int fields = sscanf(token.c_str(), "%u,%u,%u,%u,%3s", &x, &y, &first, &last, mode);
        if (fields < 2)
          break;
        pixels.push_back({x, y});
        if (fields >= 4) {
          pixel_first = first;
          pixel_last = last;
        }
        if (fields == 5)
          pixel_seq = !strcmp(mode, "seq");
        if (plus == std::string::npos)
          break;
        start = plus + 1;
      }
      pixel_on = !pixels.empty();
    }
  };
  static PassDumps &
  Dumps() {
    static PassDumps dumps;
    return dumps;
  }
  uint32_t queue_index_ = Dumps().queues++;

  void
  DumpAttachment(WMT::CommandBuffer &cmdbuf, const std::string &name, const TextureViewRef &view, uint32_t level,
                 uint32_t slice, uint32_t depth_plane, WMTBlitOption option) {
    auto &d = Dumps();
    auto texture = view.texture();
    auto format = texture.pixelFormat();
    uint64_t width = std::max<uint64_t>(1, texture.width() >> level), height = std::max<uint64_t>(1, texture.height() >> level);
    uint32_t texel = option ? 4 : MTLGetTexelSize(format);
    uint32_t samples = view->allocation && view->allocation->descriptor ? view->allocation->descriptor->sampleCount() : 1;
    std::string file = name + "-" + std::to_string(width) + "x" + std::to_string(height) + "-" + std::to_string(format);
    d.log += " " + file + (samples > 1 ? "-msaa" : "") + "@" + std::to_string(view->allocation ? (uint64_t)view->allocation->texture().handle : 0);
    uint64_t size = width * height * texel;
    // At most 3 GB of attachments per dump, 12 GB for a burst of frames (DXMT_DUMP_FRAMES), held in memory until saved.
    // ponytail: a fixed budget; save each frame as it completes if bursts need more.
    if (!texel || samples > 1 || d.bytes + size > (d.count > 1 ? 12ull << 30 : 3ull << 30))
      return;
    PassDump dump;
    dump.info.length = size;
    dump.info.options = WMTResourceStorageModeShared;
    dump.info.memory.set(0);
    dump.buffer = device_->GetMTLDevice().newBuffer(dump.info);
    if (!dump.buffer || !dump.info.memory.get_accessible_or_null())
      return;
    struct wmtcmd_blit_copy_from_texture_to_buffer_withblitoption cmd = {};
    cmd.type = WMTBlitCommandCopyFromTextureToBufferWithBlitOption;
    cmd.next.set(nullptr);
    cmd.src = texture.handle;
    cmd.slice = slice;
    cmd.level = level;
    cmd.origin = {0, 0, depth_plane};
    cmd.size = {width, height, 1};
    cmd.dst = dump.buffer.handle;
    cmd.offset = 0;
    cmd.bytes_per_row = width * texel;
    cmd.bytes_per_image = size;
    cmd.options = option;
    auto blit = cmdbuf.blitCommandEncoder();
    auto fence = Join(blit);
    blit.encodeCommands((const wmtcmd_blit_nop *)&cmd);
    blit.updateFence(fences_[fence]);
    blit.endEncoding();
    dump.name = file + ".raw";
    d.bytes += size;
    d.dumps.push_back(std::move(dump));
  }

  uint64_t execute_calls_ = 0; // ExecuteCommandLists calls on this queue: a pass dump names each pass's (B7)

  void
  DumpPass(WMT::CommandBuffer &cmdbuf, EncoderData *pass) {
    auto &d = Dumps();
    std::lock_guard<std::mutex> lock(d.mutex);
    std::string name = "pass-" + std::to_string(d.passes++);
    static const char *const kinds[] = {"null", "clear", "render", "blit", "compute", "resolve", "upscale"};
    d.log += name + " q" + std::to_string(queue_index_) + " x" + std::to_string(execute_calls_) + " " +
             ((unsigned)pass->type < 7 ? kinds[(unsigned)pass->type] : "other");
    // MacNeutron: what a blit does (B7, Palworld's base-pass segments split by blits): its commands by kind, with a
    // buffer copy's length, and whether it only takes timestamps.
    if (pass->type == EncoderType::Blit) {
      auto data = static_cast<BlitEncoderData *>(pass);
      static const char *const ops[] = {"nop",   "buf>buf",     "buf>tex",      "tex>buf",   "tex>tex",
                                        "mips",  "waitfence",   "updatefence",  "fill",      "counters",
                                        "buf>tex+opt", "tex>buf+opt", "reset-icb", "copytex"};
      if (pass->timestamp_only)
        d.log += " timestamps";
      if (pass->query_resolve_only)
        d.log += " query-resolve";
      for (auto *cmd = (wmtcmd_base *)data->cmd_head.next.get(); cmd; cmd = (wmtcmd_base *)cmd->next.get()) {
        d.log += std::string(" ") + (cmd->type < std::size(ops) ? ops[cmd->type] : "?");
        if (cmd->type == WMTBlitCommandCopyFromBufferToBuffer)
          d.log += ":" + std::to_string(((wmtcmd_blit_copy_from_buffer_to_buffer *)cmd)->copy_length);
      }
    }
    if (pass->type == EncoderType::Render) {
      auto data = static_cast<RenderEncoderData *>(pass);
      for (unsigned i = 0; i < data->colors.size(); i++) {
        auto &c = data->colors[i];
        if (c.attachment)
          DumpAttachment(cmdbuf, name + "-c" + std::to_string(i), c.attachment, c.level, c.slice, c.depth_plane, WMTBlitOptionNone);
      }
      if (data->depth.attachment) {
        auto format = data->depth.attachment.texture().pixelFormat();
        DumpAttachment(cmdbuf, name + "-d", data->depth.attachment, data->depth.level, data->depth.slice, 0,
                       format == WMTPixelFormatDepth32Float_Stencil8 ? WMTBlitOptionDepthFromDepthStencil : WMTBlitOptionNone);
      }
    }
    d.log += "\n";
  }

  // Pixel history for one render pass of the dumped frame (PassDumps). Encoded before the pass, into scratch textures
  // only, so the frame itself renders as it would have.
  void
  PixelHistory(WMT::CommandBuffer &cmdbuf, RenderEncoderData *data, const WMTRenderPassInfo &real) {
    auto &d = Dumps();
    std::lock_guard<std::mutex> lock(d.mutex);
    uint32_t pass = d.passes; // DumpPass numbers this pass once it is encoded
    if (!d.pixel_on || pass < d.pixel_first || pass > d.pixel_last)
      return;
    auto skipped = [&](const char *why) { d.pixel_notes += "# pass-" + std::to_string(pass) + " not redrawn: " + why + "\n"; };
    if (data->render_target_array_length > 1)
      return skipped("layered");
    // The draws, each under the pipeline set before it; and the visibility commands, which mustn't count into a
    // query heap on a redraw.
    std::vector<wmtcmd_base *> draws, visibility;
    std::vector<std::string> names;
    std::string pipeline = "(no pipeline)";
    for (auto *cmd = (wmtcmd_base *)&data->cmd_head; cmd; cmd = (wmtcmd_base *)cmd->next.get()) {
      switch (cmd->type) {
      case WMTRenderCommandSetPSO:
        pipeline = PipelineName(((wmtcmd_render_setpso *)cmd)->pso);
        break;
      case WMTRenderCommandSetVisibilityMode:
        visibility.push_back(cmd);
        break;
      case WMTRenderCommandDraw:
      case WMTRenderCommandDrawIndexed:
      case WMTRenderCommandDrawIndirect:
      case WMTRenderCommandDrawIndexedIndirect:
      case WMTRenderCommandDrawMeshThreadgroups:
      case WMTRenderCommandDrawMeshThreadgroupsIndirect:
      case WMTRenderCommandDXMTGeometryDraw:
      case WMTRenderCommandDXMTGeometryDrawIndexed:
      case WMTRenderCommandDXMTGeometryDrawIndirect:
      case WMTRenderCommandDXMTGeometryDrawIndexedIndirect:
      case WMTRenderCommandDXMTTessellationMeshDraw:
      case WMTRenderCommandDXMTTessellationMeshDrawIndexed:
      case WMTRenderCommandDXMTTessellationMeshDrawIndirect:
      case WMTRenderCommandDXMTTessellationMeshDrawIndexedIndirect:
      case WMTRenderCommandDispatchThreadsPerTile:
      case WMTRenderCommandExecuteCommandsInBuffer:
        draws.push_back(cmd);
        names.push_back(pipeline);
        break;
      default:
        break;
      }
    }
    if (draws.empty())
      return;
    for (size_t k = 0; k < names.size(); k++)
      d.pixel_draws += "pass-" + std::to_string(pass) + " draw-" + std::to_string(k) + " " + names[k] + "\n";

    auto metal = device_->GetMTLDevice();
    auto scratch = [&](WMTPixelFormat format, uint64_t width, uint64_t height) {
      WMTTextureInfo t = {};
      t.pixel_format = format;
      t.type = WMTTextureType2D;
      t.width = width;
      t.height = height;
      t.depth = 1;
      t.array_length = 1;
      t.mipmap_level_count = 1;
      t.sample_count = 1;
      t.usage = WMTTextureUsage(WMTTextureUsageRenderTarget | WMTTextureUsageShaderRead);
      t.options = WMTResourceOptions(WMTResourceStorageModePrivate);
      auto texture = metal.newTexture(t);
      d.pixel_textures.push_back(texture);
      return texture;
    };
    // The redraw's pass: scratch attachments, loaded (a DontCare load reads the pixel as it was) and stored.
    WMTRenderPassInfo info = real;
    info.visibility_buffer = 0;
    info.visibility_accumulate = false;
    for (auto &sample : info.sample_buffers)
      sample = {};
    struct Watched {
      WMT::Texture texture;
      uint32_t slice, level, plane;
      WMT::Reference<WMT::Texture> scratch, before; // before: one texel per watched pixel, in a row
      WMTPixelFormat format;
    };
    uint64_t width = ~0ull, height = ~0ull; // the smallest color attachment
    std::vector<Watched> colors;
    PassDumps::PixelPass record;
    record.pass = "pass-" + std::to_string(pass);
    record.draws = names;
    for (unsigned i = 0; i < std::size(info.colors); i++) {
      auto &c = data->colors[i];
      if (!c.attachment)
        continue;
      if (c.attachment->allocation && c.attachment->allocation->descriptor &&
          c.attachment->allocation->descriptor->sampleCount() > 1)
        return skipped("multisampled"); // ponytail: no pixel history in multisampled passes
      auto texture = c.attachment.texture();
      uint64_t w = std::max<uint64_t>(1, texture.width() >> c.level), h = std::max<uint64_t>(1, texture.height() >> c.level);
      width = std::min(width, w);
      height = std::min(height, h);
      auto format = texture.pixelFormat();
      colors.push_back({texture, c.slice, c.level, c.depth_plane, scratch(format, w, h), {}, format});
      auto &color = info.colors[i];
      color.texture = colors.back().scratch.handle;
      color.level = color.slice = color.depth_plane = 0;
      color.resolve_texture = 0;
      color.store_action = WMTStoreActionStore;
      if (color.load_action == WMTLoadActionDontCare)
        color.load_action = WMTLoadActionLoad;
      record.colors.push_back(i);
      record.texels.push_back(MTLGetTexelSize(format));
    }
    if (colors.empty())
      return skipped("no color attachment"); // ponytail: colors only
    for (auto &point : d.pixels)
      if (point.first < width && point.second < height)
        record.points.push_back(point);
    if (record.points.empty())
      return skipped("the pixels lie outside");
    for (auto &w : colors)
      w.before = scratch(w.format, record.points.size(), 1);
    // Depth and stencil (one texture in D3D12) are copied whole: Metal copies depth-stencil textures whole.
    WMT::Texture depth_texture = {};
    WMT::Reference<WMT::Texture> depth_scratch, depth_before;
    uint32_t depth_slice = 0, depth_level = 0;
    uint64_t depth_width = 0, depth_height = 0;
    if (data->stencil.attachment &&
        (!data->depth.attachment || data->stencil.attachment.texture().handle != data->depth.attachment.texture().handle))
      return skipped("separate stencil texture"); // ponytail: not redrawn
    if (data->depth.attachment) {
      depth_texture = data->depth.attachment.texture();
      depth_slice = data->depth.slice;
      depth_level = data->depth.level;
      depth_width = std::max<uint64_t>(1, depth_texture.width() >> depth_level);
      depth_height = std::max<uint64_t>(1, depth_texture.height() >> depth_level);
      depth_scratch = scratch(depth_texture.pixelFormat(), depth_width, depth_height);
      depth_before = scratch(depth_texture.pixelFormat(), depth_width, depth_height);
      auto redirect = [&](auto &plane) {
        if (!plane.texture)
          return;
        plane.texture = depth_scratch.handle;
        plane.level = plane.slice = plane.depth_plane = 0;
        plane.store_action = WMTStoreActionStore;
        if (plane.load_action == WMTLoadActionDontCare)
          plane.load_action = WMTLoadActionLoad;
      };
      redirect(info.depth);
      redirect(info.stencil);
    }
    // 16 bytes per color per point per redraw.
    const uint64_t slot = 16, stride = slot * colors.size() * record.points.size();
    record.info.length = stride * (draws.size() + 1);
    record.info.options = WMTResourceStorageModeShared;
    record.info.memory.set(0);
    record.buffer = metal.newBuffer(record.info);
    if (!record.buffer || !record.info.memory.get_accessible_or_null())
      return skipped("no readback buffer");
    record.seq = d.pixel_seq;

    auto copy = [&](WMT::BlitCommandEncoder &blit, obj_handle_t src, uint32_t slice, uint32_t level, WMTOrigin from,
                    WMTSize size, obj_handle_t dst, WMTOrigin to) {
      wmtcmd_blit_copy_from_texture_to_texture cmd = {};
      cmd.type = WMTBlitCommandCopyFromTextureToTexture;
      cmd.next.set(nullptr);
      cmd.src = src;
      cmd.src_slice = slice;
      cmd.src_level = level;
      cmd.src_origin = from;
      cmd.src_size = size;
      cmd.dst = dst;
      cmd.dst_origin = to;
      blit.encodeCommands((const wmtcmd_blit_nop *)&cmd);
    };
    auto read = [&](WMT::BlitCommandEncoder &blit, obj_handle_t src, uint32_t x, uint32_t y, uint32_t texel,
                    uint64_t offset) {
      wmtcmd_blit_copy_from_texture_to_buffer_withblitoption cmd = {};
      cmd.type = WMTBlitCommandCopyFromTextureToBufferWithBlitOption;
      cmd.next.set(nullptr);
      cmd.src = src;
      cmd.origin = {x, y, 0};
      cmd.size = {1, 1, 1};
      cmd.dst = record.buffer.handle;
      cmd.offset = offset;
      cmd.bytes_per_row = texel;
      cmd.bytes_per_image = texel;
      cmd.options = WMTBlitOptionNone;
      blit.encodeCommands((const wmtcmd_blit_nop *)&cmd);
    };
    // The pixels (and the whole depth) as they are before the pass.
    {
      auto blit = cmdbuf.blitCommandEncoder();
      auto before = Join(blit);
      for (auto &w : colors)
        for (uint32_t p = 0; p < record.points.size(); p++)
          copy(blit, w.texture.handle, w.slice, w.level, {record.points[p].first, record.points[p].second, w.plane},
               {1, 1, 1}, w.before.handle, {p, 0, 0});
      if (depth_before)
        copy(blit, depth_texture.handle, depth_slice, depth_level, {0, 0, 0}, {depth_width, depth_height, 1},
             depth_before.handle, {0, 0, 0});
      blit.updateFence(fences_[before]);
      blit.endEncoding();
    }
    std::vector<uint16_t> types;
    for (auto *cmd : draws)
      types.push_back(cmd->type);
    for (auto *cmd : visibility)
      cmd->type = WMTRenderCommandNop;
    // 0: no draw (the pass's clears alone); k: draw k - 1 alone, or draws 0 to k - 1 in sequence.
    for (size_t k = 0; k <= draws.size(); k++) {
      auto blit = cmdbuf.blitCommandEncoder();
      auto restored = Join(blit);
      for (auto &w : colors)
        for (uint32_t p = 0; p < record.points.size(); p++)
          copy(blit, w.before.handle, 0, 0, {p, 0, 0}, {1, 1, 1}, w.scratch.handle,
               {record.points[p].first, record.points[p].second, 0});
      if (depth_before)
        copy(blit, depth_before.handle, 0, 0, {0, 0, 0}, {depth_width, depth_height, 1}, depth_scratch.handle, {0, 0, 0});
      blit.updateFence(fences_[restored]);
      blit.endEncoding();
      for (size_t j = 0; j < draws.size(); j++)
        draws[j]->type = (record.seq ? j + 1 <= k : j + 1 == k) ? types[j] : (uint16_t)WMTRenderCommandNop;
      auto encoder = cmdbuf.renderCommandEncoder(info);
      auto drawn = Join(encoder, data->use_geometry ? WMTRenderStagePreRaster : WMTRenderStageVertex);
      encoder.encodeCommands(&data->cmd_head);
      encoder.updateFence(fences_[drawn], WMTRenderStageFragment);
      encoder.endEncoding();
      auto readback = cmdbuf.blitCommandEncoder();
      auto copied = Join(readback);
      for (size_t p = 0; p < record.points.size(); p++)
        for (size_t i = 0; i < colors.size(); i++)
          read(readback, colors[i].scratch.handle, record.points[p].first, record.points[p].second, record.texels[i],
               k * stride + (p * colors.size() + i) * slot);
      readback.updateFence(fences_[copied]);
      readback.endEncoding();
    }
    for (size_t j = 0; j < draws.size(); j++)
      draws[j]->type = types[j];
    for (auto *cmd : visibility)
      cmd->type = WMTRenderCommandSetVisibilityMode;
    d.pixel_notes += "# " + record.pass + ": " + std::to_string(draws.size()) + " draws redrawn" +
                     (record.seq ? " in sequence\n" : " alone\n");
    d.pixel_passes.push_back(std::move(record));
  }

  static void
  SaveDumps() {
    auto &d = Dumps();
    std::lock_guard<std::mutex> lock(d.mutex);
    if (d.log.empty())
      return;
    // F9 mode: each dump in its own new folder, f9-1, f9-2, ..., after any a previous run left.
    std::string dir;
    if (d.on_key) {
      do
        dir = "f9-" + std::to_string(++d.saved);
      while (!MakeCaptureFolder(dir.c_str()));
      dir += "\\";
    }
    for (auto &dump : d.dumps)
      SaveCapture((dir + dump.name).c_str(), dump.info.memory.get_accessible_or_null(), dump.info.length);
    if (d.pixel_on) {
      // "pass-<n> draw-<k> <pipeline> c<i> <before>-><after> ...": the draws that change the pixel. Before: the pass
      // without draws (alone), or after the draw before (in sequence).
      std::string text = d.pixel_notes;
      for (auto &p : d.pixel_passes) {
        auto *bytes = (const uint8_t *)p.info.memory.get_accessible_or_null();
        size_t colors = p.colors.size(), stride = 16 * colors * p.points.size();
        auto hex = [](const uint8_t *b, uint32_t n) {
          std::string s;
          char digits[3];
          for (uint32_t t = 0; t < n; t++) {
            snprintf(digits, sizeof(digits), "%02x", b[t]);
            s += digits;
          }
          return s;
        };
        for (size_t k = 1; k <= p.draws.size(); k++)
          for (size_t q = 0; q < p.points.size(); q++) {
            std::string changes;
            for (size_t i = 0; i < colors; i++) {
              size_t at = (q * colors + i) * 16;
              const uint8_t *without = bytes + (p.seq ? k - 1 : 0) * stride + at, *with = bytes + k * stride + at;
              if (memcmp(without, with, p.texels[i]))
                changes += " c" + std::to_string(p.colors[i]) + " " + hex(without, p.texels[i]) + "->" + hex(with, p.texels[i]);
            }
            if (!changes.empty())
              text += p.pass + " draw-" + std::to_string(k - 1) + " " + p.draws[k - 1] + changes + " at " +
                      std::to_string(p.points[q].first) + "," + std::to_string(p.points[q].second) + "\n";
          }
      }
      SaveCapture((dir + "pixels.txt").c_str(), text.data(), text.size());
      SaveCapture((dir + "draws.txt").c_str(), d.pixel_draws.data(), d.pixel_draws.size());
      d.pixel_notes.clear();
      d.pixel_draws.clear();
      d.pixel_passes.clear();
      d.pixel_textures.clear();
    }
    SaveCapture((dir + "passes.txt").c_str(), d.log.data(), d.log.size());
    d.dumps.clear();
    d.log.clear();
    d.passes = 0;
    d.bytes = 0;
    if (d.on_key)
      d.frame = ~0ull;
  }

  std::atomic_uint64_t inflight_cmdbuf_seq_ = 1;
  std::atomic_uint64_t inflight_cmdbuf_count_ = 0;
  std::atomic_uint64_t inflight_cmdbuf_stop_ = 0;

  // A fence signal deferred to the CPU (MacNeutron): to the generation it was asked of; `forward`: its MTLEvent signal
  // too (held back behind timestamps resolved into a custom heap, which the GPU can read).
  struct DeferredSignal {
    Rc<Fence> fence;
    Fence::Generation gen;
    uint64_t value;
    bool forward;
  };
  struct InflightCommandBuffer {
    WMT::Reference<WMT::CommandBuffer> cmdbuf{};
    HANDLE semaphore{};
    // Done on the CPU once it completes (MacNeutron): waits for the shared events of the fences its queue waited on
    // through their MTLEvents (GPU overlap spec §3.11), the sampled timestamps' values (Task F4) and timestamp
    // resolves, then the fence signals deferred behind them, so no fence reports the work done before its timestamps
    // are written, or before a fence it waited on.
    bool owes_cpu = false;
    std::vector<std::pair<WMT::Reference<WMT::SharedEvent>, uint64_t>> waits{};
    std::vector<TimestampResolve> resolves{};
    std::vector<DeferredSignal> signals{};
    // Task F4: its slots of the queue's counter buffer (ring_), [first, first + count) modulo its size, and the
    // queries' values each one goes to, by index in that range.
    obj_handle_t samples = 0;
    uint64_t sample_first = 0;
    uint32_t sample_count = 0;
    std::vector<std::pair<uint32_t, uint64_t *>> sample_values{};
  };
  // Command buffers still owing CPU work: while there are any, fence signals go behind them.
  std::atomic_uint32_t cpu_work_ = 0;
  bool custom_resolves_ = false; // ponytail: sticky once a list resolved timestamps into a custom heap

  void
  Owe(InflightCommandBuffer &inflight) { // under mutex_commit_
    if (inflight.owes_cpu)
      return;
    inflight.owes_cpu = true;
    cpu_work_.fetch_add(1, std::memory_order_relaxed);
  }

  std::array<InflightCommandBuffer, kCommandQueueSize> inflight_cmdbuf_pool_;
  // The slots taken from the pool (B6, DXMT_D3D12_CMDBUF_SLOTS), at most kCommandQueueSize. Declared before the wait
  // thread, which reads it as it starts.
  unsigned cmdbuf_slots_ = kCommandQueueSlots;
  dxmt::thread inflight_cmdbuf_wait_thread_;

  // Task F4 (Ruling 23): the queue samples every timestamp, whatever its query heap, into one counter buffer, so any
  // encoder can take them: those waiting before it at its start, its own (and those after it that no encoder start can
  // take) at its end, in one attachment. A ring of kTimestampsPerBuffer slots (Metal allows 32 counter buffers per
  // process, and each timestamp query heap holds one): each call takes the next ones, free again once its command
  // buffer completed and its values went to their queries (which resolves copy). A lone blit is left only where no
  // encoder in the call can take a timestamp. DXMT_D3D12_TIMESTAMP_BLITS=1, or no buffer left for the ring: the query
  // heaps' own counter buffers, one per encoder (GPU efficiency spec E2, E4).
  WMT::Reference<WMT::CounterSampleBuffer> ring_;
  bool ring_failed_ = false;
  uint64_t ring_next_ = 0;             // slots taken (under mutex_commit_)
  std::atomic_uint64_t ring_done_ = 0; // slots read back (by the waiting thread)
  InflightCommandBuffer *sampling_ = nullptr; // the call's command buffer while it encodes, sampling into ring_
  bool taken_ = false;               // the timestamp-only blits before taken_stop_ rode an earlier encoder's end
  EncoderData *taken_stop_ = nullptr; // (null: to the end of the call)
  std::vector<EncoderData::Sample> ends_; // scratch: the samples for an encoder's end

  // One slot of the ring for `n` samples taken at one point; ~0 for none.
  uint64_t
  Point(const EncoderData::Sample *samples, size_t n) {
    if (!n)
      return ~0ull;
    // ponytail: 4096 sample points in flight (about 200 a frame in Unreal); past that they keep their last values
    if (ring_next_ - ring_done_.load(std::memory_order_acquire) == kTimestampsPerBuffer) {
      static bool warned = false;
      if (!std::exchange(warned, true))
        WARN("D3D12CommandQueue: over ", kTimestampsPerBuffer, " timestamps in flight; later ones keep old values");
      return ~0ull;
    }
    if (!sampling_->sample_count) {
      sampling_->sample_first = ring_next_;
      Owe(*sampling_); // the waiting thread writes the queries' values: no fence passes before (the app may free them)
    }
    for (size_t i = 0; i < n; i++)
      sampling_->sample_values.push_back({sampling_->sample_count, samples[i].value});
    sampling_->sample_count++;
    return ring_next_++ % kTimestampsPerBuffer;
  }

  // The waiting timestamps' slot, for the start of the encoder being encoded; ~0 for none.
  uint64_t
  TakeStart() {
    uint64_t slot = Point(pending_samples_.data(), pending_samples_.size());
    if (slot != ~0ull)
      DXMT_STAT_COUNT("#timestamps at the next encoder's start", 1);
    pending_samples_.clear();
    return slot;
  }

  // Adds to ends_ the timestamp-only blits right after `e` (of lists[l], across the call's later lists) when the
  // encoder after them takes no start sample (a clear or resolve, which wait before their fragment stage, or none: the
  // call's end), so they ride e's end; the main loop then skips them.
  void
  Trailing(EncoderData *e, unsigned l, ID3D12CommandList *const *lists, unsigned count, bool folding) {
    size_t mark = ends_.size();
    EncoderData *cursor = e->next;
    for (;;) {
      if (!cursor) {
        if (++l == count)
          break;
        cursor = static_cast<MTLD3D12GraphicsCommandList *>(lists[l])->entry;
      } else if (cursor->type == EncoderType::Null) {
        cursor = cursor->next;
      } else if (folding && cursor->type == EncoderType::Blit && cursor->timestamp_only) {
        ends_.insert(ends_.end(), cursor->samples, cursor->samples + cursor->num_samples);
        cursor = cursor->next;
      } else {
        break;
      }
    }
    if (cursor && cursor->type != EncoderType::Clear && cursor->type != EncoderType::Resolve)
      ends_.resize(mark);
    else if (ends_.size() > mark)
      taken_ = true, taken_stop_ = cursor;
  }

  // A blit or compute encoder's one attachment (in `at`): the waiting timestamps at its start, its own and Trailing's
  // at its end. Returns the attachment count.
  unsigned
  Attach(EncoderData *e, unsigned l, ID3D12CommandList *const *lists, unsigned count, bool folding,
         WMTSampleBufferAttachmentInfo &at) {
    uint64_t start = TakeStart();
    ends_.assign(e->samples, e->samples + e->num_samples);
    Trailing(e, l, lists, count, folding);
    uint64_t end = Point(ends_.data(), ends_.size());
    at = {sampling_->samples, start, end};
    return start != ~0ull || end != ~0ull;
  }

  dxmt::mutex mutex_commit_;

  std::vector<uint64_t> sample_scratch_values_; // the waiting thread's

  void
  CommandBufferWaitingThread() {
    env::setThreadName("dxmt-cmdbuf-waiting-thread");
    SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_TIME_CRITICAL);

    uint64_t internal_seq = 1;
    for (;;) {
      inflight_cmdbuf_seq_.wait(internal_seq, std::memory_order_acquire);
      if (inflight_cmdbuf_stop_.load() == internal_seq)
        break;
      auto &inflight = inflight_cmdbuf_pool_[internal_seq % cmdbuf_slots_];

      if (inflight.cmdbuf.status() <= WMTCommandBufferStatusScheduled)
        inflight.cmdbuf.waitUntilCompleted();
      DXMT_STAT_SCOPE("queue.(retire a completed command buffer)");
      if (inflight.cmdbuf.status() == WMTCommandBufferStatusError) {
        ERR("Device error: ", inflight.cmdbuf.error().description().getUTF8String());
        device_->ResetAtomic64Locks(); // a fault inside a 64-bit atomic's critical section would hold its lock (MacNeutron)
      }

      if (inflight.semaphore)
        ReleaseSemaphore(inflight.semaphore, 1, nullptr);

      if (uint32_t n = inflight.sample_count) { // Task F4: each slot's value to its queries, before resolves read them
        auto &v = sample_scratch_values_;
        v.resize(n);
        uint32_t first = inflight.sample_first % kTimestampsPerBuffer, head = std::min(n, kTimestampsPerBuffer - first);
        MTLCounterSampleBuffer_resolveCounterRange(inflight.samples, first, head, v.data(), head * sizeof(uint64_t));
        if (head < n) // wrapped around the ring
          MTLCounterSampleBuffer_resolveCounterRange(inflight.samples, 0, n - head, v.data() + head,
                                                     (n - head) * sizeof(uint64_t));
        for (auto [index, value] : inflight.sample_values)
          *value = v[index];
        ring_done_.store(inflight.sample_first + n, std::memory_order_release);
      }
      if (inflight.owes_cpu) {
        // The GPU passed these MTLEvent waits, so each shared signal is encoded, being retired, or set (spec §3.11).
        // A failed buffer may not have: then its waits are skipped.
        if (inflight.cmdbuf.status() != WMTCommandBufferStatusError)
          for (auto &[shared, value] : inflight.waits)
            shared.waitUntilSignaledValue(value, ~0ull);
        for (auto &r : inflight.resolves) {
          if (r.values) { // Task F4: the values the queue wrote
            memcpy(r.dst, r.values + r.start, r.count * sizeof(uint64_t));
            for (auto [slot, sample] : r.aliases)
              ((uint64_t *)r.dst)[slot] = r.values[sample];
            continue;
          }
          MTLCounterSampleBuffer_resolveCounterRange(r.samples, r.start, r.count, r.dst, r.count * sizeof(uint64_t));
          for (auto [slot, sample] : r.aliases)
            MTLCounterSampleBuffer_resolveCounterRange(r.samples, sample, 1, (uint64_t *)r.dst + slot, sizeof(uint64_t));
        }
        // A failed buffer may not have run its MTLEvent signals: forwarded, every value reaches both events.
        bool failed = inflight.cmdbuf.status() == WMTCommandBufferStatusError;
        for (auto &d : inflight.signals) {
          d.gen.shared.signalValue(d.value);
          if (d.forward || failed)
            d.fence->forward(d.gen, d.value);
        }
        cpu_work_.fetch_sub(1, std::memory_order_release);
      }

      inflight = {};

      inflight_cmdbuf_count_.fetch_sub(1, std::memory_order_release);
      inflight_cmdbuf_count_.notify_one();

      internal_seq++;
    }
  };

  // MacNeutron (GPU overlap spec §3.10): an open Metal command buffer. A queue Wait encodes into it and waits for the
  // next call; ExecuteCommandLists, Signal and Present commit it, so the GPU starts on submitted work at once and
  // nothing ever waits on uncommitted work.
  bool open_ = false;

  // The open command buffer, after taking one of the pool's slots (cmdbuf_slots_) for a new one when none is open
  // (under mutex_commit_).
  InflightCommandBuffer &
  Open() {
    auto &inflight = inflight_cmdbuf_pool_[inflight_cmdbuf_seq_.load(std::memory_order_relaxed) % cmdbuf_slots_];
    if (open_)
      return inflight;
    {
      DXMT_STAT_SCOPE("queue.(wait for a command buffer slot)");
      inflight_cmdbuf_count_.wait(cmdbuf_slots_, std::memory_order_acquire);
    }
    DXMT_STAT_SCOPE("queue.(new command buffer)");
    inflight.cmdbuf = queue_.commandBuffer();
    open_ = true;
    return inflight;
  }

  void
  Commit() { // under mutex_commit_
    if (!open_)
      return;
    DXMT_STAT_SCOPE("queue.(commit)");
    DXMT_STAT_COUNT("#command buffers committed", 1);
    inflight_cmdbuf_pool_[inflight_cmdbuf_seq_.load(std::memory_order_relaxed) % cmdbuf_slots_].cmdbuf.commit();
    open_ = false;
    inflight_cmdbuf_seq_.fetch_add(1, std::memory_order_release);
    inflight_cmdbuf_seq_.notify_one();
    inflight_cmdbuf_count_.fetch_add(1, std::memory_order_relaxed);
  }

  struct CommittingScope {
    MTLD3D12CommandQueueImpl *queue;
    std::lock_guard<dxmt::mutex> lock;
    WMT::Reference<WMT::Object> pool;
    bool commit;
    InflightCommandBuffer &inflight;

    CommittingScope(MTLD3D12CommandQueueImpl *queue, bool commit) :
        queue(queue),
        lock(queue->mutex_commit_),
        pool(WMT::MakeAutoreleasePool()),
        commit(commit),
        inflight(queue->Open()) {}

    ~CommittingScope() {
      if (commit)
        queue->Commit();
    }
  };

  CommittingScope
  StartCommitting(bool commit) {
    return CommittingScope(this, commit);
  }

  // B6: DXMT_D3D12_CMDBUF_SLOTS, the number of command buffers that may be in flight (default kCommandQueueSlots,
  // clamped to [1, kCommandQueueSize]). Read once, before the wait thread starts.
  static unsigned
  SlotsFromEnv() {
    unsigned slots = kCommandQueueSlots;
    if (auto s = env::getEnvVar("DXMT_D3D12_CMDBUF_SLOTS"); !s.empty()) {
      try {
        slots = std::min<unsigned>(std::max<unsigned>(std::stoul(s), 1), kCommandQueueSize);
      } catch (const std::exception &) {
      }
    }
    return slots;
  }

public:
  MTLD3D12CommandQueueImpl(MTLD3D12Device *pDevice) :
      MTLD3D12Pageable<MTLD3D12CommandQueue, IMTLSwapChainFactory>(pDevice), cmdbuf_slots_(SlotsFromEnv()),
      inflight_cmdbuf_wait_thread_([this]() { this->CommandBufferWaitingThread(); }) {
    auto frame = env::getEnvVar("DXMT_CAPTURE_FRAME");
    if (!frame.empty()) {
      try {
        capture_state_.scheduleNextFrameCapture(std::stoull(frame));
      } catch (const std::invalid_argument &) {
      }
    }
  }

  ~MTLD3D12CommandQueueImpl() {
    StopPacing(); // MacNeutron (frame generation): the frames it holds are committed or dropped
    std::lock_guard<dxmt::mutex> lock(mutex_commit_);
    Commit(); // what ExecuteCommandLists left open
    inflight_cmdbuf_stop_.store(inflight_cmdbuf_seq_.fetch_add(1));
    inflight_cmdbuf_seq_.notify_one();
    inflight_cmdbuf_wait_thread_.join();
    SaveDumps(); // a dumped frame's passes completed: a test that never presents, or a game that quit
  }

  HRESULT
  Initialize(const D3D12_COMMAND_QUEUE_DESC *pDesc) {
    // TODO: validate and normalize
    desc_ = *pDesc;
    desc_.NodeMask = 1; // typically 1 GPU only

    auto metal_device = device_->GetMTLDevice();
    queue_ = metal_device.newCommandQueue(cmdbuf_slots_);
    if (!queue_)
      return E_FAIL;
    queue_.addResidencySet(device_->GetGlobalResidencySet());

    for (auto &fence : fences_)
      fence = metal_device.newFence();

    return S_OK;
  }

  HRESULT
  STDMETHODCALLTYPE
  QueryInterface(REFIID riid, void **ppvObject) {
    DXMT_STAT_SCOPE("queue.QueryInterface");
    if (ppvObject == nullptr)
      return E_POINTER;

    *ppvObject = nullptr;

    if (riid == __uuidof(IUnknown) || riid == __uuidof(ID3D12Object) || riid == __uuidof(ID3D12DeviceChild) ||
        riid == __uuidof(ID3D12Pageable) || riid == __uuidof(ID3D12CommandQueue)) {
      *ppvObject = ref(this);
      return S_OK;
    }

    if (riid == __uuidof(IMTLSwapChainFactory)) {
      *ppvObject = ref_and_cast<IMTLSwapChainFactory>(this);
      return S_OK;
    }

    if (riid == __uuidof(ID3D10Device) || riid == __uuidof(ID3D10Device1))
      return E_NOINTERFACE;

    if (riid == __uuidof(ID3D11Device) || riid == __uuidof(ID3D11Device1) || riid == __uuidof(ID3D11Device2) ||
        riid == __uuidof(ID3D11Device3) || riid == __uuidof(ID3D11Device4) || riid == __uuidof(ID3D11Device5))
      return E_NOINTERFACE;

    if (riid == kD3D12CommandQueueDownlevelUUID)
      return E_NOINTERFACE;

    if (logQueryInterfaceError(__uuidof(ID3D12CommandQueue), riid)) {
      WARN("D3D12CommandQueue: Unknown interface query ", str::format(riid));
    }

    return E_NOINTERFACE;
  }

  void STDMETHODCALLTYPE UpdateTileMappings(
      ID3D12Resource *resource, UINT region_count, const D3D12_TILED_RESOURCE_COORDINATE *region_start_coordinates,
      const D3D12_TILE_REGION_SIZE *region_sizes, ID3D12Heap *heap, UINT range_count,
      const D3D12_TILE_RANGE_FLAGS *range_flags, const UINT *heap_range_offsets, const UINT *range_tile_counts,
      D3D12_TILE_MAPPING_FLAGS flags
  ) {
    DXMT_STAT_SCOPE("queue.UpdateTileMappings");
    IMPLEMENT_ME
  };

  void STDMETHODCALLTYPE CopyTileMappings(
      ID3D12Resource *dst_resource, const D3D12_TILED_RESOURCE_COORDINATE *dst_region_start_coordinate,
      ID3D12Resource *src_resource, const D3D12_TILED_RESOURCE_COORDINATE *src_region_start_coordinate,
      const D3D12_TILE_REGION_SIZE *region_size, D3D12_TILE_MAPPING_FLAGS flags
  ) {
    DXMT_STAT_SCOPE("queue.CopyTileMappings");
    IMPLEMENT_ME
  };

  void STDMETHODCALLTYPE
  ExecuteCommandLists(UINT Count, ID3D12CommandList *const *ppCommandLists) {
    DXMT_STAT_SCOPE("queue.ExecuteCommandLists");
    execute_calls_++;
    auto scope = StartCommitting(true); // commits at once: the GPU starts on it (a Wait before it rides along)
    auto &cmdbuf = scope.inflight.cmdbuf;
    bool dumping = Dumps().Dumping();
    bool serial = serial_ || dumping; // F9 dumps and pixel history see the strict order
    bool fold_timestamps = !dumping && merge_; // E4; dumps keep D3D12's passes
    sampling_ = nullptr; // Task F4: the ring, once a call samples timestamps
    if (!TimestampBlits() && !ring_ && !ring_failed_ &&
        std::any_of(ppCommandLists, ppCommandLists + Count,
                    [](auto *l) { return static_cast<MTLD3D12GraphicsCommandList *>(l)->timestamps; })) {
      ring_ = device_->GetMTLDevice().newCounterSampleBuffer(kTimestampsPerBuffer, true);
      ring_failed_ = !ring_;
      if (ring_failed_)
        WARN("D3D12CommandQueue: no counter buffer left for timestamps; they go to their query heaps' own");
    }
    if (ring_) {
      sampling_ = &scope.inflight;
      sampling_->samples = ring_.handle;
    }
    taken_ = false;
    skipped_.clear();
    skip_ = 0;
    clear_folds_.clear();
    for (unsigned i = 0; i < Count; i++) {
      auto pCommandList = static_cast<MTLD3D12GraphicsCommandList *>(ppCommandLists[i]);
      custom_resolves_ |= pCommandList->custom_resolves;
      if (!pCommandList->timestamp_resolves.empty()) {
        Owe(scope.inflight);
        auto from = scope.inflight.resolves.insert(
            scope.inflight.resolves.end(), pCommandList->timestamp_resolves.begin(), pCommandList->timestamp_resolves.end()
        );
        // Task F4: this queue sampled into the heaps' own counter buffers. A queue that never sampled (no ring) keeps
        // the values: another queue's ring wrote them. ponytail: a heap sampled by a queue whose ring failed, resolved
        // on one with a ring, reads the values array (zeros); per-heap buffers go with Ruling 27's follow-up.
        if (ring_failed_)
          for (; from != scope.inflight.resolves.end(); ++from)
            from->values = nullptr;
      }
      EncoderData *current = pCommandList->entry, *previous = nullptr;
      list_join_ = 0; // a list's first encoder joins, even one M3 already encoded
      while (current) {
        if (taken_ && current == taken_stop_) // past the timestamps an earlier encoder's end took (Task F4)
          taken_ = false;
        if (skip_ < skipped_.size() && skipped_[skip_].first == current) { // encoded inside an earlier render pass
          if (current->position >= list_fences_.size())
            list_fences_.resize(current->position + 1);
          list_fences_[current->position] = skipped_[skip_++].second;
          if (current->type == EncoderType::Render) // DXMT_STAT_COUNT keeps one counter id per call site
            DXMT_STAT_COUNT("#render passes merged", 1);
          else if (current->query_resolve_only)
            DXMT_STAT_COUNT("#timestamp resolves moved after a merged pass", 1);
          else
            DXMT_STAT_COUNT("#timestamp blits folded", 1);
          current = current->next;
          continue;
        }
        if (fold_timestamps && current->type == EncoderType::Clear) { // E7
          unsigned at;
          if (auto target = ClearTarget(static_cast<ClearEncoderData *>(current), i, ppCommandLists, Count, at)) {
            DXMT_STAT_COUNT("#clears folded at execute", 1);
            clear_folds_.push_back({at, target, static_cast<ClearEncoderData *>(current)});
            pending_join_ |= current->join;
            current = current->next;
            continue;
          }
        }
        if (fold_timestamps && current->type == EncoderType::Blit && current->timestamp_only) { // E4
          if (!taken_) // Task F4: else the encoder before took them at its end
            pending_samples_.insert(pending_samples_.end(), current->samples, current->samples + current->num_samples);
          pending_join_ |= current->join;
          current = current->next;
          continue;
        }
        bool strict = serial || (current->type != EncoderType::Null && pending_join_);
        if (current->type != EncoderType::Null)
          pending_join_ = false;
        unsigned pass = 0;
        if (g_stats_on) {
          static const unsigned kinds[] = {StatId("#null encoders"),   StatId("#clear passes"),
                                           StatId("#render passes"),   StatId("#blit passes"),
                                           StatId("#compute passes"),  StatId("#resolve passes"),
                                           StatId("#temporal upscale passes")};
          static const unsigned boundaries = StatId("#encoder boundaries"),
                                no_barrier = StatId("#encoder boundaries with no barrier");
          pass = StatsNextPass();
          StatCount(kinds[(int)current->type]);
          if (current->type != EncoderType::Null) {
            if (previous) {
              StatCount(boundaries);
              if (current->barriers == previous->barriers)
                StatCount(no_barrier);
            }
            previous = current;
          }
        }
        switch (current->type) {
        case EncoderType::Null:
          break;
        case EncoderType::Clear: {
          DXMT_STAT_SCOPE("queue.(encode clear)");
          auto data = static_cast<ClearEncoderData *>(current);
          {
            WMTRenderPassInfo info;
            WMT::InitializeRenderPassInfo(info);
            if (data->clear_dsv) {
              if (data->clear_dsv & 1) {
                info.depth.clear_depth = data->depth_stencil.first;
                info.depth.texture = data->attachment.texture();
                info.depth.load_action = WMTLoadActionClear;
                info.depth.store_action = WMTStoreActionStore;
                info.depth.depth_plane = data->depth_plane;
              }
              if (data->clear_dsv & 2) {
                info.stencil.clear_stencil = data->depth_stencil.second;
                info.stencil.texture = data->attachment.texture();
                info.stencil.load_action = WMTLoadActionClear;
                info.stencil.store_action = WMTStoreActionStore;
                info.stencil.depth_plane = data->depth_plane;
              }
              info.render_target_width = data->width;
              info.render_target_height = data->height;
            } else {
              info.colors[0].clear_color = data->color;
              info.colors[0].texture = data->attachment.texture();
              info.colors[0].load_action = WMTLoadActionClear;
              info.colors[0].store_action = WMTStoreActionStore;
              info.colors[0].depth_plane = data->depth_plane;
            }
            info.render_target_array_length = data->array_length;
            FlushPending(cmdbuf); // E4
            auto encoder = cmdbuf.renderCommandEncoder(info);
            LabelPass(encoder, pass, "clear");
            Encode(encoder, Order(current, strict), WMTRenderStageFragment);
            encoder.endEncoding();
          }
          break;
        }
        case EncoderType::Render: {
          DXMT_STAT_SCOPE("queue.(encode render)");
          auto data = static_cast<RenderEncoderData *>(current);
          WMTRenderPassInfo render_pass_info;
          WMT::InitializeRenderPassInfo(render_pass_info);
          {
            for (unsigned i = 0; i < std::size(render_pass_info.colors); i++) {
              auto &color_data = data->colors[i];
              if (!color_data.attachment)
                continue;
              auto &color_info = render_pass_info.colors[i];
              color_info.texture = color_data.attachment.texture();
              color_info.load_action = color_data.load_action;
              color_info.store_action = color_data.store_action;
              color_info.level = color_data.level;
              color_info.slice = color_data.slice;
              color_info.depth_plane = color_data.depth_plane;
              color_info.clear_color = color_data.clear_color;
              color_info.resolve_texture = color_data.resolve_attachment.texture();
              color_info.resolve_level = color_data.resolve_level;
              color_info.resolve_slice = color_data.resolve_slice;
              color_info.resolve_depth_plane = color_data.resolve_depth_plane;
            }
            if (data->depth.attachment) {
              auto &depth_info = render_pass_info.depth;
              auto &depth_data = data->depth;
              depth_info.texture = depth_data.attachment.texture();
              depth_info.load_action = depth_data.load_action;
              depth_info.store_action = depth_data.store_action;
              depth_info.level = depth_data.level;
              depth_info.slice = depth_data.slice;
              depth_info.depth_plane = depth_data.depth_plane;
              depth_info.clear_depth = depth_data.clear_depth;
            }
            if (data->stencil.attachment) {
              auto &stencil_info = render_pass_info.stencil;
              auto &stencil_data = data->stencil;
              stencil_info.texture = stencil_data.attachment.texture();
              stencil_info.load_action = stencil_data.load_action;
              stencil_info.store_action = stencil_data.store_action;
              stencil_info.level = stencil_data.level;
              stencil_info.slice = stencil_data.slice;
              stencil_info.depth_plane = stencil_data.depth_plane;
              stencil_info.clear_stencil = stencil_data.clear_stencil;
            }
            render_pass_info.default_raster_sample_count = data->default_raster_sample_count;
            render_pass_info.render_target_array_length = data->render_target_array_length;
            render_pass_info.render_target_width = data->render_target_width;
            render_pass_info.render_target_height = data->render_target_height;
            render_pass_info.visibility_buffer = data->visibility_buffer;
            render_pass_info.visibility_accumulate = data->visibility_buffer != 0;
            for (unsigned i = 0; i < data->num_samples; i++)
              render_pass_info.sample_buffers[i] = {data->samples[i].buffer, data->samples[i].index, ~0ull};
          }
          bool folded = false; // E7: the clears folded into this pass; it joins, as they would have
          for (auto &f : clear_folds_)
            if (f.list == i && f.target == current) {
              ApplyClear(f.clear, data, render_pass_info);
              folded = true;
            }
          if (folded)
            strict = true;
          if (!dumping && merge_) // M3: also in strict order, the default; dumps keep D3D12's passes
            PlanMerge(data, i, ppCommandLists, Count, render_pass_info);
          else
            merged_.clear();
          // ExecuteIndirect's resolvers, writing the pass's ICBs (and taking the waiting timestamps, Task F4)
          auto resolve_icbs = [&](uint16_t fence, uint64_t start = ~0ull) {
            DXMT_STAT_COUNT("#indirect resolve passes", 1);
            WMTSampleBufferAttachmentInfo at = {start == ~0ull ? 0 : sampling_->samples, start, ~0ull};
            auto pre = at.sample_buffer ? cmdbuf.computeCommandEncoderWithSampleBuffers(true, &at, 1)
                                        : cmdbuf.computeCommandEncoder(true);
            LabelPass(pre, pass, "indirect resolve");
            Encode(pre, fence, &data->pre_head, data->pre_tail);
            pre.endEncoding();
          };
          if (dumping) { // strict order: the resolvers, the pass's pixel history, then the pass
            if (data->pre_tail)
              resolve_icbs(Order(nullptr, true));
            PixelHistory(cmdbuf, data, render_pass_info);
          }
          uint64_t resolver_start = ~0ull;
          if (sampling_) { // Task F4: the call's counter buffer, one attachment
            uint64_t start = TakeStart();
            ends_.assign(data->samples, data->samples + data->num_samples);
            for (auto *m : merged_)
              ends_.insert(ends_.end(), m->samples, m->samples + m->num_samples);
            Trailing(merged_.empty() ? current : merged_.back(), merged_.empty() ? i : merged_list_, ppCommandLists,
                     Count, fold_timestamps);
            uint64_t end = Point(ends_.data(), ends_.size());
            if (data->pre_tail && !dumping) // the resolvers come first
              std::swap(start, resolver_start);
            for (auto &b : render_pass_info.sample_buffers)
              b = {};
            if (start != ~0ull || end != ~0ull)
              render_pass_info.sample_buffers[0] = {sampling_->samples, end, start};
          } else {
            if (data->pre_tail && !dumping)
              FlushPending(cmdbuf); // E4
            if (auto [buffer, index] = TakePending(cmdbuf, render_pass_info.sample_buffers[0].sample_buffer); buffer) {
              if (render_pass_info.sample_buffers[0].sample_buffer) // E4
                render_pass_info.sample_buffers[0].start_of_vertex_sample_index = index;
              else
                render_pass_info.sample_buffers[0] = {buffer, ~0ull, index};
            }
          }
          uint16_t fence = Order(current, strict), early = early_;
          if (data->pre_tail && !dumping) { // the resolvers wait as the pass would; the pass waits on them alone
            resolve_icbs(fence, resolver_start);
            waits_.assign(1, fence);
          }
          // Apple GPUs write only the last of an encoder's counter buffers (GPU efficiency spec E2): the pass keeps the
          // first; each other one's sample gets its own blit right after the pass.
          WMTRenderPassSampleBufferInfo trailing[std::size(render_pass_info.sample_buffers)] = {};
          for (unsigned i = 1; i < std::size(render_pass_info.sample_buffers); i++)
            std::swap(trailing[i], render_pass_info.sample_buffers[i]);
          auto encoder = cmdbuf.renderCommandEncoder(render_pass_info);
          LabelPass(encoder, pass, "render", data);
          // Geometry shader draws (MacNeutron) read resources in the object and mesh stages too.
          // Merged passes' commands follow this pass's, in one chain, unlinked again after.
          wmtcmd_base *tail = data->cmd_tail;
          for (auto *m : merged_)
            if (m->type == EncoderType::Render) {
              tail->next.set(&static_cast<RenderEncoderData *>(m)->cmd_head);
              tail = static_cast<RenderEncoderData *>(m)->cmd_tail;
            }
          Encode(encoder, fence, data->use_geometry ? WMTRenderStagePreRaster : WMTRenderStageVertex, &data->cmd_head,
                 tail, early);
          tail = data->cmd_tail;
          size_t first_skipped = skipped_.size();
          for (auto *m : merged_) {
            if (m->type == EncoderType::Render) {
              tail->next.set(nullptr);
              tail = static_cast<RenderEncoderData *>(m)->cmd_tail;
            }
            skipped_.push_back({m, fence});
          }
          encoder.endEncoding();
          for (auto &t : trailing)
            if (t.sample_buffer)
              SampleAfter(cmdbuf, t.sample_buffer, t.end_of_fragment_sample_index, kLoneCounterBuffer);
          // B7: the timestamp resolves merged past, in their order, each after all before it (the merged pass too).
          for (auto *d : deferred_) {
            auto blit = static_cast<BlitEncoderData *>(d);
            auto deferred = cmdbuf.blitCommandEncoder();
            LabelPass(deferred, pass, "blit (resolves after the merged pass)");
            uint16_t after = Order(nullptr, true);
            Encode(deferred, after, &blit->cmd_head, blit->cmd_tail);
            deferred.endEncoding();
            for (size_t k = first_skipped; k < skipped_.size(); k++)
              if (skipped_[k].first == d)
                skipped_[k].second = after;
          }
          break;
        }
        case EncoderType::Blit: {
          DXMT_STAT_SCOPE("queue.(encode blit)");
          auto data = static_cast<BlitEncoderData *>(current);
          WMTSampleBufferAttachmentInfo samples[4];
          unsigned num_samples = data->num_samples;
          for (unsigned i = 0; i < num_samples; i++)
            samples[i] = {data->samples[i].buffer, ~0ull, data->samples[i].index};
          if (sampling_) // Task F4
            num_samples = Attach(current, i, ppCommandLists, Count, fold_timestamps, samples[0]);
          if (data->cmd_tail == (wmtcmd_base *)&data->cmd_head) {
            EncodeEmpty(cmdbuf, current, pass, strict, samples, num_samples);
            break;
          }
          if (auto [buffer, index] = TakePending(cmdbuf, num_samples ? samples[0].sample_buffer : 0); buffer) { // E4
            if (!num_samples)
              samples[num_samples++] = {buffer, index, ~0ull};
            else
              samples[0].start_of_encoder_sample_index = index;
          }
          auto encoder = num_samples ? cmdbuf.blitCommandEncoderWithSampleBuffers(samples, num_samples)
                                           : cmdbuf.blitCommandEncoder();
          LabelPass(encoder, pass, "blit");
          Encode(encoder, Order(current, strict), &data->cmd_head, data->cmd_tail);
          encoder.endEncoding();
          break;
        }
        case EncoderType::Compute: {
          DXMT_STAT_SCOPE("queue.(encode compute)");
          auto data = static_cast<ComputeEncoderData *>(current);
          WMTSampleBufferAttachmentInfo samples[4];
          unsigned num_samples = data->num_samples;
          for (unsigned i = 0; i < num_samples; i++)
            samples[i] = {data->samples[i].buffer, ~0ull, data->samples[i].index};
          if (sampling_) // Task F4
            num_samples = Attach(current, i, ppCommandLists, Count, fold_timestamps, samples[0]);
          if (data->cmd_tail == (wmtcmd_base *)&data->cmd_head) {
            EncodeEmpty(cmdbuf, current, pass, strict, samples, num_samples);
            break;
          }
          if (auto [buffer, index] = TakePending(cmdbuf, num_samples ? samples[0].sample_buffer : 0); buffer) { // E4
            if (!num_samples)
              samples[num_samples++] = {buffer, index, ~0ull};
            else
              samples[0].start_of_encoder_sample_index = index;
          }
          auto encoder = num_samples ? cmdbuf.computeCommandEncoderWithSampleBuffers(false, samples, num_samples)
                                           : cmdbuf.computeCommandEncoder(false);
          LabelPass(encoder, pass, "compute");
          Encode(encoder, Order(current, strict), &data->cmd_head, data->cmd_tail);
          encoder.endEncoding();
          break;
        }
        case EncoderType::Resolve: {
          DXMT_STAT_SCOPE("queue.(encode resolve)");
          auto data = static_cast<ResolveEncoderData *>(current);

          WMTRenderPassInfo info;
          WMT::InitializeRenderPassInfo(info);
          info.colors[0].texture = data->src.texture();
          info.colors[0].load_action = WMTLoadActionLoad;
          info.colors[0].store_action = WMTStoreActionStoreAndMultisampleResolve;
          info.colors[0].resolve_texture = data->dst.texture();
          FlushPending(cmdbuf); // E4

          auto encoder = cmdbuf.renderCommandEncoder(info);
          encoder.setLabel(WMT::String::string("ResolvePass", WMTUTF8StringEncoding));
          Encode(encoder, Order(current, strict), WMTRenderStageFragment);
          encoder.endEncoding();

          break;
        }
        case EncoderType::TemporalUpscale: {
          // MacNeutron (XeSS answered by MetalFX): MetalFX encodes its own passes, which wait on and update the
          // upscaler's fence; a blit before hands earlier work over to it, a blit after hands it on (as DXMT's D3D11
          // path, dxmt_context.cpp). D3D12 textures are untracked: without these MetalFX reads its inputs early.
          DXMT_STAT_SCOPE("queue.(encode temporal upscale)");
          auto data = static_cast<TemporalUpscaleEncoderData *>(current);
          auto scaler_fence = data->scaler->fence();
          // Task F4: the waiting timestamps at the begin blit's start, those after the upscale (that no encoder start
          // takes) at the end blit's end. MetalFX's own passes take none.
          WMTSampleBufferAttachmentInfo first = {0, ~0ull, ~0ull}, last = first;
          if (sampling_) {
            first = {sampling_->samples, TakeStart(), ~0ull};
            ends_.clear();
            Trailing(current, i, ppCommandLists, Count, fold_timestamps);
            last = {sampling_->samples, ~0ull, Point(ends_.data(), ends_.size())};
          } else {
            FlushPending(cmdbuf); // E4
          }
          uint16_t fence = Order(current, strict);
          auto fill = ScratchFill(); // both blits have only fences to do
          auto begin = first.start_of_encoder_sample_index != ~0ull
                           ? cmdbuf.blitCommandEncoderWithSampleBuffers(&first, 1)
                           : cmdbuf.blitCommandEncoder();
          LabelPass(begin, pass, "upscale");
          for (auto wait : waits_)
            begin.waitForFence(fences_[wait]);
          begin.encodeCommands((const wmtcmd_blit_nop *)&fill);
          begin.updateFence(scaler_fence);
          begin.endEncoding();
          cmdbuf.encodeTemporalScale(data->scaler->scaler(), data->color.texture(), data->output.texture(),
                                     data->depth.texture(), data->motion.texture(), data->exposure.texture(),
                                     scaler_fence, data->props, data->reactive.texture());
          if (FrameGenOn()) {
            std::lock_guard<std::mutex> lock(g_framegen_slot.mutex);
            g_framegen_slot.scaler = data->scaler;
            g_framegen_slot.depth = data->depth;
            g_framegen_slot.motion = data->motion;
            g_framegen_slot.props = data->props;
            g_framegen_slot.fresh = true;
          }
          auto end = last.end_of_encoder_sample_index != ~0ull ? cmdbuf.blitCommandEncoderWithSampleBuffers(&last, 1)
                                                              : cmdbuf.blitCommandEncoder();
          end.waitForFence(scaler_fence);
          end.encodeCommands((const wmtcmd_blit_nop *)&fill);
          end.updateFence(fences_[fence]);
          end.endEncoding();
          break;
        }
        }
        if (dumping)
          DumpPass(cmdbuf, current);
        current = current->next;
      }
    }
    FlushPending(cmdbuf, kLoneEnd); // E4: lone timestamps after the call's last encoder
    pending_join_ = false;
    sampling_ = nullptr;
    taken_ = false;
  };

  void STDMETHODCALLTYPE SetMarker(UINT metadata, const void *data, UINT size) {
    DXMT_STAT_SCOPE("queue.SetMarker");};

  void STDMETHODCALLTYPE BeginEvent(UINT metadata, const void *data, UINT size) {
    DXMT_STAT_SCOPE("queue.BeginEvent");};

  void STDMETHODCALLTYPE EndEvent() {
    DXMT_STAT_SCOPE("queue.EndEvent");};

  HRESULT STDMETHODCALLTYPE
  Signal(ID3D12Fence *pFence, UINT64 Value) {
    DXMT_STAT_SCOPE("queue.Signal");
    auto scope = StartCommitting(true);
    auto &cmdbuf = scope.inflight.cmdbuf;
    auto &fence = static_cast<MTLD3D12Fence *>(pFence)->fence;
    auto gen = fence->ask(Value);
    // Other queues wait on the MTLEvent (GPU overlap spec §3.11): signaled at once, unless timestamps the GPU could
    // read (a custom heap's) are still to be written.
    bool owes = cpu_work_.load(std::memory_order_acquire) != 0, hold = owes && custom_resolves_;
    if (!hold)
      cmdbuf.encodeSignalEvent(gen.gpu, Value);
    if (owes) { // behind timestamps still to be written, or fences waited on: signal once they are
      DXMT_STAT_COUNT("#fence signals deferred to the CPU", 1);
      if (hold)
        DXMT_STAT_COUNT("#fence signals forwarded to the GPU event", 1);
      Owe(scope.inflight);
      scope.inflight.signals.push_back({fence, std::move(gen), Value, hold});
      return S_OK;
    }
    cmdbuf.encodeSignalEvent(gen.shared, Value);
    return S_OK;
  };

  HRESULT STDMETHODCALLTYPE
  Wait(ID3D12Fence *pFence, UINT64 Value) {
    DXMT_STAT_SCOPE("queue.Wait");
    auto gen = static_cast<MTLD3D12Fence *>(pFence)->fence->current();
    if (gen.shared.signaledValue() >= Value) {
      DXMT_STAT_COUNT("#queue waits already met", 1);
      return S_OK;
    }
    // On the MTLEvent (GPU overlap spec §3.11); the shared event's value is then owed before this queue's later
    // signals reach the CPU.
    DXMT_STAT_COUNT("#queue waits on the GPU event", 1);
    auto scope = StartCommitting(false);
    scope.inflight.cmdbuf.encodeWaitForEvent(gen.gpu, Value);
    Owe(scope.inflight);
    scope.inflight.waits.push_back({std::move(gen.shared), Value});
    return S_OK;
  };

  HRESULT STDMETHODCALLTYPE
  GetTimestampFrequency(UINT64 *pFrequency) {
    DXMT_STAT_SCOPE("queue.GetTimestampFrequency");
    if (!pFrequency)
      return E_INVALIDARG;
    *pFrequency = device_->TimestampFrequency();
    return S_OK;
  };

  HRESULT STDMETHODCALLTYPE
  GetClockCalibration(UINT64 *gpu_timestamp, UINT64 *cpu_timestamp) {
    DXMT_STAT_SCOPE("queue.GetClockCalibration");
    // Metal's GPU clock with QueryPerformanceCounter read on either side (Unreal treats a failure here as fatal).
    LARGE_INTEGER before, after;
    uint64_t cpu, gpu;
    QueryPerformanceCounter(&before);
    device_->GetMTLDevice().sampleTimestamps(cpu, gpu);
    QueryPerformanceCounter(&after);
    if (gpu_timestamp)
      *gpu_timestamp = gpu;
    if (cpu_timestamp)
      *cpu_timestamp = before.QuadPart + (after.QuadPart - before.QuadPart) / 2;
    return S_OK;
  };

  D3D12_COMMAND_QUEUE_DESC *STDMETHODCALLTYPE
  GetDesc(D3D12_COMMAND_QUEUE_DESC *__ret) {
    DXMT_STAT_SCOPE("queue.GetDesc");
    *__ret = desc_;
    return __ret;
  };

  HRESULT STDMETHODCALLTYPE
  CreateSwapChain(
      IDXGIFactory1 *pFactory, HWND hWnd, const DXGI_SWAP_CHAIN_DESC1 *pDesc,
      const DXGI_SWAP_CHAIN_FULLSCREEN_DESC *pFullscreenDesc, IDXGISwapChain1 **ppSwapChain
  ) {
    DXMT_STAT_SCOPE("queue.CreateSwapChain");
    return dxmt::CreateSwapChain(pFactory, device_, this, hWnd, pDesc, pFullscreenDesc, ppSwapChain);
  }

  HRESULT
  Present(Presenter *presenter, ID3D12Resource *backbuffer, HANDLE hLantecyWaitable, double after) {
    HRESULT hr = PresentFrame(presenter, backbuffer, hLantecyWaitable, after);
    // Once the present is committed: start or stop a frame capture at this boundary.
    auto &dumps = Dumps();
    if (dumps.Dumping()) { // the end of a dumped frame, in passes.txt
      std::lock_guard<std::mutex> lock(dumps.mutex);
      dumps.log += "# frame " + std::to_string(dumps.frames - dumps.frame) + " presented\n";
    }
    uint64_t frames = ++dumps.frames;
    StatsFrame();
    if (dumps.frame != ~0ull && frames == dumps.frame + dumps.count - 1 + 8)
      SaveDumps();
    else if (dumps.on_key && dumps.frame == ~0ull && (GetAsyncKeyState(VK_F9) & 0x8000))
      dumps.frame = frames; // the frame now starting
    switch (capture_state_.getNextAction(++presented_frames_)) {
    case CaptureState::NextAction::StartCapture: {
      WMTCaptureInfo info;
      info.capture_object = device_->GetMTLDevice();
      info.destination = WMTCaptureDestinationGPUTraceDocument;
      char stamp[64];
      std::time_t now = std::time(nullptr);
      std::strftime(stamp, sizeof(stamp), "_%H'%M'%S_%m-%d-%y.gputrace", std::localtime(&now));
      auto file = env::getUnixPath(env::getExeBaseName() + "_F." + std::to_string(presented_frames_ + 1) + stamp);
      WARN("A new capture will be saved to ", file);
      info.output_url.set(file.c_str());
      WMT::CaptureManager::sharedCaptureManager().startCapture(info);
      break;
    }
    case CaptureState::NextAction::StopCapture:
      WMT::CaptureManager::sharedCaptureManager().stopCapture();
      break;
    case CaptureState::NextAction::Nothing:
      if (capture_state_.shouldCaptureNextFrame())
        capture_state_.scheduleNextFrameCapture(presented_frames_ + 1);
      break;
    }
    return hr;
  }

  // MacNeutron (frame generation on MetalFX, MACNEUTRON_FRAMEGEN=1): MetalFX's frame interpolator, paired with the
  // upscaler of the last temporal upscale (g_framegen_slot), and a ring of three frames: each present's frame copied
  // out of the back buffer (`real`, the next interpolation's previous frame too) and the frame generated before it.
  // The game's command buffer writes them; a pacing thread presents them on a queue of its own, behind an event the
  // game's command buffer signals, so the game never waits on a drawable: two drawables a frame, taken on the game's
  // thread, held it for most of each frame (Palworld: Present 13.7 ms a frame, stalls to 480 ms). The thread signals
  // another event once it has read a slot, which the game's command buffer waits on before writing that slot again.
  static constexpr unsigned kFrameGenRing = 3;
  struct FramePair {
    uint64_t index;
    bool generated;
    Rc<Presenter> presenter;
    DXMTPresentMetadata metadata;
  };
  struct FrameGen {
    WMT::Reference<WMT::FXFrameInterpolator> interpolator;
    WMT::Reference<WMT::Fence> fence;
    WMT::Reference<WMT::Texture> real[kFrameGenRing], generated[kFrameGenRing];
    WMTFXFrameInterpolatorInfo info{};
    WMTPixelFormat format = WMTPixelFormatInvalid;
    uint64_t width = 0, height = 0;
    bool failed = false;
    double interval = 0.0; // seconds between presents, smoothed
    std::chrono::steady_clock::time_point last{};
    uint64_t enqueued = 0;  // frames handed to the pacing thread (the slot of frame n is n % kFrameGenRing)
    bool has_prev = false;  // the slot before `enqueued`'s holds the previous present's frame
    WMT::Reference<WMT::CommandQueue> queue;
    WMT::Reference<WMT::Event> written, read; // game to pacer: frame n's slot written; pacer to game: frame n's read
    dxmt::mutex mutex;
    dxmt::condition_variable cv;
    std::vector<FramePair> pending;
    uint64_t committed = 0; // frames the pacing thread has committed
    bool stop = false;
    dxmt::thread thread;
  } framegen_;

  void
  PacingThread() {
    auto &fg = framegen_;
    for (;;) {
      FramePair pair;
      {
        std::unique_lock<dxmt::mutex> lock(fg.mutex);
        fg.cv.wait(lock, [&] { return fg.stop || !fg.pending.empty(); });
        if (fg.pending.empty())
          return;
        pair = std::move(fg.pending.front());
        fg.pending.erase(fg.pending.begin());
      }
      auto pool = WMT::MakeAutoreleasePool();
      unsigned slot = pair.index % kFrameGenRing;
      auto cmdbuf = fg.queue.commandBuffer();
      cmdbuf.encodeWaitForEvent(fg.written, pair.index + 1);
      auto none = [](auto) {};
      if (pair.generated) {
        DXMT_STAT_SCOPE("pacer.(generated frame: drawable and blit)");
        auto drawable = pair.presenter->encodeCommands(cmdbuf, fg.generated[slot], pair.metadata, none, none);
        cmdbuf.presentDrawable(drawable);
      }
      WMT::MetalDrawable drawable;
      {
        DXMT_STAT_SCOPE("pacer.(real frame: drawable and blit)");
        drawable = pair.presenter->encodeCommands(cmdbuf, fg.real[slot], pair.metadata, none, none);
      }
      // As soon as it can, as the generated frame: the drawables themselves pace the pair, one display refresh each
      // (macOS's compositor returns a drawable once it has shown the next). A minimum duration on top held each pair
      // for more refreshes (the test: 34 ms a pair, against 18 without).
      cmdbuf.presentDrawable(drawable);
      cmdbuf.encodeSignalEvent(fg.read, pair.index + 1);
      cmdbuf.commit();
      {
        std::lock_guard<dxmt::mutex> lock(fg.mutex);
        fg.committed = pair.index + 1;
      }
      fg.cv.notify_all();
    }
  }

  void
  StopPacing() {
    auto &fg = framegen_;
    if (!fg.thread.joinable())
      return;
    {
      std::lock_guard<dxmt::mutex> lock(fg.mutex);
      fg.stop = true;
    }
    fg.cv.notify_all();
    fg.thread.join();
  }

  // Waits until the pacing thread has committed every frame handed to it (a present it can't take must come after).
  void
  DrainPacing() {
    auto &fg = framegen_;
    std::unique_lock<dxmt::mutex> lock(fg.mutex);
    fg.cv.wait(lock, [&] { return fg.committed == fg.enqueued; });
  }

  // Frame generation's present: true when the pacing thread took it (frame copied, maybe a frame generated before it),
  // false when the frame is presented as rendered (then after every frame the thread had). Generates only right after a
  // temporal upscale of this frame's size, with the previous present's frame in the ring, no reset, and a present
  // interval under 50 ms.
  bool
  PresentFrameGen(WMT::CommandBuffer &cmdbuf, Presenter *presenter, WMT::Texture frame,
                  const DXMTPresentMetadata &metadata) {
    auto &fg = framegen_;
    auto now = std::chrono::steady_clock::now();
    double since = fg.last.time_since_epoch().count() ? std::chrono::duration<double>(now - fg.last).count() : 1.0;
    fg.last = now;
    if (since < 0.05)
      fg.interval = fg.interval > 0.0 ? fg.interval * 0.9 + since * 0.1 : since;
    Rc<TemporalScaler> scaler;
    TextureViewRef depth, motion;
    WMTFXTemporalScalerProps props{};
    bool fresh = false;
    {
      std::lock_guard<std::mutex> lock(g_framegen_slot.mutex);
      fresh = g_framegen_slot.fresh;
      g_framegen_slot.fresh = false;
      scaler = g_framegen_slot.scaler;
      depth = g_framegen_slot.depth;
      motion = g_framegen_slot.motion;
      props = g_framegen_slot.props;
    }
    if (fg.failed)
      return false;
    auto device = device_->GetMTLDevice();
    // The ring, for this frame's size and format; another size or format waits for the frames in flight.
    if (frame.width() != fg.width || frame.height() != fg.height || frame.pixelFormat() != fg.format) {
      if (!fresh || !scaler || scaler->info().output_width != frame.width() ||
          scaler->info().output_height != frame.height())
        return false; // nothing to pair with at this size: as rendered
      DrainPacing();
      WMTTextureInfo t{};
      t.pixel_format = frame.pixelFormat();
      t.width = frame.width();
      t.height = frame.height();
      t.depth = 1;
      t.array_length = 1;
      t.type = WMTTextureType2D;
      t.mipmap_level_count = 1;
      t.sample_count = 1;
      t.usage = WMTTextureUsage(WMTTextureUsageShaderRead | WMTTextureUsageShaderWrite | WMTTextureUsageRenderTarget);
      t.options = WMTResourceOptions(WMTResourceStorageModePrivate | WMTResourceHazardTrackingModeUntracked);
      for (unsigned i = 0; i < kFrameGenRing; i++) {
        fg.real[i] = device.newTexture(t);
        fg.generated[i] = device.newTexture(t);
      }
      fg.width = frame.width();
      fg.height = frame.height();
      fg.format = frame.pixelFormat();
      fg.has_prev = false;
      if (!fg.queue) {
        fg.queue = device.newCommandQueue(kFrameGenRing + 1);
        fg.written = device.newEvent();
        fg.read = device.newEvent();
        fg.fence = device.newFence();
        fg.thread = dxmt::thread([this]() { PacingThread(); });
      }
    }
    bool generate = fresh && fg.has_prev && !props.reset && since < 0.05 && fg.interval > 0.0 && depth && motion &&
                    scaler->info().output_width == fg.width && scaler->info().output_height == fg.height;
    if (generate) {
      auto &si = scaler->info();
      WMTFXFrameInterpolatorInfo info{};
      info.color_format = fg.format;
      info.output_format = fg.format;
      info.depth_format = depth.texture().pixelFormat();
      info.motion_format = motion.texture().pixelFormat();
      info.input_width = si.input_width;
      info.input_height = si.input_height;
      info.output_width = si.output_width;
      info.output_height = si.output_height;
      info.scaler = scaler->scaler().handle;
      if (!fg.interpolator || memcmp(&info, &fg.info, sizeof(info))) {
        DXMT_STAT_SCOPE("queue.(frame generation: make the interpolator)");
        fg.interpolator = device.newFrameInterpolator(info);
        fg.info = info;
        if (!fg.interpolator) {
          fprintf(stderr, "macneutron-framegen: MetalFX made no frame interpolator for %ux%u to %ux%u (colour %u, "
                          "depth %u, motion %u): frames are presented as rendered\n",
                  info.input_width, info.input_height, info.output_width, info.output_height,
                  (unsigned)info.color_format, (unsigned)info.depth_format, (unsigned)info.motion_format);
          fg.failed = true;
          DrainPacing();
          return false;
        }
        fprintf(stderr, "macneutron-framegen: generating a frame between each two, %ux%u (paired with the %ux%u "
                        "upscaler)\n", info.output_width, info.output_height, info.input_width, info.input_height);
      }
    }
    // The game's thread runs ahead of the pacing thread by the ring's depth at most.
    uint64_t n = fg.enqueued;
    {
      DXMT_STAT_SCOPE("queue.(frame generation: wait for the pacing thread)");
      std::unique_lock<dxmt::mutex> lock(fg.mutex);
      fg.cv.wait(lock, [&] { return fg.committed + kFrameGenRing - 1 > n; });
    }
    unsigned slot = n % kFrameGenRing, prev = (n + kFrameGenRing - 1) % kFrameGenRing;
    uint16_t done = Order(nullptr, true);
    auto fill = ScratchFill();
    auto begin = cmdbuf.blitCommandEncoder();
    for (auto wait : waits_)
      begin.waitForFence(fences_[wait]);
    begin.encodeCommands((const wmtcmd_blit_nop *)&fill);
    begin.updateFence(fg.fence);
    begin.endEncoding();
    if (n >= kFrameGenRing) // the pacing thread's last read of this slot
      cmdbuf.encodeWaitForEvent(fg.read, n - kFrameGenRing + 1);
    if (generate) {
      DXMT_STAT_SCOPE("queue.(encode frame interpolation)");
      DXMT_STAT_COUNT("#frames generated", 1);
      WMTFXFrameInterpolatorProps p{};
      p.motion_vector_scale_x = props.motion_vector_scale_x;
      p.motion_vector_scale_y = props.motion_vector_scale_y;
      p.delta_time = (float)fg.interval;
      p.near_plane = 0.1f;
      p.far_plane = 10000.0f;
      p.field_of_view = 70.0f;
      p.aspect_ratio = (float)fg.width / (float)fg.height;
      p.jitter_offset_x = props.jitter_offset_x;
      p.jitter_offset_y = props.jitter_offset_y;
      p.depth_reversed = props.depth_reversed;
      cmdbuf.encodeFrameInterpolate(fg.interpolator, frame, fg.real[prev], depth.texture(), motion.texture(),
                                    fg.generated[slot], fg.fence, p);
    }
    wmtcmd_blit_copy_texture copy = {};
    copy.type = WMTBlitCommandCopyTexture;
    copy.next.set(nullptr);
    copy.src = frame.handle;
    copy.dst = fg.real[slot].handle;
    auto end = cmdbuf.blitCommandEncoder();
    end.waitForFence(fg.fence);
    end.encodeCommands((const wmtcmd_blit_nop *)&copy);
    end.updateFence(fences_[done]);
    end.endEncoding();
    cmdbuf.encodeSignalEvent(fg.written, n + 1);
    {
      std::lock_guard<dxmt::mutex> lock(fg.mutex);
      fg.pending.push_back({n, generate, Rc<Presenter>(presenter), metadata});
      fg.enqueued = n + 1;
    }
    fg.has_prev = true;
    // Handed over only once this command buffer is committed (the scope's end), so the thread's wait on `written`
    // never comes first: notified then (PresentFrame).
    return true;
  }

  HRESULT
  PresentFrame(Presenter *presenter, ID3D12Resource *backbuffer, HANDLE hLantecyWaitable, double after) {
    bool handed = false;
    {
      auto scope = StartCommitting(true);
      auto &cmdbuf = scope.inflight.cmdbuf;

      auto g = reinterpret_cast<MTLD3D12Resource *>(backbuffer);
      auto &view = g->texture->view(g->texture->fullView);

      auto state = presenter->synchronizeLayerProperties();
      // MacNeutron (frame generation): the pacing thread presents this frame, and a generated one before it. A game
      // that waits for vsync (`after`) already presents at the display's rate: nothing is generated for it.
      if (FrameGenOn() && after <= 0.0)
        handed = PresentFrameGen(cmdbuf, presenter, view.texture, state.metadata);
      if (!handed) {
        if (FrameGenOn() && framegen_.thread.joinable())
          DrainPacing();
        uint16_t fence = 0;
        auto drawable = presenter->encodeCommands(
            cmdbuf, view.texture, state.metadata,
            [&](auto encoder) { fence = Join(encoder, WMTRenderStageFragment); },
            [&](auto encoder) { encoder.updateFence(fences_[fence], WMTRenderStageFragment); }
        );

        if (after > 0)
          cmdbuf.presentDrawableAfterMinimumDuration(drawable, after);
        else
          cmdbuf.presentDrawable(drawable);
      }
      scope.inflight.semaphore = hLantecyWaitable;
    }
    if (handed)
      framegen_.cv.notify_all();

    return S_OK;
  }
};

HRESULT
CreateCommandQueue(MTLD3D12Device *pDevice, const D3D12_COMMAND_QUEUE_DESC *pDesc, REFIID riid, void **ppCommandQueue) {
  auto command_queue = Com(new MTLD3D12CommandQueueImpl(pDevice));
  HRESULT hr = command_queue->Initialize(pDesc);
  if (FAILED(hr))
    return hr;
  return command_queue->QueryInterface(riid, ppCommandQueue);
};

} // namespace dxmt