#pragma once

#ifndef _WIN32

#include <windows.h>

#include "log/log.hpp"
#include "win32_native.h"

inline BOOL ProcessIdToSessionId(DWORD pid, DWORD *id) {
  dxmt::Logger::warn("ProcessIdToSessionId not implemented.");
  *id = 0;
  return FALSE;
}

inline HANDLE GetModuleHandle(LPCSTR lpModuleName) {
  dxmt::Logger::warn("GetModuleHandle not implemented.");
  return nullptr;
}

inline void ExitThread(DWORD dwExitCode) {
  dxmt::Logger::warn("ExitThread not implemented.");
}

#define WM_SIZE 0

inline LONG SendMessage(HWND hWnd, UINT Msg, UINT_PTR wParam, LONG_PTR lParam) {
  dxmt::Logger::warn("SendMessage not implemented.");
  return 0;
}

inline DWORD GetProcessId(HANDLE Process) {
  dxmt::Logger::warn("GetProcessId not implemented.");
  return 0;
}

inline DWORD GetWindowThreadProcessId(HWND hWnd, LPDWORD lpdwProcessId) {
  dxmt::Logger::warn("GetWindowThreadProcessId not implemented.");
  *lpdwProcessId = 0;
  return 0;
}

inline DWORD MAKELONG(WORD a, WORD b) {
  return (((DWORD)a) << 16) | ((DWORD)b);
}

/* winerror.h's, which the native headers lack. */
#define D3D12_ERROR_ADAPTER_NOT_FOUND ((HRESULT)0x887E0001)
#define D3D12_ERROR_DRIVER_VERSION_MISMATCH ((HRESULT)0x887E0002)

#define ARRAYSIZE(a) (sizeof(a)/sizeof(*(a)))

#endif
