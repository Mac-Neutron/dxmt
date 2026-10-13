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
#include "dxil_lower.hpp"
#include "../airconv_error.hpp"
#include "llvm/IR/Instructions.h"

namespace dxmt::dxil {

using namespace dxmt::dxbc;
using shader::common::ResourceType;
using shader::common::ScalerDataType;

namespace {

ResourceType ToResourceType(ResourceKind kind) {
  switch (kind) {
  case ResourceKind::Texture1D: return ResourceType::Texture1D;
  case ResourceKind::Texture1DArray: return ResourceType::Texture1DArray;
  case ResourceKind::Texture2D: return ResourceType::Texture2D;
  case ResourceKind::Texture2DArray: return ResourceType::Texture2DArray;
  case ResourceKind::Texture2DMS: return ResourceType::Texture2DMultisampled;
  case ResourceKind::Texture2DMSArray: return ResourceType::Texture2DMultisampledArray;
  case ResourceKind::Texture3D: return ResourceType::Texture3D;
  case ResourceKind::TextureCube: return ResourceType::TextureCube;
  case ResourceKind::TextureCubeArray: return ResourceType::TextureCubeArray;
  case ResourceKind::TypedBuffer: return ResourceType::TextureBuffer;
  default: return ResourceType::NonApplicable; // raw and structured buffers
  }
}

ScalerDataType ToScalerType(ComponentType type) {
  switch (type) {
  case ComponentType::I16:
  case ComponentType::I32:
  case ComponentType::I64: return ScalerDataType::Int;
  case ComponentType::U16:
  case ComponentType::U32:
  case ComponentType::U64: return ScalerDataType::Uint;
  default: return ScalerDataType::Float;
  }
}

// The resource annotateHandle's ResourceProperties {p0, p1} describe (DxilResourceProperties.h): p0 bits 0-7 kind,
// 12 UAV, 13 ROV, 14 globally coherent, 15 comparison sampler or UAV counter; p1 the component type of textures and
// typed buffers (bits 0-7), a structured buffer's stride or a constant buffer's size. As a heap range (spec 3.4).
Resource HeapResource(uint32_t p0, uint32_t p1) {
  Resource r;
  r.kind = ResourceKind(p0 & 0xff);
  bool uav = p0 & (1u << 12), bit15 = p0 & (1u << 15);
  r.cls = r.kind == ResourceKind::CBuffer   ? ResourceClass::CBuffer
          : r.kind == ResourceKind::Sampler ? ResourceClass::Sampler
          : uav                             ? ResourceClass::UAV
                                            : ResourceClass::SRV;
  r.space = kHeapSpace, r.lower_bound = 0, r.size = ~0u;
  r.rasterizer_ordered = p0 & (1u << 13);
  r.globally_coherent = p0 & (1u << 14);
  r.comparison_sampler = r.cls == ResourceClass::Sampler && bit15;
  r.has_counter = r.cls == ResourceClass::UAV && bit15;
  switch (r.kind) {
  case ResourceKind::StructuredBuffer: r.stride = p1; break;
  case ResourceKind::CBuffer: r.cbuffer_size = p1; break;
  case ResourceKind::RawBuffer: case ResourceKind::Sampler: break;
  default: r.element_type = ComponentType(p1 & 0xff); break;
  }
  r.heap = true, r.heap_props[0] = p0, r.heap_props[1] = p1;
  return r;
}

// Dynamic resources (spec 3.4): a heap range for each distinct ResourceProperties that annotateHandle gives a
// createHandleFromHeap handle (ResolveHandle finds it by them), with range IDs from kHeapRangeBase.
void AddHeapRanges(const llvm::Module &module, EntryInfo &entry) {
  auto annotate = module.getFunction("dx.op.annotateHandle");
  if (!annotate)
    return;
  uint32_t next = kHeapRangeBase;
  for (auto *user : annotate->users()) {
    auto call = llvm::dyn_cast<llvm::CallInst>(user);
    uint32_t props[2];
    if (!call || !HeapHandle(*call, props) ||
        std::any_of(entry.resources.begin(), entry.resources.end(), [&](const Resource &r) {
          return r.heap && r.heap_props[0] == props[0] && r.heap_props[1] == props[1];
        }))
      continue;
    auto r = HeapResource(props[0], props[1]);
    r.id = next++;
    entry.resources.push_back(r);
  }
}

// 64-bit groupshared atomics: atomicrmw or cmpxchg of i64 on addrspace(3), which no dx.op carries. Metal has no
// threadgroup ulong atomics (its compiler service crashes on them, the i64 spike), so they run under a threadgroup lock
// array (Task 14c): 256 u32 locks (1 KiB), or the largest power of two that fits beside the shader's own groupshared
// memory in D3D's 32768 bytes. 0 for a shader without one.
llvm::Expected<uint32_t> GroupSharedLocks(const llvm::Module &module) {
  bool any = false;
  for (auto &fn : module)
    for (auto &bb : fn)
      for (auto &inst : bb) {
        if (auto rmw = llvm::dyn_cast<llvm::AtomicRMWInst>(&inst);
            rmw && rmw->getPointerAddressSpace() == 3 && rmw->getValOperand()->getType()->isIntegerTy(64)) {
          switch (rmw->getOperation()) {
          case llvm::AtomicRMWInst::Xchg: case llvm::AtomicRMWInst::Add: case llvm::AtomicRMWInst::And:
          case llvm::AtomicRMWInst::Or: case llvm::AtomicRMWInst::Xor: case llvm::AtomicRMWInst::Max:
          case llvm::AtomicRMWInst::Min: case llvm::AtomicRMWInst::UMax: case llvm::AtomicRMWInst::UMin:
            break;
          default: // DXC emits none
            return llvm::make_error<UnsupportedFeature>("DXIL: 64-bit groupshared atomicrmw " +
                                                        llvm::AtomicRMWInst::getOperationName(rmw->getOperation()).str() +
                                                        " not supported");
          }
          any = true;
        }
        auto cx = llvm::dyn_cast<llvm::AtomicCmpXchgInst>(&inst);
        if (cx && cx->getPointerAddressSpace() == 3 && cx->getNewValOperand()->getType()->isIntegerTy(64))
          any = true;
      }
  if (!any)
    return 0;
  uint64_t used = 0;
  for (auto &gv : module.globals())
    if (gv.getAddressSpace() == 3)
      used += module.getDataLayout().getTypeAllocSize(gv.getValueType());
  uint64_t room = used < 32768 ? 32768 - used : 0;
  if (room < 4)
    return llvm::make_error<UnsupportedFeature>(
        "DXIL: 64-bit groupshared atomics need 4 bytes of threadgroup memory for a lock; the shader uses " +
        std::to_string(used) + " of 32768");
  uint32_t n = 256;
  while (n * 4 > room)
    n /= 2;
  return n;
}

struct Usage {
  bool read = false, written = false, sampled = false, compared = false, atomic = false, atomic64 = false;
};

// What each resource is used for: airconv's binding maps pick Metal access modes and depth textures from it.
std::map<std::pair<ResourceClass, uint32_t>, Usage>
ScanUsage(const EntryInfo &entry, const llvm::Module &module) {
  std::map<std::pair<ResourceClass, uint32_t>, Usage> usage;
  for (auto &fn : module)
    if (fn.getName().startswith("dx.op."))
      for (auto *user : fn.users()) {
        auto call = llvm::dyn_cast<llvm::CallInst>(user);
        if (!call || call->arg_size() < 2)
          continue;
        uint32_t opcode = OpCode(*call);
        auto handle = ResolveHandle(entry, call->getArgOperand(1));
        if (!handle) {
          llvm::consumeError(handle.takeError());
          continue;
        }
        auto &u = usage[{handle->cls, handle->range}];
        switch (opcode) {
        case op::Sample: case op::SampleBias: case op::SampleLevel: case op::SampleGrad: case op::TextureGather:
        case op::CalculateLOD:
          u.sampled = true;
          break;
        case op::SampleCmp: case op::SampleCmpLevelZero: case op::TextureGatherCmp: case op::SampleCmpGrad:
        case op::SampleCmpBias:
          u.sampled = u.compared = true;
          break;
        case op::TextureLoad: case op::BufferLoad: case op::RawBufferLoad: case op::CBufferLoad: case op::CBufferLoadLegacy:
          u.read = true;
          break;
        case op::TextureStore: case op::BufferStore: case op::RawBufferStore:
          u.written = true;
          break;
        case op::AtomicBinOp: case op::AtomicCompareExchange:
          u.read = u.written = u.atomic = true;
          u.atomic64 |= call->getType()->isIntegerTy(64);
          break;
        }
      }
  return usage;
}

// airconv's resource maps (ShaderInfo), keyed by range ID, as the DXBC dcl_* declarations fill them.
void FillResourceMaps(const EntryInfo &entry, const llvm::Module &module, ShaderInfo &info) {
  auto usage = ScanUsage(entry, module);
  for (auto &r : entry.resources) {
    ResourceRange range{r.id, r.lower_bound, r.size, r.space};
    auto u = usage[{r.cls, r.id}];
    switch (r.cls) {
    case ResourceClass::SRV: {
      ShaderResourceViewInfo srv{range, ToScalerType(r.element_type), ToResourceType(r.kind)};
      srv.read = u.read;
      srv.sampled = u.sampled;
      srv.compared = u.compared;
      srv.structure_stride = r.stride;
      srv.arg_index = srv.arg_metadata_index = ~0u;
      info.srvMap[r.id] = srv;
      break;
    }
    case ResourceClass::UAV: {
      UnorderedAccessViewInfo uav{range, ToScalerType(r.element_type), ToResourceType(r.kind)};
      uav.read = u.read;
      uav.written = u.written;
      uav.global_coherent = r.globally_coherent;
      uav.rasterizer_order = r.rasterizer_ordered;
      uav.with_counter = r.has_counter;
      // Float atomics on a typed R32_FLOAT UAV (SM 6.6; spec 3.7) go through an R32Uint view of it: its loads and
      // stores bitcast (Cast), its atomics take the integer path.
      uav.atomic_view = u.atomic && r.element_type == ComponentType::F32 && uav.resource_type != ResourceType::NonApplicable;
      if (uav.atomic_view)
        uav.scaler_type = ScalerDataType::Uint;
      // 64-bit atomics on a typed buffer (Task 14b) go through its buffer's address: the same lock as a raw view's. By
      // use: DXC 1.9 declares a RWBuffer<uint64_t> or <int64_t> U32, with an Atomic64Use tag that heap handles lack.
      uav.address64 = u.atomic64 && r.kind == ResourceKind::TypedBuffer;
      uav.structure_stride = r.stride;
      uav.arg_index = uav.arg_metadata_index = uav.arg_counter_index = ~0u;
      info.uavMap[r.id] = uav;
      break;
    }
    case ResourceClass::CBuffer:
      info.cbufferMap[r.id] = ConstantBufferInfo{range, (r.cbuffer_size + 15) / 16, ~0u};
      break;
    case ResourceClass::Sampler:
      info.samplerMap[r.id] = SamplerInfo{range, ~0u, ~0u, ~0u};
      break;
    }
  }
}

} // namespace

llvm::Error
InitializeDXIL(const Container &container, SM50ShaderInternal *shader, MTL_SHADER_REFLECTION *refl) {
  llvm::LLVMContext context;
  context.setOpaquePointers(false);
  auto module = LoadModule(context, container.bitcode, container.bitcode_size);
  if (!module)
    return module.takeError();
  auto entry = ReadEntry(**module);
  if (!entry)
    return entry.takeError();
  auto gs_locks = GroupSharedLocks(**module);
  if (!gs_locks)
    return gs_locks.takeError();
  AddHeapRanges(**module, *entry);
  if (entry->wave_size_min == kMalformedWaveSize)
    return llvm::make_error<UnsupportedFeature>("DXIL: malformed WaveSize metadata");
  // Apple GPUs run 32-lane SIMD groups (d3d12_device.cpp's WaveLaneCountMin/Max); the preferred size is a hint.
  if (entry->wave_size_min && !(entry->wave_size_min <= 32 && 32 <= entry->wave_size_max))
    return llvm::make_error<UnsupportedFeature>("DXIL: [WaveSize(" + std::to_string(entry->wave_size_min) + ", " +
                                                std::to_string(entry->wave_size_max) +
                                                ")] excludes this device's 32-lane waves");
  switch (entry->kind) {
  case ShaderKind::Vertex: shader->shader_type = microsoft::D3D10_SB_VERTEX_SHADER; break;
  case ShaderKind::Pixel: shader->shader_type = microsoft::D3D10_SB_PIXEL_SHADER; break;
  case ShaderKind::Compute: shader->shader_type = microsoft::D3D11_SB_COMPUTE_SHADER; break;
  case ShaderKind::Geometry:
    if (entry->gs_stream_mask != 1) // streams other than 0 only feed stream output, which DXMT lacks
      return llvm::make_error<UnsupportedFeature>("DXIL: geometry shader streams other than 0 not supported");
    shader->shader_type = microsoft::D3D10_SB_GEOMETRY_SHADER;
    shader->gs_input_primitive = (microsoft::D3D10_SB_PRIMITIVE)entry->gs_input_primitive;
    shader->gs_output_topology = (microsoft::D3D10_SB_PRIMITIVE_TOPOLOGY)entry->gs_output_topology;
    shader->gs_max_vertex_output = entry->gs_max_vertex_count;
    shader->gs_instance_count = entry->gs_instances;
    break;
  default: return llvm::make_error<UnsupportedFeature>("DXIL: only vertex, pixel, geometry and compute shaders are supported");
  }
  FillResourceMaps(*entry, **module, shader->shader_info);
  // B6: D3D12 render passes list their attachments as their writes; whatever this writes, they can't.
  bool writes_uav = false;
  for (uint32_t o : {op::TextureStore, op::BufferStore, op::RawBufferStore, op::BufferUpdateCounter, op::AtomicBinOp,
                     op::AtomicCompareExchange})
    writes_uav |= ModuleCallsOp(**module, o);
  // SM50Compile links airconv's msad and sample position libraries for these.
  shader->shader_info.use_msad = ModuleCallsOp(**module, op::Msad);
  shader->shader_info.use_samplepos = ModuleCallsOp(**module, op::Texture2DMSGetSamplePosition) ||
                                      ModuleCallsOp(**module, op::RenderTargetGetSamplePosition);
  // Ops 78 and 79 with the i64 overload take the device's lock table from the root arguments (Task 14a). Each stage of
  // a geometry pipeline binds with its own ShaderInfo, so its function declares the field only for its own atomics.
  for (auto &fn : **module)
    if (fn.getName().startswith("dx.op.") && fn.getReturnType()->isIntegerTy(64))
      for (auto *user : fn.users())
        if (auto call = llvm::dyn_cast<llvm::CallInst>(user))
          shader->shader_info.use_atomic64_locks |=
              OpCode(*call) == op::AtomicBinOp || OpCode(*call) == op::AtomicCompareExchange;
  // 2D compute quads (HLSL_SM_6_6_Derivatives.md): numthreads X and Y even (Y = 1 is the 1D layout, Metal's own), in a
  // shader that takes derivatives, samples with an implicit LOD, calculates a LOD or reads its quad.
  std::initializer_list<uint32_t> quad_ops = {op::Sample, op::SampleBias, op::SampleCmp, op::CalculateLOD,
                                              op::DerivCoarseX, op::DerivCoarseY, op::DerivFineX, op::DerivFineY,
                                              op::QuadReadLaneAt, op::QuadOp, op::QuadVote, op::SampleCmpBias};
  bool quads_2d = entry->kind == ShaderKind::Compute && entry->numthreads[0] % 2 == 0 && entry->numthreads[1] % 2 == 0 &&
                  std::any_of(quad_ops.begin(), quad_ops.end(), [&](uint32_t o) { return ModuleCallsOp(**module, o); });
  std::map<uint32_t, uint32_t> unpacked_inputs;
  if (auto err = AddSignatureHandlers(*entry, **module, shader, unpacked_inputs, quads_2d, *gs_locks != 0))
    return err;
  uint32_t lane_index_arg = ~0u, lane_count_arg = ~0u;
  AddWaveInputs(**module, shader, lane_index_arg, lane_count_arg);
  if (entry->kind == ShaderKind::Compute) {
    std::copy(entry->numthreads, entry->numthreads + 3, shader->threadgroup_size);
    shader->func_signature.UseMaxWorkgroupSize(entry->numthreads[0] * entry->numthreads[1] * entry->numthreads[2]);
  }
  auto dxil = std::make_shared<DXILShader>();
  if (auto fn = (*module)->getFunction(entry->name))
    dxil->include_helper_lanes = fn->hasFnAttribute("waveops-include-helper-lanes");
  dxil->bitcode.assign(container.bitcode, container.bitcode + container.bitcode_size);
  dxil->capture_hash = container.capture_hash;
  dxil->entry = std::move(*entry);
  dxil->unpacked_inputs = std::move(unpacked_inputs);
  dxil->quads_2d = quads_2d;
  dxil->gs_locks = *gs_locks;
  dxil->lane_index_arg = lane_index_arg;
  dxil->lane_count_arg = lane_count_arg;
  shader->dxil = dxil;
  if (refl) {
    *refl = {};
    refl->ConstanttBufferTableBindIndex = ~0u;
    refl->ArgumentBufferBindIndex = ~0u;
    if (shader->shader_type == microsoft::D3D11_SB_COMPUTE_SHADER)
      std::copy(shader->threadgroup_size, shader->threadgroup_size + 3, refl->ThreadgroupSize);
    if (shader->shader_type == microsoft::D3D10_SB_GEOMETRY_SHADER) {
      refl->GeometryShader.GSPassThrough = ~0u; // never a pass-through: always the mesh pipeline
      refl->GeometryShader.Primitive = shader->gs_input_primitive;
    }
    if (shader->shader_type == microsoft::D3D10_SB_PIXEL_SHADER) {
      refl->PixelShader.ValidRenderTargets = shader->pso_valid_output_reg_mask;
      refl->PixelShader.HasCoverageOutput = shader->ps_has_coverage_output;
    }
    refl->NumOutputElement = shader->max_output_register;
    refl->WritesUAV = writes_uav;
  }
  return llvm::Error::success();
}

} // namespace dxmt::dxil
