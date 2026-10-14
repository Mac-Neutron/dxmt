/*
 * MacNeutron (DLSS answered by MetalFX): NGX's Direct3D 12 entry points, on DXMT's private D3D12 interface to
 * MetalFX's temporal upscaler (IMTLD3D12DeviceExt, IMTLD3D12CommandListExt: the one Wine's libxess.dll answers XeSS
 * through). The game's NGX SDK loads this library as the NGX core (DXMT_ENABLE_NVEXT=1 points it here) and calls
 * these exports; nothing of NVIDIA's runs.
 *
 * A DLSS feature's upscaler is made at its first evaluation, from the formats of the textures the game really passes
 * (creation names only sizes and flags), and made again when they change. NGX's conventions are MetalFX's: motion
 * vectors point from the current frame to the previous one, in pixels after MV.Scale; the jitter is the sample's
 * offset in input pixels. The first evaluation logs what the game passed (formats, sizes, offsets, jitter, scales,
 * flags) to the game's log, once, before any pixel is trusted.
 */

#include "com/com_pointer.hpp"
#include "nvngx.hpp"
#include "nvngx_feature.hpp"
#include "nvngx_parameter.hpp"
#include "../d3d12/d3d12_interfaces.hpp"
#include <algorithm>
#include <atomic>
#include <cstdio>
#include <cstring>
#include <memory>
#include <mutex>

namespace dxmt {

namespace {

std::atomic<unsigned> g_next_id{1};

/* A message for the game's log (stderr: whatever WINEDEBUG says), at most once per `flag`. */
template <typename... Args>
void
Once(std::atomic<bool> &flag, const char *format, Args... args) {
  if (flag.exchange(true))
    return;
  fprintf(stderr, "macneutron-dlss: ");
  fprintf(stderr, format, args...);
  fputc('\n', stderr);
}

/* An ID3D12Resource parameter, whichever of NGX's setters the game used for it. */
ID3D12Resource *
Resource(ParametersImpl *params, const char *name) {
  ID3D12Resource *resource = nullptr;
  if (params->Get(name, &resource) == NVNGX_RESULT_OK && resource)
    return resource;
  void *any = nullptr;
  if (params->Get(name, &any) == NVNGX_RESULT_OK)
    return static_cast<ID3D12Resource *>(any);
  return nullptr;
}

unsigned
Uint(ParametersImpl *params, const char *name, unsigned fallback = 0) {
  unsigned value = 0;
  return params->Get(name, &value) == NVNGX_RESULT_OK ? value : fallback;
}

float
Float(ParametersImpl *params, const char *name, float fallback) {
  float value = 0;
  return params->Get(name, &value) == NVNGX_RESULT_OK ? value : fallback;
}

/* The width and height of `resource` past (x, y): 0 when (x, y) is outside it. */
void
Extent(const D3D12_RESOURCE_DESC &desc, UINT x, UINT y, UINT &w, UINT &h) {
  w = desc.Width > x ? (UINT)desc.Width - x : 0;
  h = desc.Height > y ? desc.Height - y : 0;
}

struct DLSSFeatureD3D12 : DLSSFeature {
  std::mutex lock;
  Com<IMTLD3D12DeviceExt> ext;
  Com<IMTLD3D12TemporalScaler> scaler;
  MTL_TEMPORAL_SCALER_D3D12_DESC scaler_desc{};
  float min_scale = 1.0f, max_scale = 3.0f;
};

/* Every call's refusals, each logged once per process. */
std::atomic<bool> g_logged_inputs, g_logged_missing, g_logged_small, g_logged_scaler, g_logged_foreign,
    g_logged_refused, g_logged_feature, g_logged_noext;

NVNGX_RESULT
InitWith(ID3D12Device *device) {
  if (!device)
    return NVNGX_RESULT_INVALID_PARAMETER;
  Com<IMTLD3D12DeviceExt> ext;
  if (FAILED(device->QueryInterface(IID_PPV_ARGS(&ext)))) {
    Once(g_logged_noext, "%s", "NGX initialised on a device without MetalFX's temporal upscaler: DLSS unavailable");
    return NVNGX_RESULT_FEATURE_NOT_SUPPORTED;
  }
  return NVNGX_RESULT_OK;
}

NVNGX_RESULT
Evaluate(DLSSFeatureD3D12 *dlss, ID3D12GraphicsCommandList *list, ParametersImpl *params) {
  auto color = Resource(params, NVNGX_Parameter_Color), output = Resource(params, NVNGX_Parameter_Output),
       depth = Resource(params, NVNGX_Parameter_Depth), motion = Resource(params, NVNGX_Parameter_MotionVectors);
  auto exposure = (dlss->flag & NVNGX_DLSS_FLAG_AUTO_EXPOSURE) ? nullptr : Resource(params, NVNGX_Parameter_ExposureTexture);
  if (!list || !color || !output || !depth || !motion) {
    Once(g_logged_missing, "EvaluateFeature without a texture it needs: list %p colour %p output %p depth %p motion %p",
         (void *)list, (void *)color, (void *)output, (void *)depth, (void *)motion);
    return NVNGX_RESULT_INVALID_PARAMETER;
  }
  UINT cx = Uint(params, "DLSS.Input.Color.Subrect.Base.X"), cy = Uint(params, "DLSS.Input.Color.Subrect.Base.Y"),
       dx = Uint(params, "DLSS.Input.Depth.Subrect.Base.X"), dy = Uint(params, "DLSS.Input.Depth.Subrect.Base.Y"),
       mx = Uint(params, "DLSS.Input.MV.SubrectBase.X"), my = Uint(params, "DLSS.Input.MV.SubrectBase.Y"),
       ox = Uint(params, "DLSS.Output.Subrect.Base.X"), oy = Uint(params, "DLSS.Output.Subrect.Base.Y");
  UINT iw = Uint(params, NVNGX_Parameter_DLSS_Render_Subrect_Dimensions_Width, dlss->width),
       ih = Uint(params, NVNGX_Parameter_DLSS_Render_Subrect_Dimensions_Height, dlss->height);
  if (!iw || !ih) {
    iw = dlss->width;
    ih = dlss->height;
  }
  auto cd = color->GetDesc(), dd = depth->GetDesc(), md = motion->GetDesc(), od = output->GetDesc();
  UINT cw, ch, dw, dh, mw, mh, ow, oh;
  Extent(cd, cx, cy, cw, ch);
  Extent(dd, dx, dy, dw, dh);
  Extent(md, mx, my, mw, mh);
  Extent(od, ox, oy, ow, oh);

  float jitter_x = Float(params, NVNGX_Parameter_Jitter_Offset_X, 0.0f),
        jitter_y = Float(params, NVNGX_Parameter_Jitter_Offset_Y, 0.0f),
        scale_x = Float(params, NVNGX_Parameter_MV_Scale_X, 1.0f), scale_y = Float(params, NVNGX_Parameter_MV_Scale_Y, 1.0f),
        pre_exposure = Float(params, NVNGX_Parameter_DLSS_Pre_Exposure, 1.0f);
  int reset = 0;
  params->Get(NVNGX_Parameter_Reset, &reset);
  if (!g_logged_inputs.exchange(true))
    fprintf(stderr,
            "macneutron-dlss: flags %#x quality %d; render %ux%u of %ux%u, output %ux%u; formats colour %u depth %u "
            "motion %u output %u; sizes colour %llux%u depth %llux%u motion %llux%u output %llux%u; bases colour %u,%u "
            "depth %u,%u motion %u,%u output %u,%u; exposure texture %p; jitter %f,%f; motion scale %f,%f; "
            "pre-exposure %f; reset %d\n",
            (unsigned)dlss->flag, dlss->quality, iw, ih, dlss->width, dlss->height, dlss->target_width,
            dlss->target_height, (unsigned)cd.Format, (unsigned)dd.Format, (unsigned)md.Format, (unsigned)od.Format,
            (unsigned long long)cd.Width, cd.Height, (unsigned long long)dd.Width, dd.Height,
            (unsigned long long)md.Width, md.Height, (unsigned long long)od.Width, od.Height, cx, cy, dx, dy, mx, my,
            ox, oy, (void *)exposure, jitter_x, jitter_y, scale_x, scale_y, pre_exposure, reset);

  /* The upscaler for these textures: the one there is if they hold the region it reads, else one reading as much of
   * them as they share (no more than the output), so that a game's dynamic resolution moves only the content size. */
  MTL_TEMPORAL_SCALER_D3D12_DESC want = {};
  want.OutputWidth = dlss->target_width;
  want.OutputHeight = dlss->target_height;
  want.ColorFormat = cd.Format;
  want.DepthFormat = dd.Format;
  want.MotionFormat = md.Format;
  want.OutputFormat = od.Format;
  want.ReactiveMaskFormat = DXGI_FORMAT_UNKNOWN;
  want.AutoExposure = !!(dlss->flag & NVNGX_DLSS_FLAG_AUTO_EXPOSURE);
  want.OutputResolutionMotionVectors = !(dlss->flag & NVNGX_DLSS_FLAG_MV_LOWRES);
  want.JitteredMotionVectors = !!(dlss->flag & NVNGX_DLSS_FLAG_MV_JITTERED);
  if (ow < want.OutputWidth || oh < want.OutputHeight) {
    Once(g_logged_small, "EvaluateFeature: output %llux%u at %u,%u holds no %ux%u", (unsigned long long)od.Width,
         od.Height, ox, oy, want.OutputWidth, want.OutputHeight);
    return NVNGX_RESULT_INVALID_PARAMETER;
  }
  if (want.OutputResolutionMotionVectors) {
    if (mw < want.OutputWidth || mh < want.OutputHeight)
      mw = mh = 0;
    else
      mw = mh = ~0u; /* not an input-size texture */
  }
  UINT tw = std::min({cw, dw, mw, want.OutputWidth}), th = std::min({ch, dh, mh, want.OutputHeight});
  if (tw < iw || th < ih) {
    Once(g_logged_small, "EvaluateFeature: render %ux%u at bases (%u,%u) (%u,%u) (%u,%u) isn't in its textures", iw, ih,
         cx, cy, dx, dy, mx, my);
    return NVNGX_RESULT_INVALID_PARAMETER;
  }
  want.InputWidth = dlss->scaler_desc.InputWidth;
  want.InputHeight = dlss->scaler_desc.InputHeight;
  if (!dlss->scaler || memcmp(&want, &dlss->scaler_desc, sizeof(want)) || tw < want.InputWidth ||
      th < want.InputHeight || iw > want.InputWidth || ih > want.InputHeight) {
    dlss->scaler = nullptr;
    want.InputWidth = tw;
    want.InputHeight = th;
    HRESULT hr = dlss->ext->CreateTemporalScaler(&want, &dlss->scaler);
    if (FAILED(hr)) {
      dlss->scaler = nullptr;
      Once(g_logged_scaler,
           "EvaluateFeature: no MetalFX upscaler for %ux%u to %ux%u, formats colour %u depth %u motion %u output %u: "
           "%#lx",
           tw, th, want.OutputWidth, want.OutputHeight, (unsigned)want.ColorFormat, (unsigned)want.DepthFormat,
           (unsigned)want.MotionFormat, (unsigned)want.OutputFormat, (unsigned long)hr);
      return NVNGX_RESULT_FAIL;
    }
    dlss->scaler_desc = want;
  }
  Com<IMTLD3D12CommandListExt> list_ext;
  if (FAILED(list->QueryInterface(IID_PPV_ARGS(&list_ext)))) {
    Once(g_logged_foreign, "EvaluateFeature: command list %p isn't DXMT's", (void *)list);
    return NVNGX_RESULT_INVALID_PARAMETER;
  }
  MTL_TEMPORAL_UPSCALE_D3D12_DESC d = {};
  d.Color = color;
  d.Depth = depth;
  d.MotionVector = motion;
  d.Exposure = exposure;
  d.Output = output;
  d.InputContentWidth = iw;
  d.InputContentHeight = ih;
  d.ColorOffsetX = cx;
  d.ColorOffsetY = cy;
  d.DepthOffsetX = dx;
  d.DepthOffsetY = dy;
  d.MotionOffsetX = mx;
  d.MotionOffsetY = my;
  d.OutputOffsetX = ox;
  d.OutputOffsetY = oy;
  d.JitterOffsetX = jitter_x;
  d.JitterOffsetY = jitter_y;
  d.MotionVectorScaleX = scale_x;
  d.MotionVectorScaleY = scale_y;
  d.PreExposure = pre_exposure > 0.0f ? pre_exposure : 1.0f;
  d.Reset = reset != 0;
  d.DepthReversed = !!(dlss->flag & NVNGX_DLSS_FLAG_DEPTH_INVERTED);
  HRESULT hr = list_ext->TemporalUpscale(dlss->scaler.ptr(), &d);
  if (FAILED(hr)) {
    Once(g_logged_refused, "EvaluateFeature: DXMT refused the upscale (%ux%u to %ux%u): %#lx", iw, ih,
         want.OutputWidth, want.OutputHeight, (unsigned long)hr);
    return NVNGX_RESULT_FAIL;
  }
  return NVNGX_RESULT_OK;
}

} // namespace

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_Init_Ext(
    unsigned long long id, const wchar_t *path, ID3D12Device *device, unsigned int sdk_version, const void *feature_info
) {
  return InitWith(device);
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_Init(
    unsigned long long id, const wchar_t *path, ID3D12Device *device, const void *feature_info, unsigned int sdk_version
) {
  return InitWith(device);
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_Init_ProjectID(
    const char *project, unsigned int engine_type, const char *engine_version, const wchar_t *path,
    ID3D12Device *device, unsigned int sdk_version, const void *feature_info
) {
  return InitWith(device);
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_Init_with_ProjectID(
    const char *project, unsigned int engine_type, const char *engine_version, const wchar_t *path,
    ID3D12Device *device, const void *feature_info, unsigned int sdk_version
) {
  return InitWith(device);
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_Shutdown() {
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_Shutdown1(ID3D12Device *device) {
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_GetCapabilityParameters(NVNGXParameter **out_params) {
  if (!out_params)
    return NVNGX_RESULT_INVALID_PARAMETER;
  *out_params = NewCapabilityParameters();
  return NVNGX_RESULT_OK;
}

/* The deprecated map the SDK manages: one per process, never freed by the game. */
NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_GetParameters(NVNGXParameter **out_params) {
  if (!out_params)
    return NVNGX_RESULT_INVALID_PARAMETER;
  static NVNGXParameter *shared = NewCapabilityParameters();
  *out_params = shared;
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_AllocateParameters(NVNGXParameter **out_params) {
  if (!out_params)
    return NVNGX_RESULT_INVALID_PARAMETER;
  *out_params = new ParametersImpl();
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_PopulateParameters_Impl(NVNGXParameter *params) {
  if (!params)
    return NVNGX_RESULT_INVALID_PARAMETER;
  PopulateCapabilityParameters(params);
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_DestroyParameters(NVNGXParameter *params) {
  delete static_cast<ParametersImpl *>(params);
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_GetScratchBufferSize(unsigned int feature, const NVNGXParameter *params, size_t *out_size) {
  if (out_size)
    *out_size = 0;
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_CreateFeature(
    ID3D12GraphicsCommandList *list, unsigned int feature, NVNGXParameter *params, unsigned int **out_handle
) {
  if (!params || !out_handle)
    return NVNGX_RESULT_INVALID_PARAMETER;
  if (feature != NVNGX_FEATURE_SUPERSAMPLING) {
    Once(g_logged_feature, "CreateFeature: feature %u isn't DLSS super resolution: refused", feature);
    return NVNGX_RESULT_FEATURE_NOT_SUPPORTED;
  }
  auto parameters = static_cast<ParametersImpl *>(params);
  auto dlss = std::make_unique<DLSSFeatureD3D12>();
  dlss->feature = NVNGX_FEATURE_SUPERSAMPLING;
  dlss->api = 12;
  dlss->handle = g_next_id++;
  ID3D12Device *device = nullptr;
  if (!list || FAILED(list->GetDevice(IID_PPV_ARGS(&device))))
    return NVNGX_RESULT_INVALID_PARAMETER;
  HRESULT hr = device->QueryInterface(IID_PPV_ARGS(&dlss->ext));
  device->Release();
  if (FAILED(hr))
    return NVNGX_RESULT_FEATURE_NOT_SUPPORTED;
  dlss->ext->GetTemporalScalerScaleRange(&dlss->min_scale, &dlss->max_scale);
  if (NVNGX_FAILED(parameters->Get(NVNGX_Parameter_Width, &dlss->width)) ||
      NVNGX_FAILED(parameters->Get(NVNGX_Parameter_Height, &dlss->height)) ||
      NVNGX_FAILED(parameters->Get(NVNGX_Parameter_OutWidth, &dlss->target_width)) ||
      NVNGX_FAILED(parameters->Get(NVNGX_Parameter_OutHeight, &dlss->target_height)) || !dlss->width ||
      !dlss->height || !dlss->target_width || !dlss->target_height)
    return NVNGX_RESULT_INVALID_PARAMETER;
  if (NVNGX_FAILED(parameters->Get(NVNGX_Parameter_PerfQualityValue, &dlss->quality)))
    dlss->quality = 0;
  if (NVNGX_FAILED(parameters->Get(NVNGX_Parameter_DLSS_Feature_Create_Flags, &dlss->flag)))
    dlss->flag = 0;
  if (NVNGX_FAILED(parameters->Get("DLSS.Enable.Output.Subrects", &dlss->enable_output_subrects)))
    dlss->enable_output_subrects = 0;
  /* MetalFX's range of output / input; a DLAA (1.0x) or 3x request fits it on Apple's GPUs. */
  float sx = (float)dlss->target_width / dlss->width, sy = (float)dlss->target_height / dlss->height;
  if (sx > dlss->max_scale + 0.01f || sy > dlss->max_scale + 0.01f || sx < 0.99f || sy < 0.99f) {
    fprintf(stderr, "macneutron-dlss: CreateFeature: %ux%u to %ux%u is outside MetalFX's %.2fx-%.2fx\n", dlss->width,
            dlss->height, dlss->target_width, dlss->target_height, dlss->min_scale, dlss->max_scale);
    return NVNGX_RESULT_INVALID_PARAMETER;
  }
  fprintf(stderr, "macneutron-dlss: DLSS %ux%u to %ux%u (quality %d, flags %#x) answered by MetalFX\n", dlss->width,
          dlss->height, dlss->target_width, dlss->target_height, dlss->quality, (unsigned)dlss->flag);
  *out_handle = &dlss.release()->handle;
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_ReleaseFeature(unsigned int *handle) {
  if (!handle)
    return NVNGX_RESULT_OK;
  auto common = static_cast<CommonFeature *>((void *)handle);
  if (common->feature == NVNGX_FEATURE_SUPERSAMPLING && common->api == 12)
    delete static_cast<DLSSFeatureD3D12 *>(common);
  return NVNGX_RESULT_OK;
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_EvaluateFeature(
    ID3D12GraphicsCommandList *list, const unsigned int *handle, NVNGXParameter *params, void *callback
) {
  if (!handle || !params)
    return NVNGX_RESULT_INVALID_PARAMETER;
  auto common = static_cast<CommonFeature *>((void *)handle);
  if (common->feature != NVNGX_FEATURE_SUPERSAMPLING || common->api != 12)
    return NVNGX_RESULT_FEATURE_NOT_SUPPORTED;
  auto dlss = static_cast<DLSSFeatureD3D12 *>(common);
  std::lock_guard<std::mutex> guard(dlss->lock);
  return Evaluate(dlss, list, static_cast<ParametersImpl *>(params));
}

NVNGX_API NVNGX_RESULT
NVSDK_NGX_D3D12_GetFeatureRequirements(
    IDXGIAdapter *adapter, const NVNGX_FeatureDiscoveryInfo *discovery_info, NVNGX_FeatureRequirement *requirement
) {
  if (!requirement || !discovery_info)
    return NVNGX_RESULT_INVALID_PARAMETER;
  if (discovery_info->FeatureID == NVNGX_FEATURE_SUPERSAMPLING) {
    requirement->FeatureSupported = NVNGX_FEATURE_SUPPORT_RESULT_SUPPORTED;
    requirement->MinHWArchitecture = 0;
    strcpy_s(requirement->MinOSVersion, "10.0.16299.0");
    return NVNGX_RESULT_OK;
  }
  requirement->FeatureSupported = NVNGX_FEATURE_SUPPORT_RESULT_UNSUPPORTED;
  return NVNGX_RESULT_FEATURE_NOT_SUPPORTED;
}

}; // namespace dxmt
