#pragma once
#include "com/com_guid.hpp"
#include "d3d12.h"

// MacNeutron: DXMT's private D3D12 interface to MetalFX's temporal upscaler (XeSS answered by MetalFX). Wine's
// libxess.dll keeps a C copy: the structs stay plain, the IIDs and the method order (after IUnknown's) never change. A
// new method goes in a new interface deriving from the old one, under a new IID.

struct MTL_TEMPORAL_SCALER_D3D12_DESC {
  UINT InputWidth, InputHeight; // the colour, depth and motion textures' size (motion: the output's, with
                                // OutputResolutionMotionVectors)
  UINT OutputWidth, OutputHeight;
  DXGI_FORMAT ColorFormat, DepthFormat, MotionFormat, OutputFormat;
  DXGI_FORMAT ReactiveMaskFormat; // DXGI_FORMAT_UNKNOWN: no reactive mask
  BOOL AutoExposure, OutputResolutionMotionVectors, JitteredMotionVectors;
  FLOAT InputContentMinScale, InputContentMaxScale; // output / input content size, clamped to the device's range
                                                    // (0: the device's bound)
};

struct MTL_TEMPORAL_UPSCALE_D3D12_DESC {
  ID3D12Resource *Color, *Depth, *MotionVector, *Exposure /*may be null*/, *ReactiveMask /*may be null*/, *Output;
  UINT InputContentWidth, InputContentHeight;
  UINT ColorOffsetX, ColorOffsetY, DepthOffsetX, DepthOffsetY, MotionOffsetX, MotionOffsetY,
      ReactiveOffsetX, ReactiveOffsetY, OutputOffsetX, OutputOffsetY;
  FLOAT JitterOffsetX, JitterOffsetY, MotionVectorScaleX, MotionVectorScaleY, PreExposure;
  BOOL Reset, DepthReversed;
};

DEFINE_COM_INTERFACE("5b27acde-a0d8-4677-93c4-eedd17a0ab14", IMTLD3D12TemporalScaler) : public IUnknown {};

// QueryInterface on the device; it fails when the GPU has no MetalFX temporal scaler.
DEFINE_COM_INTERFACE("a8c64ba7-6d54-4833-8cc6-2605c5b454be", IMTLD3D12DeviceExt) : public IUnknown {
  virtual HRESULT STDMETHODCALLTYPE GetTemporalScalerScaleRange(FLOAT *pMin, FLOAT *pMax) = 0;
  // E_INVALIDARG: a size outside the range or an unsupported format; E_OUTOFMEMORY: MetalFX made no upscaler.
  virtual HRESULT STDMETHODCALLTYPE CreateTemporalScaler(const MTL_TEMPORAL_SCALER_D3D12_DESC *pDesc,
                                                         IMTLD3D12TemporalScaler **ppScaler) = 0;
};

// QueryInterface on a command list (DIRECT or COMPUTE). The upscale runs in the list's order; a bad call (E_INVALIDARG:
// a null or mismatched resource, a content size outside the scaler's range) records nothing.
DEFINE_COM_INTERFACE("5690e8a7-51a1-4b37-8bbd-730d0fdb2cec", IMTLD3D12CommandListExt) : public IUnknown {
  virtual HRESULT STDMETHODCALLTYPE TemporalUpscale(IMTLD3D12TemporalScaler *pScaler,
                                                    const MTL_TEMPORAL_UPSCALE_D3D12_DESC *pDesc) = 0;
};
