/*
 * Copyright 2026 MacNeutron contributors
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
#include "dxil.hpp"
#include "../dxbc_converter.hpp"
#include "../nt/dxbc_converter_base.hpp"

namespace dxmt::dxil {

struct HandleInfo {
  ResourceClass cls;
  uint32_t range;       // the resource's range ID (airconv's RangeId)
  llvm::Value *index;   // the absolute register, as airconv's binding maps take it
  const Resource *resource;
};

// Resolves a %dx.types.Handle to its resource, through annotateHandle. Only reads the handle's defining call, so it
// also serves SM50Initialize's usage scan.
llvm::Expected<HandleInfo> ResolveHandle(const EntryInfo &entry, llvm::Value *handle);
// An annotateHandle (216) of a createHandleFromHeap (218): returns that call and sets `props` to the
// ResourceProperties, which pick its heap range; nullptr for any other handle (MacNeutron).
llvm::CallInst *HeapHandle(llvm::CallInst &annotate, uint32_t props[2]);

// Replaces dx.op calls with AIR, one call at a time, inside the AIR entry function being built.
// GPU efficiency spec E5 (MacNeutron): a raw or structured buffer load is bounds-checked once for all its components
// (Metal Shader Converter's form: one that straddles its view's end reads zeros); DXMT_DXIL_BOUNDS=component checks
// each component, as before (triage).
bool DxilBoundsPerComponent();
// DXMT_DXIL_BOUNDS=off (Task F2, measurement only): no bounds checks on typed Buffer<> reads (the view's offset is
// still added) nor on raw and structured accesses: a read past a view reads whatever lies there (a GPU fault where
// nothing is mapped), a write past it lands there.
bool DxilBoundsOff();

class Lowering {
public:
  // `vertex_outputs`: for a geometry shader, the vertex shader whose outputs its inputs read (from the payload).
  Lowering(const DXILShader &dxil, dxbc::context &ctx, const EntryInfo *vertex_outputs = nullptr);
  // Lowers and erases `call`, or fails naming the op. Handle-producing calls are kept for Cleanup.
  llvm::Error Lower(llvm::CallInst *call);
  // Erases the handle-producing calls once nothing uses them.
  void Cleanup(llvm::Function &function);
  // A 64-bit groupshared atomicrmw or cmpxchg (Task 14c) under the threadgroup lock array `locks` ([N x i32]
  // addrspace(3)); replaces and erases it.
  void GroupShared64(llvm::Instruction *inst, llvm::GlobalVariable *locks);

private:
  llvm::Error LowerResource(uint32_t opcode, llvm::CallInst *call); // dxil_lower_resources.cpp
  llvm::Error LowerMath(uint32_t opcode, llvm::CallInst *call);     // dxil_lower_math.cpp
  llvm::Error LowerOther(uint32_t opcode, llvm::CallInst *call);    // dxil_lower.cpp: IO, IDs, control
  llvm::Error Unsupported(llvm::CallInst *call);

  void Replace(llvm::CallInst *call, llvm::Value *value);
  // An aggregate of type `ty` built from `fields` with insertvalue (DXIL's ResRet/CBufRet/Dimensions structs).
  llvm::Value *Aggregate(llvm::Type *ty, llvm::ArrayRef<llvm::Value *> fields);
  // Reinterprets or converts a scalar to `ty` (same width: bitcast; float<->half: fp cast; ints: trunc/zext).
  llvm::Value *Cast(llvm::Value *value, llvm::Type *ty);

  // Resources (dxil_lower_resources.cpp).
  llvm::Optional<dxbc::BufferResourceHandle> Buffer(const HandleInfo &h);
  llvm::Optional<dxbc::TextureDescirptor> Texture(const HandleInfo &h);
  // Bound-checked pointer to `ty` at `byte_offset` in a buffer; null when out of bounds (reads 0, drops writes).
  llvm::Value *ElementPointer(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Type *ty);
  llvm::Value *LoadElement(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Type *ty);
  llvm::Value *InBounds(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, uint64_t bytes);
  uint32_t ExtractedComponents(llvm::CallInst *call);
  // A pointer to `count` consecutive `ty` at `byte_offset`, null unless all are in bounds (E5).
  llvm::Value *AccessPointer(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Type *ty, unsigned count);
  void StoreElement(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Value *value);
  llvm::Error LowerBufferLoad(llvm::CallInst *call, const HandleInfo &h, llvm::Value *byte_offset, uint32_t mask);
  llvm::Error LowerBufferStore(llvm::CallInst *call, const HandleInfo &h, llvm::Value *byte_offset, unsigned first_value, uint32_t mask);
  llvm::Error LowerTypedBufferLoad(llvm::CallInst *call, const HandleInfo &h, llvm::Value *index);
  llvm::Error LowerTypedBufferStore(llvm::CallInst *call, const HandleInfo &h, llvm::Value *index);
  llvm::Error LowerTexture(uint32_t opcode, llvm::CallInst *call, const HandleInfo &h);
  llvm::Error LowerDimensions(llvm::CallInst *call, const HandleInfo &h);
  // Ops 78 and 79 with the i64 overload (Task 14a): raw and structured buffers through UnderLock.
  llvm::Error Atomic64(uint32_t opcode, llvm::CallInst *call, const HandleInfo &h);

  // 64-bit atomics (dxil_lower.cpp; DXIL SM 6.8 plan, Tasks 14a-14c, spec 3.8): runs `critical`, which reads the
  // location, computes, writes and returns the old value, while holding the u32 lock at `lock`, inside fences of
  // `flags` at `scope`. Returns the old value; the insert point ends after it.
  llvm::Value *UnderLock(llvm::Value *lock, llvm::air::MemFlags flags, llvm::air::ThreadScope scope,
                         llvm::function_ref<llvm::Value *()> critical);

  // IO (dxil_lower.cpp).
  llvm::Value *UnpackedInput(uint32_t element_id);
  llvm::Value *UnpackedInputOfKind(SemanticKind kind);
  llvm::Value *RegisterElementPointer(dxbc::register_file &file, const SignatureElement &e, llvm::Value *row, uint32_t col);

  // Wave operations (dxil_lower.cpp). A pixel shader whose entry lacks [WaveOpsIncludeHelperLanes] leaves its helper
  // lanes out of them (D3D's Wave_Ops_Include_Helper_Lanes.md), and Metal's simdgroup functions count them.
  bool HelpersLeftOut() const;
  // The lanes taking part in a wave operation at the insert point: the i64 ballot of the non-helper lanes when helpers
  // are left out, nullptr otherwise (nothing is masked). Emitted at each operation: divergence changes it.
  llvm::Value *WaveLanes();
  // i1: this lane takes part. When helpers are left out: a non-helper lane, or any lane of a simdgroup with no active
  // non-helper lane; true otherwise.
  llvm::Value *Participates();

  // Compute shaders, which have no air.dfdx (spec 3.3): the derivative of float `v` (scalar or vector) across its quad,
  // read with air.quad_shuffle. Quad lane l is Metal's (simd lane & 3), never D3D's flattened ID: coarse lanes 1 - 0
  // (x) and 2 - 0 (y); fine (l | 1) - (l & 2) and (l | 2) - (l & 1).
  llvm::Value *QuadDerivative(llvm::Value *v, bool y, bool fine);
  // (ddx, ddy) of `coord`, coarse: the gradients implicit-LOD sampling in a compute shader uses.
  std::pair<llvm::Value *, llvm::Value *> CoarseGradients(llvm::Value *coord);

  // Geometry shaders: the vertex shader output register and column holding input `e`'s row `row`, column `col`.
  llvm::Expected<llvm::Value *> VertexOutputPointer(const SignatureElement &e, llvm::Value *row, uint32_t col, llvm::Value *vertex);

  const DXILShader &dxil;
  const EntryInfo &entry;
  const EntryInfo *vertex_outputs;
  dxbc::context &ctx;
  llvm::IRBuilder<> &ir;
  llvm::air::AIRBuilder &air;
  dxbc::Converter conv; // airconv's DXBC helpers: metadata decoding, array index clamps
};

uint32_t ConstantU32(llvm::Value *v, uint32_t otherwise = 0);
// Calls a Metal simdgroup function (AIR names as `xcrun metal -S -emit-llvm` shows them), marked convergent like Metal's.
llvm::Value *Simd(llvm::IRBuilder<> &ir, llvm::StringRef name, llvm::Type *ret, llvm::ArrayRef<llvm::Value *> args);

} // namespace dxmt::dxil
