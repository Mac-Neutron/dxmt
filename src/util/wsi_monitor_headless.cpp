/*
 * This file is part of DXMT, Copyright (c) 2023 Feifan He
 *
 * Derived from a part of DXVK (originally under zlib License),
 * Copyright (c) 2017 Philip Rebohle
 * Copyright (c) 2019 Joshua Ashton
 *
 * See <https://github.com/doitsujin/dxvk/blob/master/LICENSE>
 */

#include "wsi_monitor.hpp"
#include "winemetal.h"

namespace dxmt::wsi {

// MacNeutron: a VM host's displays (WMTSetWsiHooks), its guest's; else none.
static bool hookedDisplay(uint32_t index, WMTWsiDisplay *pDisplay) {
  const WMTWsiHooks *hooks = WMTGetWsiHooks();
  return hooks && hooks->display(index, pDisplay);
}

static bool displayOf(HMONITOR hMonitor, WMTWsiDisplay *pDisplay) {
  for (uint32_t i = 0; hookedDisplay(i, pDisplay); i++)
    if (pDisplay->monitor == (uint64_t)(uintptr_t)hMonitor)
      return true;
  return false;
}

HMONITOR getDefaultMonitor() {
  WMTWsiDisplay d;
  return hookedDisplay(0, &d) ? (HMONITOR)(uintptr_t)d.monitor : nullptr;
}

HMONITOR enumMonitors(uint32_t index) {
  WMTWsiDisplay d;
  return hookedDisplay(index, &d) ? (HMONITOR)(uintptr_t)d.monitor : nullptr;
}

bool getDisplayName(HMONITOR hMonitor, WCHAR (&Name)[32]) {
  WMTWsiDisplay d;
  if (!displayOf(hMonitor, &d))
    return false;
  for (uint32_t i = 0; i < 32; i++)
    Name[i] = d.name[i]; // UTF-16 to the native WCHAR: a display's name is ASCII
  Name[31] = 0;
  return true;
}

bool getDesktopCoordinates(HMONITOR hMonitor, RECT *pRect) {
  WMTWsiDisplay d;
  if (!displayOf(hMonitor, &d))
    return false;
  *pRect = {d.left, d.top, d.right, d.bottom};
  return true;
}

// The current mode is the hooks' first, EnumDisplaySettings' list after it; the registry's is the current one.
static inline bool retrieveDisplayMode(HMONITOR hMonitor, DWORD modeNumber,
                                       WsiMode *pMode) {
  const WMTWsiHooks *hooks = WMTGetWsiHooks();
  uint32_t index = modeNumber == ENUM_CURRENT_SETTINGS || modeNumber == ENUM_REGISTRY_SETTINGS ? 0 : modeNumber + 1;
  WMTWsiMode m;
  if (!hooks || !hooks->mode((uint64_t)(uintptr_t)hMonitor, index, &m))
    return false;
  *pMode = {m.width, m.height, {m.refresh_num, m.refresh_den}, 32, false};
  return true;
}

bool getDisplayMode(HMONITOR hMonitor, uint32_t modeNumber, WsiMode *pMode) {
  return retrieveDisplayMode(hMonitor, modeNumber, pMode);
}

bool getCurrentDisplayMode(HMONITOR hMonitor, WsiMode *pMode) {
  return retrieveDisplayMode(hMonitor, ENUM_CURRENT_SETTINGS, pMode);
}

bool getDesktopDisplayMode(HMONITOR hMonitor, WsiMode *pMode) {
  return retrieveDisplayMode(hMonitor, ENUM_REGISTRY_SETTINGS, pMode);
}

} // namespace dxmt::wsi
