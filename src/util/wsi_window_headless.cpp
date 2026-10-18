/*
 * This file is part of DXMT, Copyright (c) 2023 Feifan He
 *
 * Derived from a part of DXVK (originally under zlib License),
 * Copyright (c) 2017 Philip Rebohle
 * Copyright (c) 2019 Joshua Ashton
 *
 * See <https://github.com/doitsujin/dxvk/blob/master/LICENSE>
 */

#include "wsi_window.hpp"
#include "wsi_monitor.hpp"

#include "util_string.hpp"
#include "log/log.hpp"
#include "winemetal.h"

namespace dxmt::wsi {

// MacNeutron: a VM host's window system (WMTSetWsiHooks), from what its guest last said; else headless. The window's
// own changes are the guest's, so the actions only succeed.
static bool hooked() { return WMTGetWsiHooks() != nullptr; }

static bool hookedWindow(HWND hWindow, WMTWsiWindow *pWindow) {
  const WMTWsiHooks *hooks = WMTGetWsiHooks();
  return hooks && hooks->window((uint64_t)(uintptr_t)hWindow, pWindow);
}

void getWindowSize(HWND hWindow, uint32_t *pWidth, uint32_t *pHeight) {
  WMTWsiWindow w = {0, 1024, 1024, 0};
  hookedWindow(hWindow, &w);
  if (pWidth)
    *pWidth = w.width;

  if (pHeight)
    *pHeight = w.height;
}

void resizeWindow(HWND hWindow, DXMTWindowState *pState, uint32_t width,
                  uint32_t height) {
}

bool setWindowMode(HMONITOR hMonitor, HWND hWindow, const WsiMode &mode) {
  return hooked();
}

void saveWindowState(HWND hWindow, DXMTWindowState *pState) {
}

bool enterFullscreenMode(HMONITOR hMonitor, HWND hWindow,
                         DXMTWindowState *pState,
                         [[maybe_unused]] bool modeSwitch) {
  return hooked();
}

bool leaveFullscreenMode(HWND hWindow, DXMTWindowState *pState,
                         bool restoreCoordinates) {
  return hooked();
}

bool restoreDisplayMode(HMONITOR hMonitor) {
  return hooked();
}

HMONITOR getWindowMonitor(HWND hWindow) {
  WMTWsiWindow w = {};
  return hookedWindow(hWindow, &w) ? (HMONITOR)(uintptr_t)w.monitor : 0;
}

bool isWindow(HWND hWindow) {
  WMTWsiWindow w = {};
  return hookedWindow(hWindow, &w) ? (w.flags & 4) != 0 : true;
}

void updateFullscreenWindow(HMONITOR hMonitor, HWND hWindow,
                            bool forceTopmost) {
}

bool isForeground(HWND hWindow) {
  WMTWsiWindow w = {};
  return hookedWindow(hWindow, &w) ? (w.flags & 2) != 0 : true;
}

bool isMinimized(HWND hWindow) {
  WMTWsiWindow w = {};
  return hookedWindow(hWindow, &w) && (w.flags & 1) != 0;
}

bool isProcessForeground() { return true; }

void minimizeWindow(HWND hWindow) {}

} // namespace dxmt::wsi
