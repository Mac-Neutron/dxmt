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
#include "llvm/IR/Constants.h"
#include "llvm/IR/Instructions.h"

namespace dxmt::dxil {

using llvm::air::Texture;

llvm::Optional<dxbc::BufferResourceHandle>
Lowering::Buffer(const HandleInfo &h) {
  auto d = h.cls == ResourceClass::UAV ? ctx.binding.GetUAVBuffer(air, h.range, h.index)
                                       : ctx.binding.GetSRVBuffer(air, h.range, h.index);
  if (!d)
    return {};
  return dxbc::BufferResourceHandle{d->Pointer, d->Metadata, d->StructureStride, dxbc::swizzle_identity, d->GlobalCoherent};
}

llvm::Optional<dxbc::TextureDescirptor>
Lowering::Texture(const HandleInfo &h) {
  return h.cls == ResourceClass::UAV ? ctx.binding.GetUAVTexture(air, h.range, h.index)
                                     : ctx.binding.GetSRVTexture(air, h.range, h.index);
}

llvm::Value *
Lowering::ElementPointer(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Type *ty) {
  unsigned space = llvm::cast<llvm::PointerType>(buffer.Pointer->getType())->getAddressSpace();
  auto bytes = ir.CreateBitCast(buffer.Pointer, ir.getInt8PtrTy(space));
  auto ptr = ir.CreateBitCast(ir.CreateGEP(ir.getInt8Ty(), bytes, byte_offset), ty->getPointerTo(space));
  if (!buffer.Metadata || DxilBoundsOff())
    return ptr;
  return ir.CreateSelect(InBounds(buffer, byte_offset, ty->getPrimitiveSizeInBits() / 8), ptr,
                         llvm::Constant::getNullValue(ptr->getType()));
}

// `bytes` from `byte_offset` lie inside the buffer's view, without wrapping: an offset near 4 GiB (a -1 index, say)
// mustn't pass the check. In 32 bits (offset <= length - bytes, length >= bytes): Apple GPUs have no 64-bit integer
// ALU, and a 64-bit form cost SMITE 2 0.5 ms of vertex time per frame.
llvm::Value *
Lowering::InBounds(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, uint64_t bytes) {
  auto length = conv.DecodeRawBufferByteLength(buffer.Metadata);
  auto n = ir.getInt32(bytes);
  return ir.CreateAnd(ir.CreateICmpUGE(length, n), ir.CreateICmpULE(byte_offset, ir.CreateSub(length, n)));
}

llvm::Value *
Lowering::AccessPointer(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Type *ty, unsigned count) {
  unsigned space = llvm::cast<llvm::PointerType>(buffer.Pointer->getType())->getAddressSpace();
  auto bytes = ir.CreateBitCast(buffer.Pointer, ir.getInt8PtrTy(space));
  auto ptr = ir.CreateBitCast(ir.CreateGEP(ir.getInt8Ty(), bytes, byte_offset), ty->getPointerTo(space));
  if (!buffer.Metadata || DxilBoundsOff())
    return ptr;
  return ir.CreateSelect(InBounds(buffer, byte_offset, count * (ty->getPrimitiveSizeInBits() / 8)), ptr,
                         llvm::Constant::getNullValue(ptr->getType()));
}

llvm::Value *
Lowering::LoadElement(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Type *ty) {
  auto ptr = ElementPointer(buffer, byte_offset, ty);
  return buffer.GlobalCoherent ? (llvm::Value *)air.CreateDeviceCoherentLoad(ty, ptr) : ir.CreateLoad(ty, ptr);
}

void
Lowering::StoreElement(dxbc::BufferResourceHandle &buffer, llvm::Value *byte_offset, llvm::Value *value) {
  auto ptr = ElementPointer(buffer, byte_offset, value->getType());
  if (buffer.GlobalCoherent)
    air.CreateDeviceCoherentStore(value, ptr);
  else
    ir.CreateStore(value, ptr);
}

// The components of a ResRet the shader reads (SM 6.0/6.1 BufferLoad names all four: one bounds check covering all of
// them would zero a load of a view's last dword). Any use but extractvalue of a component: all four.
uint32_t
Lowering::ExtractedComponents(llvm::CallInst *call) {
  uint32_t mask = 0;
  for (auto *user : call->users()) {
    auto extract = llvm::dyn_cast<llvm::ExtractValueInst>(user);
    if (!extract || extract->getNumIndices() != 1)
      return 0xf;
    if (extract->getIndices()[0] < 4)
      mask |= 1u << extract->getIndices()[0];
  }
  return mask ? mask : 0x1;
}

// %dx.types.ResRet.T = {T, T, T, T, i32 status}; components outside `mask` stay undef.
llvm::Error
Lowering::LowerBufferLoad(llvm::CallInst *call, const HandleInfo &h, llvm::Value *byte_offset, uint32_t mask) {
  auto buffer = Buffer(h);
  auto ty = call->getType();
  auto elem = ty->getStructElementType(0);
  if (!buffer) {
    Replace(call, llvm::Constant::getNullValue(ty));
    return llvm::Error::success();
  }
  llvm::Value *fields[5] = {};
  unsigned size = elem->getPrimitiveSizeInBits() / 8;
  if (!DxilBoundsPerComponent()) { // one check for every component loaded: all of them or none (E5)
    auto base = AccessPointer(*buffer, byte_offset, elem, 32 - __builtin_clz(mask));
    for (unsigned c = 0; c < 4; c++)
      if (mask & (1 << c)) {
        auto ptr = ir.CreateConstGEP1_32(elem, base, c);
        fields[c] = buffer->GlobalCoherent ? (llvm::Value *)air.CreateDeviceCoherentLoad(elem, ptr) : ir.CreateLoad(elem, ptr);
      }
  } else
    for (unsigned c = 0; c < 4; c++)
      if (mask & (1 << c))
        fields[c] = LoadElement(*buffer, ir.CreateAdd(byte_offset, ir.getInt32(c * size)), elem);
  fields[4] = ir.getInt32(1); // status: fully mapped
  Replace(call, Aggregate(ty, fields));
  return llvm::Error::success();
}

llvm::Error
Lowering::LowerBufferStore(llvm::CallInst *call, const HandleInfo &h, llvm::Value *byte_offset, unsigned first_value, uint32_t mask) {
  auto buffer = Buffer(h);
  if (buffer) {
    auto elem = call->getArgOperand(first_value)->getType();
    unsigned size = elem->getPrimitiveSizeInBits() / 8;
    for (unsigned c = 0; c < 4; c++)
      if (mask & (1 << c))
        StoreElement(*buffer, ir.CreateAdd(byte_offset, ir.getInt32(c * size)), call->getArgOperand(first_value + c));
  }
  Replace(call, nullptr);
  return llvm::Error::success();
}

// Typed buffers are Metal texture buffers; the descriptor's metadata holds the view's first element and element count
// (as the DXBC ld/store_uav_typed lowering reads it).
llvm::Error
Lowering::LowerTypedBufferLoad(llvm::CallInst *call, const HandleInfo &h, llvm::Value *index) {
  auto d = Texture(h);
  auto ty = call->getType();
  if (!d) {
    Replace(call, llvm::Constant::getNullValue(ty));
    return llvm::Error::success();
  }
  llvm::air::Texture tex;
  tex.kind = d->ResourceKind;
  tex.sample_type = d->SampleType;
  tex.memory_access = d->MemoryAccess;
  auto address = ir.CreateAdd(index, conv.DecodeTextureBufferOffset(d->Metadata));
  if (!DxilBoundsOff()) {
    auto oob = ir.CreateICmpUGE(index, conv.DecodeTextureBufferElement(d->Metadata));
    address = ir.CreateSelect(oob, llvm::ConstantInt::getAllOnesValue(address->getType()), address);
  }
  if (tex.memory_access == llvm::air::Texture::acesss_readwrite) // as the DXBC typed UAV load: see earlier writes
    air.CreateTextureFence(tex, d->ResourceHandle);
  auto [texel, residency] = air.CreateRead(tex, d->ResourceHandle, address, nullptr, nullptr, ir.getInt32(0), d->GlobalCoherent);
  auto elem = ty->getStructElementType(0);
  llvm::Value *fields[5];
  for (unsigned c = 0; c < 4; c++)
    fields[c] = Cast(ir.CreateExtractElement(texel, c), elem);
  fields[4] = ir.getInt32(1);
  Replace(call, Aggregate(ty, fields));
  return llvm::Error::success();
}

llvm::Error
Lowering::LowerTypedBufferStore(llvm::CallInst *call, const HandleInfo &h, llvm::Value *index) {
  // bufferStore(69, handle, index, offset, v0, v1, v2, v3, mask) / textureStore-like: values at 4..7
  auto d = Texture(h);
  if (d) {
    llvm::air::Texture tex;
    tex.kind = d->ResourceKind;
    tex.sample_type = d->SampleType;
    tex.memory_access = d->MemoryAccess;
    auto texel_ty = llvm::cast<llvm::FixedVectorType>(air.getTexelType(tex));
    llvm::Value *texel = llvm::UndefValue::get(texel_ty);
    for (unsigned c = 0; c < 4; c++)
      texel = ir.CreateInsertElement(texel, Cast(call->getArgOperand(4 + c), texel_ty->getElementType()), c);
    auto address = ir.CreateAdd(index, conv.DecodeTextureBufferOffset(d->Metadata));
    air.CreateWrite(tex, d->ResourceHandle, address, nullptr, nullptr, ir.getInt32(0), texel, d->GlobalCoherent);
  }
  Replace(call, nullptr);
  return llvm::Error::success();
}

namespace {

// Coordinates, array slice and sampler choice for a texture kind, as airconv's DXBC sample lowering builds them
// (nt/dxbc_converter_base.cpp, InstSample). `c` holds DXIL's c0..c3 (undef where unused).
struct Addressing {
  llvm::Value *coord = nullptr;
  llvm::Value *array = nullptr;
  bool cube = false;
};

bool IsArray(Texture::ResourceKind k) {
  switch (k) {
  case Texture::texture1d_array: case Texture::texture2d_array: case Texture::depth2d_array: case Texture::texturecube_array:
  case Texture::depthcube_array: case Texture::texture2d_ms_array: case Texture::depth_2d_ms_array: return true;
  default: return false;
  }
}

bool IsMultisampled(Texture::ResourceKind k) {
  return k == Texture::texture2d_ms || k == Texture::texture2d_ms_array || k == Texture::depth_2d_ms ||
         k == Texture::depth_2d_ms_array;
}

// Components of the position/coordinate vector (1D textures are 2D in Metal here, as the DXBC path treats them).
unsigned Dimensions(Texture::ResourceKind k) {
  switch (k) {
  case Texture::texture3d: case Texture::texturecube: case Texture::depthcube: case Texture::texturecube_array:
  case Texture::depthcube_array: return 3;
  default: return 2;
  }
}

// DXIL's atomic operation codes (DXIL.rst, AtomicBinOpCode) as LLVM's.
llvm::AtomicRMWInst::BinOp AtomicOp(llvm::Value *code) {
  using B = llvm::AtomicRMWInst::BinOp;
  static const B ops[] = {B::Add, B::And, B::Or, B::Xor, B::Min, B::Max, B::UMin, B::UMax, B::Xchg};
  return ops[std::min(ConstantU32(code), 8u)];
}

// The component of c0..c3 holding the array slice.
unsigned ArrayComponent(Texture::ResourceKind k) {
  switch (k) {
  case Texture::texture1d_array: return 1;
  case Texture::texturecube_array: case Texture::depthcube_array: return 3;
  default: return 2;
  }
}

} // namespace

llvm::Error
Lowering::LowerTexture(uint32_t opcode, llvm::CallInst *call, const HandleInfo &h) {
  auto arg = [&](unsigned i) { return call->getArgOperand(i); };
  auto ty = call->getType();
  auto d = Texture(h);
  if (!d) {
    Replace(call, ty->isVoidTy() ? nullptr : llvm::Constant::getNullValue(ty));
    return llvm::Error::success();
  }
  llvm::air::Texture tex;
  tex.kind = d->ResourceKind;
  tex.sample_type = d->SampleType;
  tex.memory_access = d->MemoryAccess;
  auto logical = d->ResourceKindLogical;
  unsigned dims = Dimensions(logical);
  bool one_d = logical == Texture::texture1d || logical == Texture::texture1d_array;

  // Float coordinates from c0.. (sample/gather/lod) or integer positions (load/store).
  auto vector = [&](unsigned first, llvm::Type *elem) -> llvm::Value * {
    llvm::Value *v = llvm::UndefValue::get(llvm::FixedVectorType::get(elem, dims));
    for (unsigned i = 0; i < dims; i++)
      v = ir.CreateInsertElement(v, one_d && i == 1 ? llvm::Constant::getNullValue(elem) : arg(first + i), i);
    return v;
  };
  auto array_index = [&](unsigned first) -> llvm::Value * {
    if (!IsArray(logical))
      return nullptr;
    auto v = arg(first + ArrayComponent(logical));
    return v->getType()->isFloatingPointTy() ? conv.ClampArrayIndex(v, d->Metadata) : v;
  };
  // Texel offsets o0.. from argument `first` (`count` of them, the rest 0). Constants (undef is 0) as immediates.
  // Runtime ones, SM 6.7's programmable offsets and Gather's at any model, pass through: wrapped to D3D's low 4 bits,
  // sign-extended, when `wrap` (Load and Sample*); Gather's unwrapped, as the DXBC path's gather4_po (spec 3.6).
  // Constants wrap too: from 6.7 DXC no longer checks their range. Returns whether all were constant.
  auto offset_values = [&](unsigned first, unsigned count, bool wrap, llvm::Value *out[3]) {
    bool constant = true;
    for (unsigned i = 0; i < 3; i++) {
      auto v = i < count && !(one_d && i == 1) ? arg(first + i) : nullptr;
      if (v && !llvm::isa<llvm::Constant>(v)) {
        out[i] = wrap ? ir.CreateAShr(ir.CreateShl(v, 28), 28) : v;
        constant = false;
      } else {
        int32_t c = v ? (int32_t)ConstantU32(v, 0) : 0;
        out[i] = ir.getInt32(wrap ? ((c & 15) ^ 8) - 8 : c);
      }
    }
    return constant;
  };
  // The offsets as the texture kind's offset vector (AIRBuilder::GetOffset): all constant, today's immediate form.
  auto Offsets = [&](unsigned first, unsigned count, bool wrap) -> llvm::Value * {
    llvm::Value *v[3];
    if (!offset_values(first, count, wrap, v))
      return air.GetOffset(tex, v);
    int32_t o[3];
    for (unsigned i = 0; i < 3; i++)
      o[i] = (int32_t)ConstantU32(v[i]);
    return air.GetOffset(tex, o);
  };
  // The texel (vector or depth scalar) as the call's ResRet struct.
  auto result = [&](llvm::Value *texel) {
    auto elem = ty->getStructElementType(0);
    llvm::Value *fields[5];
    for (unsigned c = 0; c < 4; c++) {
      auto v = texel->getType()->isVectorTy() ? ir.CreateExtractElement(texel, c) : texel;
      fields[c] = Cast(v, elem);
    }
    fields[4] = ir.getInt32(1); // status: fully mapped
    Replace(call, Aggregate(ty, fields));
    return llvm::Error::success();
  };

  switch (opcode) {
  case op::TextureLoad: { // (66, handle, i32 mipOrSample, i32 c0, c1, c2, i32 o0, o1, o2)
    auto pos = vector(3, ir.getInt32Ty());
    llvm::Value *o[3]; // added to pos, which GetOffset has no form for on multisampled kinds
    offset_values(6, dims, true, o);
    llvm::Value *offset = llvm::UndefValue::get(pos->getType());
    for (unsigned i = 0; i < dims; i++)
      offset = ir.CreateInsertElement(offset, o[i], i);
    pos = ir.CreateAdd(pos, offset);
    bool ms = IsMultisampled(logical);
    if (tex.memory_access == Texture::acesss_readwrite) // see earlier writes, as the DXBC typed UAV load
      air.CreateTextureFence(tex, d->ResourceHandle);
    auto [texel, residency] = air.CreateRead(tex, d->ResourceHandle, pos, array_index(3), ms ? arg(2) : nullptr,
                                             ms ? ir.getInt32(0) : arg(2), d->GlobalCoherent);
    return result(texel);
  }
  case op::TextureStore: { // (67, handle, i32 c0, c1, c2, v0, v1, v2, v3, i8 mask)
    auto pos = vector(2, ir.getInt32Ty());
    auto texel_ty = llvm::cast<llvm::FixedVectorType>(air.getTexelType(tex));
    llvm::Value *texel = llvm::UndefValue::get(texel_ty);
    for (unsigned c = 0; c < 4; c++)
      texel = ir.CreateInsertElement(texel, Cast(arg(5 + c), texel_ty->getElementType()), c);
    air.CreateWrite(tex, d->ResourceHandle, pos, array_index(2), nullptr, ir.getInt32(0), texel, d->GlobalCoherent);
    Replace(call, nullptr);
    return llvm::Error::success();
  }
  case op::AtomicBinOp:             // (78, handle, i32 op, i32 c0, c1, c2, i32 value)
  case op::AtomicCompareExchange: { // (79, handle, i32 c0, c1, c2, i32 compare, i32 value)
    bool cas = opcode == op::AtomicCompareExchange;
    unsigned first = cas ? 2 : 3;
    llvm::Value *pos, *array = nullptr;
    if (logical == Texture::texture_buffer) { // as the DXBC typed UAV atomic: out of range goes to ~0u
      auto index = arg(first);
      auto oob = ir.CreateICmpUGE(index, conv.DecodeTextureBufferElement(d->Metadata));
      pos = ir.CreateSelect(oob, ir.getInt32(~0u), ir.CreateAdd(index, conv.DecodeTextureBufferOffset(d->Metadata)));
    } else {
      pos = vector(first, ir.getInt32Ty());
      array = array_index(first);
    }
    auto splat = [&](llvm::Value *v) { return ir.CreateVectorSplat(4, v); };
    auto old = cas ? air.CreateAtomicCmpXchg(tex, d->ResourceHandle, pos, splat(arg(5)), splat(arg(6)), array).first
                   : air.CreateAtomicRMW(tex, d->ResourceHandle, AtomicOp(arg(2)), pos, splat(arg(6)), array);
    if (!old)
      return Unsupported(call); // not an integer texture
    Replace(call, ir.CreateExtractElement(old, (uint64_t)0));
    return llvm::Error::success();
  }
  default:
    break;
  }

  // Sampling and gathering: the sampler is argument 2, coordinates start at 3.
  auto sampler_handle = ResolveHandle(entry, arg(2));
  if (!sampler_handle)
    return sampler_handle.takeError();
  auto s = ctx.binding.GetSampler(air, sampler_handle->range, sampler_handle->index);
  if (!s) {
    Replace(call, llvm::Constant::getNullValue(ty));
    return llvm::Error::success();
  }
  auto sampler = Dimensions(logical) == 3 && logical != Texture::texture3d ? s->CubeSamplerHandle : s->SamplerHandle;
  auto bias = ir.CreateBitCast(ir.CreateTrunc(s->Metadata, ir.getInt32Ty()), ir.getFloatTy()); // the sampler's LOD bias
  auto coord = vector(3, ir.getFloatTy());
  auto array = array_index(3);
  // Sample*'s o0..o2. Only there: Gather's argument 9 is its channel, and CalculateLOD's arguments end at 6.
  auto o = [&] { return Offsets(7, dims, true); };
  auto min_lod = [&](llvm::Value *shader_clamp) {
    auto clamp = conv.DecodeTextureMinLODClamp(d->Metadata);
    if (shader_clamp && !llvm::isa<llvm::UndefValue>(shader_clamp))
      clamp = air.CreateFPBinOp(llvm::air::AIRBuilder::fmax, clamp, shader_clamp);
    return llvm::air::sample_min_lod_clamp{clamp};
  };
  // MacNeutron: D3D adds the sampler's MipLODBias to a gradient sample's LOD; Metal's gradient sample takes no bias.
  // Gradients scaled by 2^bias move the LOD by bias, also past the anisotropy cap.
  auto scaled = [&](std::pair<llvm::Value *, llvm::Value *> grad, llvm::Value *lod_bias) {
    auto scale = ir.CreateVectorSplat(dims, air.CreateFPUnOp(llvm::air::AIRBuilder::exp2, lod_bias, false));
    auto ddx = ir.CreateFMul(grad.first, scale);
    return std::make_pair(ddx, ir.CreateFMul(grad.second, scale));
  };
  // A compute shader has no implicit derivatives (spec 3.3): Sample, SampleBias and SampleCmp take the quad's coarse
  // gradients of their coordinates through the gradient path (`reference`: SampleCmp's).
  bool compute = entry.kind == ShaderKind::Compute;
  auto coarse = [&](llvm::Value *lod_bias, llvm::Value *shader_clamp, llvm::Value *reference) {
    auto [ddx, ddy] = scaled(CoarseGradients(coord), lod_bias);
    auto clamp = min_lod(shader_clamp).lod;
    auto offset = o();
    return result((reference ? air.CreateSampleCmpGrad(tex, d->ResourceHandle, sampler, coord, array, reference, ddx, ddy,
                                                       clamp, offset)
                             : air.CreateSampleGrad(tex, d->ResourceHandle, sampler, coord, array, ddx, ddy, clamp, offset))
                      .first);
  };

  switch (opcode) {
  case op::Sample: // (60, srv, sampler, c0..c3, o0..o2, clamp)
    if (compute)
      return coarse(bias, arg(10), nullptr);
    return result(air.CreateSample(tex, d->ResourceHandle, sampler, coord, array, o(), llvm::air::sample_bias{bias}, min_lod(arg(10))).first);
  case op::SampleBias: // (61, ..., bias, clamp)
    if (compute)
      return coarse(ir.CreateFAdd(arg(10), bias), arg(11), nullptr);
    return result(air.CreateSample(tex, d->ResourceHandle, sampler, coord, array, o(),
                                   llvm::air::sample_bias{ir.CreateFAdd(arg(10), bias)}, min_lod(arg(11))).first);
  case op::SampleLevel: // (62, ..., lod)
    return result(air.CreateSample(tex, d->ResourceHandle, sampler, coord, array, o(),
                                   llvm::air::sample_level{ir.CreateFAdd(arg(10), bias)}).first);
  case op::SampleGrad: { // (63, ..., ddx0..2, ddy0..2, clamp)
    llvm::Value *ddx = llvm::UndefValue::get(coord->getType()), *ddy = ddx;
    for (unsigned i = 0; i < dims; i++) {
      ddx = ir.CreateInsertElement(ddx, one_d && i == 1 ? llvm::ConstantFP::get(ir.getFloatTy(), 0) : arg(10 + i), i);
      ddy = ir.CreateInsertElement(ddy, one_d && i == 1 ? llvm::ConstantFP::get(ir.getFloatTy(), 0) : arg(13 + i), i);
    }
    std::tie(ddx, ddy) = scaled({ddx, ddy}, bias);
    return result(air.CreateSampleGrad(tex, d->ResourceHandle, sampler, coord, array, ddx, ddy, min_lod(arg(16)).lod, o()).first);
  }
  case op::SampleCmp: // (64, ..., compare, clamp)
    if (compute)
      return coarse(bias, arg(11), arg(10));
    return result(air.CreateSampleCmp(tex, d->ResourceHandle, sampler, coord, array, arg(10), o(),
                                      llvm::air::sample_bias{bias}, min_lod(arg(11))).first);
  case op::SampleCmpLevelZero: // (65, ..., compare)
    return result(air.CreateSampleCmp(tex, d->ResourceHandle, sampler, coord, array, arg(10), o(),
                                      llvm::air::sample_level{llvm::ConstantFP::get(ir.getFloatTy(), 0)}).first);
  case op::TextureGather: // (73, srv, sampler, c0..c3, i32 o0, o1, i32 channel)
    return result(air.CreateGather(tex, d->ResourceHandle, sampler, coord, array, Offsets(7, 2, false), arg(9)).first);
  case op::TextureGatherCmp: // (74, ..., i32 channel, float compare)
    return result(air.CreateGatherCompare(tex, d->ResourceHandle, sampler, coord, array, arg(10),
                                          Offsets(7, 2, false)).first);
  case op::CalculateLOD: { // (81, srv, sampler, c0, c1, c2, i1 clamped)
    // ponytail: a cube in a compute shader keeps Metal's LOD (no derivatives in a kernel); D3D's projection onto the
    // major face when a shader needs it
    if (!compute || (Dimensions(logical) == 3 && logical != Texture::texture3d)) {
      auto lod = air.CreateCalculateLOD(tex, d->ResourceHandle, sampler, coord);
      Replace(call, ConstantU32(arg(6), 1) ? lod.first : lod.second);
      return llvm::Error::success();
    }
    // Compute (spec 3.3): 0.5 log2 of the larger squared gradient in mip 0's texels; clamped to the view's mips.
    using llvm::air::AIRBuilder;
    auto [ddx, ddy] = CoarseGradients(coord);
    static const Texture::Query axes[] = {Texture::width, Texture::height, Texture::depth};
    llvm::Value *size = llvm::UndefValue::get(coord->getType());
    for (unsigned i = 0; i < dims; i++) {
      auto n = one_d && i == 1 ? ir.getInt32(0) : air.CreateTextureQuery(tex, d->ResourceHandle, axes[i], ir.getInt32(0));
      size = ir.CreateInsertElement(size, ir.CreateUIToFP(n, ir.getFloatTy()), i);
    }
    auto texels2 = [&](llvm::Value *g) {
      auto t = ir.CreateFMul(g, size);
      return air.CreateDotProduct(t, t);
    };
    auto x2 = texels2(ddx);
    auto lod = ir.CreateFMul(llvm::ConstantFP::get(ir.getFloatTy(), 0.5),
                             air.CreateFPUnOp(AIRBuilder::log2, air.CreateFPBinOp(AIRBuilder::fmax, x2, texels2(ddy)), false));
    if (ConstantU32(arg(6), 1)) {
      auto mips = air.CreateTextureQuery(tex, d->ResourceHandle, Texture::num_mip_levels, ir.getInt32(0));
      auto top = ir.CreateUIToFP(ir.CreateSub(mips, ir.getInt32(1)), ir.getFloatTy());
      lod = air.CreateFPBinOp(AIRBuilder::fmin,
                              air.CreateFPBinOp(AIRBuilder::fmax, lod, llvm::ConstantFP::get(ir.getFloatTy(), 0)), top);
    }
    Replace(call, lod);
    return llvm::Error::success();
  }
  default:
    return Unsupported(call);
  }
}

// %dx.types.Dimensions {width, height, depth or array size, mip levels or sample count}, as the DXBC resinfo/bufinfo
// lowerings answer them.
llvm::Error
Lowering::LowerDimensions(llvm::CallInst *call, const HandleInfo &h) {
  auto ty = call->getType();
  auto kind = h.resource->kind;
  if (kind == ResourceKind::RawBuffer || kind == ResourceKind::StructuredBuffer) {
    auto buffer = Buffer(h);
    llvm::Value *size = buffer && buffer->Metadata ? conv.DecodeRawBufferByteLength(buffer->Metadata) : ir.getInt32(0);
    if (kind == ResourceKind::StructuredBuffer && h.resource->stride)
      size = ir.CreateUDiv(size, ir.getInt32(h.resource->stride));
    Replace(call, Aggregate(ty, {size, ir.getInt32(0), ir.getInt32(0), ir.getInt32(0)}));
    return llvm::Error::success();
  }
  auto d = Texture(h);
  if (!d) {
    Replace(call, llvm::Constant::getNullValue(ty));
    return llvm::Error::success();
  }
  llvm::air::Texture tex;
  tex.kind = d->ResourceKind;
  tex.sample_type = d->SampleType;
  tex.memory_access = d->MemoryAccess;
  auto logical = d->ResourceKindLogical;
  if (kind == ResourceKind::TypedBuffer) {
    Replace(call, Aggregate(ty, {conv.DecodeTextureBufferElement(d->Metadata), ir.getInt32(0), ir.getInt32(0), ir.getInt32(0)}));
    return llvm::Error::success();
  }
  auto level = call->getArgOperand(2);
  if (llvm::isa<llvm::UndefValue>(level))
    level = ir.getInt32(0);
  auto q = [&](Texture::Query query, llvm::Value *lod) { return air.CreateTextureQuery(tex, d->ResourceHandle, query, lod); };
  llvm::Value *x = q(Texture::width, level), *y = ir.getInt32(0), *z = ir.getInt32(0);
  if (logical == Texture::texture1d_array)
    y = q(Texture::array_length, ir.getInt32(0));
  else if (logical != Texture::texture1d)
    y = q(Texture::height, level);
  if (logical == Texture::texture3d)
    z = q(Texture::depth, level);
  else if (IsArray(logical) && logical != Texture::texture1d_array)
    z = q(Texture::array_length, ir.getInt32(0));
  auto last = IsMultisampled(logical) ? q(Texture::num_samples, ir.getInt32(0)) : q(Texture::num_mip_levels, ir.getInt32(0));
  Replace(call, Aggregate(ty, {x, y, z, last}));
  return llvm::Error::success();
}

llvm::Error
Lowering::LowerResource(uint32_t opcode, llvm::CallInst *call) {
  if (opcode == op::CheckAccessFullyMapped) { // (71, i32 status): every resource is fully mapped here
    Replace(call, ir.CreateICmpNE(call->getArgOperand(1), ir.getInt32(0)));
    return llvm::Error::success();
  }
  auto handle = ResolveHandle(entry, call->getArgOperand(1));
  if (!handle)
    return handle.takeError();
  auto &h = *handle;
  auto kind = h.resource->kind;
  bool structured = kind == ResourceKind::StructuredBuffer;
  auto stride = ir.getInt32(h.resource->stride);

  switch (opcode) {
  case op::CBufferLoadLegacy: { // (59, handle, i32 row) -> %dx.types.CBufRet.T
    auto ty = call->getType();
    auto d = ctx.binding.GetConstantBuffer(air, h.range, h.index);
    if (!d) {
      Replace(call, llvm::Constant::getNullValue(ty));
      return llvm::Error::success();
    }
    auto row_ty = air.getIntTy(4);
    auto row = ir.CreateLoad(row_ty, ir.CreateGEP(row_ty, d->Pointer, {call->getArgOperand(2)}));
    auto elem = ty->getStructElementType(0);
    unsigned count = ty->getStructNumElements(), bits = elem->getPrimitiveSizeInBits();
    llvm::SmallVector<llvm::Value *, 8> fields;
    for (unsigned e = 0; e < count; e++) {
      llvm::Value *v;
      if (bits == 32) {
        v = ir.CreateExtractElement(row, e);
      } else if (bits == 16) {
        v = ir.CreateTrunc(ir.CreateLShr(ir.CreateExtractElement(row, e / 2), (e % 2) * 16), ir.getInt16Ty());
      } else { // 64
        auto lo = ir.CreateZExt(ir.CreateExtractElement(row, e * 2), ir.getInt64Ty());
        auto hi = ir.CreateZExt(ir.CreateExtractElement(row, e * 2 + 1), ir.getInt64Ty());
        v = ir.CreateOr(lo, ir.CreateShl(hi, 32));
      }
      fields.push_back(ir.CreateBitCast(v, elem));
    }
    Replace(call, Aggregate(ty, fields));
    return llvm::Error::success();
  }
  case op::CBufferLoad: { // (58, handle, i32 byteOffset, i32 alignment) -> T
    auto ty = call->getType();
    auto d = ctx.binding.GetConstantBuffer(air, h.range, h.index);
    if (!d) {
      Replace(call, llvm::Constant::getNullValue(ty));
      return llvm::Error::success();
    }
    auto offset = call->getArgOperand(2);
    auto word_ptr = ir.CreateGEP(air.getIntTy(4), d->Pointer, {ir.CreateLShr(offset, 4), ir.CreateAnd(ir.CreateLShr(offset, 2), 3)});
    llvm::Value *word = ir.CreateLoad(ir.getInt32Ty(), word_ptr);
    if (ty->getPrimitiveSizeInBits() == 16)
      word = ir.CreateTrunc(ir.CreateLShr(word, ir.CreateShl(ir.CreateAnd(offset, 2), 3)), ir.getInt16Ty());
    else if (ty->getPrimitiveSizeInBits() == 64)
      return llvm::make_error<UnsupportedFeature>("DXIL: 64-bit cbufferLoad not supported");
    Replace(call, ir.CreateBitCast(word, ty));
    return llvm::Error::success();
  }
  case op::RawBufferLoad: { // (139, handle, i32 index, i32 elementOffset, i8 mask, i32 alignment)
    auto offset = structured ? ir.CreateAdd(ir.CreateMul(call->getArgOperand(2), stride), call->getArgOperand(3))
                             : call->getArgOperand(2);
    return LowerBufferLoad(call, h, offset, ConstantU32(call->getArgOperand(4), 0xf));
  }
  case op::RawBufferStore: { // (140, handle, i32 index, i32 elementOffset, v0..v3, i8 mask, i32 alignment)
    auto offset = structured ? ir.CreateAdd(ir.CreateMul(call->getArgOperand(2), stride), call->getArgOperand(3))
                             : call->getArgOperand(2);
    return LowerBufferStore(call, h, offset, 4, ConstantU32(call->getArgOperand(8), 0xf));
  }
  case op::BufferLoad: // (68, handle, i32 index, i32 offset)
    if (kind == ResourceKind::TypedBuffer)
      return LowerTypedBufferLoad(call, h, call->getArgOperand(2));
    return LowerBufferLoad(call, h,
                           structured ? ir.CreateAdd(ir.CreateMul(call->getArgOperand(2), stride), call->getArgOperand(3))
                                      : call->getArgOperand(2),
                           ExtractedComponents(call));
  case op::BufferStore: // (69, handle, i32 index, i32 offset, v0..v3, i8 mask)
    if (kind == ResourceKind::TypedBuffer)
      return LowerTypedBufferStore(call, h, call->getArgOperand(2));
    return LowerBufferStore(call, h,
                            structured ? ir.CreateAdd(ir.CreateMul(call->getArgOperand(2), stride), call->getArgOperand(3))
                                       : call->getArgOperand(2),
                            4, ConstantU32(call->getArgOperand(8), 0xf));
  case op::GetDimensions: // (72, handle, i32 mipLevel)
    return LowerDimensions(call, h);
  case op::AtomicBinOp:           // (78, handle, i32 op, i32 c0, c1, c2, i32 value) -> old
  case op::AtomicCompareExchange: // (79, handle, i32 c0, c1, c2, i32 compare, i32 value) -> old
    if (!call->getType()->isIntegerTy(32))
      return Unsupported(call); // 64-bit atomics
    if (kind == ResourceKind::RawBuffer || structured) {
      bool cas = opcode == op::AtomicCompareExchange;
      unsigned c = cas ? 2 : 3;
      auto offset = structured ? ir.CreateAdd(ir.CreateMul(call->getArgOperand(c), stride), call->getArgOperand(c + 1))
                               : call->getArgOperand(c);
      auto buffer = Buffer(h);
      if (!buffer) {
        Replace(call, ir.getInt32(0));
        return llvm::Error::success();
      }
      auto ptr = ElementPointer(*buffer, offset, ir.getInt32Ty()); // null out of bounds, as loads and stores
      Replace(call, cas ? ir.CreateExtractValue(ir.CreateAtomicCmpXchg(ptr, call->getArgOperand(5), call->getArgOperand(6), {},
                                                                       llvm::AtomicOrdering::Monotonic,
                                                                       llvm::AtomicOrdering::Monotonic),
                                                {0})
                        : air.CreateAtomicRMW(AtomicOp(call->getArgOperand(2)), ptr, call->getArgOperand(6)));
      return llvm::Error::success();
    }
    return LowerTexture(opcode, call, h); // typed buffers and textures
  case op::TextureLoad:
  case op::TextureStore:
  case op::Sample:
  case op::SampleBias:
  case op::SampleLevel:
  case op::SampleGrad:
  case op::SampleCmp:
  case op::SampleCmpLevelZero:
  case op::TextureGather:
  case op::TextureGatherCmp:
  case op::CalculateLOD:
    return LowerTexture(opcode, call, h);
  case op::Texture2DMSGetSamplePosition: { // (75, srv, i32 index) -> {f32, f32}: D3D's standard positions (air_samplepos)
    llvm::Value *count = ir.getInt32(0); // no texture: (0, 0), as the DXBC samplepos lowering
    if (auto d = Texture(h)) {
      llvm::air::Texture tex;
      tex.kind = d->ResourceKind;
      tex.sample_type = d->SampleType;
      tex.memory_access = d->MemoryAccess;
      count = air.CreateTextureQuery(tex, d->ResourceHandle, Texture::num_samples, ir.getInt32(0));
    }
    auto pos = conv.GetSamplePos(count, call->getArgOperand(2));
    Replace(call, Aggregate(call->getType(), {ir.CreateExtractElement(pos, (uint64_t)0), ir.CreateExtractElement(pos, 1)}));
    return llvm::Error::success();
  }
  default:
    return Unsupported(call);
  }
}

} // namespace dxmt::dxil
