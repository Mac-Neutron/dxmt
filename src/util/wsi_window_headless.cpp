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
// own changes are the guest's: each action goes to the hooks for the guest to make, and succeeds here.
static bool hooked() { return WMTGetWsiHooks() != nullptr; }

static bool act(uint32_t kind, HWND hWindow, HMONITOR hMonitor = nullptr, uint32_t arg = 0,
                const WsiMode &mode = {}) {
  const WMTWsiHooks *hooks = WMTGetWsiHooks();
  if (!hooks)
    return false;
  WMTWsiAction a = {kind, arg, (uint64_t)(uintptr_t)hWindow, (uint64_t)(uintptr_t)hMonitor, mode.width, mode.height,
                    mode.refreshRate.numerator, mode.refreshRate.denominator, mode.bitsPerPixel, mode.interlaced};
  hooks->action(&a);
  return true;
}

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
  act(WMTWsiResizeWindow, hWindow, nullptr, 0, WsiMode{width, height, {0, 0}, 0, false});
}

bool setWindowMode(HMONITOR hMonitor, HWND hWindow, const WsiMode &mode) {
  return act(WMTWsiSetWindowMode, hWindow, hMonitor, 0, mode);
}

void saveWindowState(HWND hWindow, DXMTWindowState *pState) {
  act(WMTWsiSaveWindowState, hWindow);
}

bool enterFullscreenMode(HMONITOR hMonitor, HWND hWindow,
                         DXMTWindowState *pState,
                         [[maybe_unused]] bool modeSwitch) {
  return act(WMTWsiEnterFullscreenMode, hWindow, hMonitor, modeSwitch);
}

bool leaveFullscreenMode(HWND hWindow, DXMTWindowState *pState,
                         bool restoreCoordinates) {
  return act(WMTWsiLeaveFullscreenMode, hWindow, nullptr, restoreCoordinates);
}

bool restoreDisplayMode(HMONITOR hMonitor) {
  return act(WMTWsiRestoreDisplayMode, nullptr, hMonitor);
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
  act(WMTWsiUpdateFullscreenWindow, hWindow, hMonitor, forceTopmost);
}

bool isForeground(HWND hWindow) {
  WMTWsiWindow w = {};
  return hookedWindow(hWindow, &w) ? (w.flags & 2) != 0 : true;
}

bool isMinimized(HWND hWindow) {
  WMTWsiWindow w = {};
  return hookedWindow(hWindow, &w) && (w.flags & 1) != 0;
}

bool isProcessForeground() {
  WMTWsiWindow w = {};
  return hookedWindow(nullptr, &w) ? (w.flags & 8) != 0 : true;
}

void minimizeWindow(HWND hWindow) { act(WMTWsiMinimizeWindow, hWindow); }

} // namespace dxmt::wsi
