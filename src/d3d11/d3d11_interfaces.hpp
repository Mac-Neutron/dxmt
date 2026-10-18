#pragma once
#include "com/com_guid.hpp"
#include "d3d11.h"

struct MTL_TEMPORAL_UPSCALE_D3D11_DESC {
  UINT InputContentWidth; // can be 0, which means full width
  UINT InputContentHeight; // can be 0, which means full height
  BOOL AutoExposure;
  BOOL InReset;
  BOOL DepthReversed;
  BOOL MotionVectorInDisplayRes;
  ID3D11Resource *Color;
  ID3D11Resource *Depth;
  ID3D11Resource *MotionVector;
  ID3D11Resource *Output;
  FLOAT MotionVectorScaleX;
  FLOAT MotionVectorScaleY;
  FLOAT PreExposure;
  ID3D11Resource *ExposureTexture;
  FLOAT JitterOffsetX;
  FLOAT JitterOffsetY;
};

DEFINE_COM_INTERFACE("43ace3ce-1956-448b-a4eb-aee68bdeb283",
                     IMTLD3D11ContextExt)
    : public IUnknown{
   virtual void STDMETHODCALLTYPE TemporalUpscale(const MTL_TEMPORAL_UPSCALE_D3D11_DESC *pDesc) = 0;
   virtual void STDMETHODCALLTYPE BeginUAVOverlap() = 0;
   virtual void STDMETHODCALLTYPE EndUAVOverlap() = 0;
};

typedef enum MTL_FEATURE {
  MTL_FEATURE_METALFX_TEMPORAL_SCALER = 0,
} MTL_FEATURE;

DEFINE_COM_INTERFACE("19a8e35a-38be-418f-94e3-9f7323936870", IMTLD3D11ContextExt1) : public IMTLD3D11ContextExt {
  virtual HRESULT STDMETHODCALLTYPE CheckFeatureSupport(
      MTL_FEATURE Feature, void *pFeatureSupportData, UINT FeatureSupportDataSize
  ) = 0;
};

// MacNeutron: a VM host's guest answers a dynamic resource's WRITE_DISCARD and WRITE_NO_OVERWRITE maps itself, in
// pieces of its memory the immediate context lends it. GetPieces lends count pieces of the subresource's memory not
// in use (the recycled ones first, then new ones), each the caller's until UsePiece makes it the subresource's memory
// (WRITE_DISCARD's rename, and a dynamic texture's Unmap upload), which returns the one it replaces to the recycling;
// count 0 answers the current piece. E_NOTIMPL: a resource whose maps the context makes itself (not dynamic, a
// linear dynamic texture), or a deferred context. A piece's id means nothing but to UsePiece, which takes only the
// ids it lent out or the current one's.
struct MTLD3D11Piece {
  UINT64 id;
  void *cpu;
  UINT row_pitch;
  UINT depth_pitch;
};

DEFINE_COM_INTERFACE("a5253c74-3ff9-497f-b184-f137c5a7b928", IMTLD3D11ContextExt2) : public IMTLD3D11ContextExt1 {
  virtual HRESULT STDMETHODCALLTYPE GetPieces(ID3D11Resource *resource, UINT subresource, UINT count,
                                              MTLD3D11Piece *pieces) = 0;
  virtual void STDMETHODCALLTYPE UsePiece(ID3D11Resource *resource, UINT subresource, UINT64 piece) = 0;
};

DEFINE_COM_INTERFACE("efc77ae6-2179-4c0a-b844-7661ca0dcde7", IMTLD3D11DeviceExt)
    : public IUnknown {
  virtual void STDMETHODCALLTYPE SetShaderExtensionSlot(UINT Slot) = 0;
};
