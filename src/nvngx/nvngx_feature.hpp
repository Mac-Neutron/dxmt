#pragma once
#include "nvngx.hpp"
#include <cstdint>

namespace dxmt {

struct CommonFeature {
  unsigned int handle;
  NVNGX_FEATURE feature;
  unsigned int api = 11; // MacNeutron: 12 for a feature NVSDK_NGX_D3D12_CreateFeature made (nvngx_d3d12.cpp)
};

struct DLSSFeature : CommonFeature {
  uint32_t width;
  uint32_t height;
  uint32_t target_width;
  uint32_t target_height;
  int quality;
  int flag;
  int enable_output_subrects;
};

// MacNeutron: the capability map both APIs hand out (nvngx.cpp): a new one, or `params` filled with it.
NVNGXParameter *NewCapabilityParameters();
void PopulateCapabilityParameters(NVNGXParameter *params);

}