#pragma once

/*
 * The Win32 calls DXMT makes, for the native build (macOS, no Wine): real handles, not stubs. A HANDLE is a pointer to
 * a mn_win32_handle that says what it is, and every function is static inline: any binary that includes this header
 * acts on another's handles (a program's event that DXMT's fence sets). Events and semaphores are a mutex, a condition
 * variable and a count; files are file descriptors. Native WCHAR is wchar_t (UTF-32): paths go to the file system as
 * UTF-8. GetCurrentProcess and GetCurrentThread are Windows' pseudo-handles, -1 and -2.
 */

#ifndef _WIN32

#include <windows.h>

#include "win32_native_event.h"

#include <cerrno>
#include <cstdlib>
#include <cstring>
#include <ctime>
#include <dlfcn.h>
#include <fcntl.h>
#include <libproc.h>
#include <pthread.h>
#include <stdio.h>
#include <string>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

typedef void *HLOCAL;
typedef SECURITY_ATTRIBUTES *LPSECURITY_ATTRIBUTES;
typedef LONG *PLONG;
typedef LARGE_INTEGER *PLARGE_INTEGER;
typedef struct OVERLAPPED OVERLAPPED, *LPOVERLAPPED; /* never used: every call here is synchronous */
typedef void (*FARPROC)(void);

typedef struct SYSTEMTIME {
  WORD wYear, wMonth, wDayOfWeek, wDay, wHour, wMinute, wSecond, wMilliseconds;
} SYSTEMTIME, *LPSYSTEMTIME;

#define INFINITE 0xffffffffu
#define GENERIC_READ 0x80000000u
#define GENERIC_WRITE 0x40000000u
#define FILE_APPEND_DATA 0x00000004u
#define FILE_SHARE_READ 0x00000001u
#define FILE_SHARE_WRITE 0x00000002u
#define FILE_SHARE_DELETE 0x00000004u
#define CREATE_NEW 1
#define CREATE_ALWAYS 2
#define OPEN_EXISTING 3
#define OPEN_ALWAYS 4
#define TRUNCATE_EXISTING 5
#define FILE_ATTRIBUTE_DIRECTORY 0x00000010u
#define FILE_ATTRIBUTE_NORMAL 0x00000080u
#define INVALID_FILE_ATTRIBUTES 0xffffffffu
#define INVALID_SET_FILE_POINTER 0xffffffffu
#define FILE_BEGIN 0
#define FILE_CURRENT 1
#define FILE_END 2
#define MOVEFILE_REPLACE_EXISTING 0x00000001u
#define ERROR_SUCCESS 0
#define ERROR_FILE_NOT_FOUND 2
#define ERROR_PATH_NOT_FOUND 3
#define ERROR_ACCESS_DENIED 5
#define ERROR_INVALID_HANDLE 6
#define ERROR_NOT_ENOUGH_MEMORY 8
#define ERROR_FILE_EXISTS 80
#define ERROR_INVALID_PARAMETER 87
#define ERROR_ALREADY_EXISTS 183
#define ERROR_TOO_MANY_POSTS 298
#ifndef THREAD_PRIORITY_TIME_CRITICAL
#define THREAD_PRIORITY_TIME_CRITICAL 15
#endif
#define THREAD_QUERY_LIMITED_INFORMATION 0x0800
#define DLL_PROCESS_DETACH 0
#define DLL_PROCESS_ATTACH 1
#define DLL_THREAD_ATTACH 2
#define DLL_THREAD_DETACH 3
#define VK_F9 0x78

static inline DWORD &mn_win32_error() {
  static thread_local DWORD error = ERROR_SUCCESS;
  return error;
}
static inline DWORD GetLastError() { return mn_win32_error(); }
static inline void SetLastError(DWORD e) { mn_win32_error() = e; }

static inline DWORD mn_win32_from_errno(int e) {
  switch (e) {
  case ENOENT: return ERROR_FILE_NOT_FOUND;
  case ENOTDIR: return ERROR_PATH_NOT_FOUND;
  case EACCES: case EPERM: return ERROR_ACCESS_DENIED;
  case EEXIST: return ERROR_ALREADY_EXISTS;
  case ENOMEM: return ERROR_NOT_ENOUGH_MEMORY;
  case EBADF: return ERROR_INVALID_HANDLE;
  default: return ERROR_INVALID_PARAMETER;
  }
}

static inline mn_win32_handle *mn_win32_get(HANDLE h, uint32_t kind) {
  if (!h || h == INVALID_HANDLE_VALUE || h == (HANDLE)(intptr_t)-2) return nullptr;
  mn_win32_handle *o = (mn_win32_handle *)h;
  return o->kind == kind ? o : nullptr;
}

static inline mn_win32_handle *mn_win32_new(uint32_t kind) {
  mn_win32_handle *o = new mn_win32_handle();
  o->kind = kind;
  o->refs = 1;
  o->fd = -1;
  if (kind != MN_WIN32_FILE) {
    pthread_mutex_init(&o->m, nullptr);
    pthread_cond_init(&o->c, nullptr);
  }
  return o;
}

/* Native paths: the wide string (UTF-32) as UTF-8. */
static inline std::string mn_win32_utf8(LPCWSTR s) {
  std::string out;
  for (; s && *s; s++) {
    uint32_t c = (uint32_t)*s;
    if (c < 0x80) {
      out += (char)c;
    } else if (c < 0x800) {
      out += (char)(0xc0 | c >> 6), out += (char)(0x80 | (c & 0x3f));
    } else if (c < 0x10000) {
      out += (char)(0xe0 | c >> 12), out += (char)(0x80 | (c >> 6 & 0x3f)), out += (char)(0x80 | (c & 0x3f));
    } else {
      out += (char)(0xf0 | c >> 18), out += (char)(0x80 | (c >> 12 & 0x3f));
      out += (char)(0x80 | (c >> 6 & 0x3f)), out += (char)(0x80 | (c & 0x3f));
    }
  }
  return out;
}

/* Events and semaphores. */

static inline HANDLE CreateEventW(LPSECURITY_ATTRIBUTES, BOOL manual, BOOL initial, LPCWSTR) {
  mn_win32_handle *o = mn_win32_new(MN_WIN32_EVENT);
  o->manual = manual, o->count = initial ? 1 : 0, o->max = 1;
  return o;
}
static inline HANDLE CreateEventA(LPSECURITY_ATTRIBUTES sa, BOOL manual, BOOL initial, LPCSTR) {
  return CreateEventW(sa, manual, initial, nullptr);
}

static inline BOOL SetEvent(HANDLE h) {
  if (!mn_win32_set_event(h)) return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  return TRUE;
}

static inline BOOL ResetEvent(HANDLE h) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_EVENT);
  if (!o) return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  pthread_mutex_lock(&o->m);
  o->count = 0;
  pthread_mutex_unlock(&o->m);
  return TRUE;
}

static inline HANDLE CreateSemaphoreW(LPSECURITY_ATTRIBUTES, LONG initial, LONG max, LPCWSTR) {
  if (max <= 0 || initial < 0 || initial > max) return SetLastError(ERROR_INVALID_PARAMETER), nullptr;
  mn_win32_handle *o = mn_win32_new(MN_WIN32_SEMAPHORE);
  o->count = initial, o->max = max;
  return o;
}
/* As upstream's stub spelled it (util_win32_compat.h): a narrow name. */
static inline HANDLE CreateSemaphore(LPSECURITY_ATTRIBUTES sa, LONG initial, LONG max, LPCSTR) {
  return CreateSemaphoreW(sa, initial, max, nullptr);
}

static inline BOOL ReleaseSemaphore(HANDLE h, LONG n, LPLONG previous) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_SEMAPHORE);
  if (!o || n <= 0) return SetLastError(o ? ERROR_INVALID_PARAMETER : ERROR_INVALID_HANDLE), FALSE;
  pthread_mutex_lock(&o->m);
  BOOL ok = o->count <= o->max - n;
  if (ok) {
    if (previous) *previous = o->count;
    o->count += n;
    pthread_cond_broadcast(&o->c);
  }
  pthread_mutex_unlock(&o->m);
  if (!ok) SetLastError(ERROR_TOO_MANY_POSTS);
  return ok;
}

static inline DWORD WaitForSingleObject(HANDLE h, DWORD ms) {
  mn_win32_handle *o = (mn_win32_handle *)h;
  if (!mn_win32_get(h, MN_WIN32_EVENT) && !mn_win32_get(h, MN_WIN32_SEMAPHORE))
    return SetLastError(ERROR_INVALID_HANDLE), WAIT_FAILED;
  struct timespec deadline;
  clock_gettime(CLOCK_REALTIME, &deadline); /* pthread_cond_timedwait's clock on macOS */
  deadline.tv_sec += ms / 1000;
  deadline.tv_nsec += (long)(ms % 1000) * 1000000;
  if (deadline.tv_nsec >= 1000000000) deadline.tv_sec++, deadline.tv_nsec -= 1000000000;
  pthread_mutex_lock(&o->m);
  int r = 0;
  while (o->count == 0 && r != ETIMEDOUT) {
    if (ms == 0) r = ETIMEDOUT;
    else if (ms == INFINITE) pthread_cond_wait(&o->c, &o->m);
    else r = pthread_cond_timedwait(&o->c, &o->m, &deadline);
  }
  BOOL got = o->count > 0;
  if (got && (o->kind == MN_WIN32_SEMAPHORE || !o->manual)) o->count--;
  pthread_mutex_unlock(&o->m);
  return got ? WAIT_OBJECT_0 : WAIT_TIMEOUT;
}

/* Handles. */

static inline BOOL CloseHandle(HANDLE h) {
  if (h == (HANDLE)(intptr_t)-1 || h == (HANDLE)(intptr_t)-2) return TRUE; /* the pseudo-handles */
  mn_win32_handle *o = (mn_win32_handle *)h;
  if (!h || (o->kind != MN_WIN32_EVENT && o->kind != MN_WIN32_SEMAPHORE && o->kind != MN_WIN32_FILE &&
             o->kind != MN_WIN32_THREAD))
    return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  if (__atomic_fetch_sub(&o->refs, 1, __ATOMIC_ACQ_REL) != 1) return TRUE;
  if (o->kind == MN_WIN32_FILE) {
    close(o->fd);
  } else if (o->kind != MN_WIN32_THREAD) {
    pthread_mutex_destroy(&o->m);
    pthread_cond_destroy(&o->c);
  }
  o->kind = 0;
  delete o;
  return TRUE;
}

static inline HANDLE GetCurrentProcess() { return (HANDLE)(intptr_t)-1; }
static inline HANDLE GetCurrentThread() { return (HANDLE)(intptr_t)-2; }
static inline DWORD GetCurrentProcessId() { return (DWORD)getpid(); }
static inline DWORD GetCurrentThreadId() {
  uint64_t id = 0;
  pthread_threadid_np(nullptr, &id);
  return (DWORD)id;
}

/* One process: an event's or semaphore's duplicate is another reference to it, a file's a dup'd descriptor. */
static inline BOOL DuplicateHandle(HANDLE, HANDLE source, HANDLE, LPHANDLE target, DWORD, BOOL, DWORD options) {
  mn_win32_handle *o = (mn_win32_handle *)source;
  if (!target || !source || source == INVALID_HANDLE_VALUE || source == (HANDLE)(intptr_t)-2 ||
      (o->kind != MN_WIN32_EVENT && o->kind != MN_WIN32_SEMAPHORE && o->kind != MN_WIN32_FILE &&
       o->kind != MN_WIN32_THREAD))
    return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  if (o->kind == MN_WIN32_FILE) {
    mn_win32_handle *d = mn_win32_new(MN_WIN32_FILE);
    if ((d->fd = dup(o->fd)) < 0) return SetLastError(mn_win32_from_errno(errno)), delete d, FALSE;
    *target = d;
  } else {
    __atomic_fetch_add(&o->refs, 1, __ATOMIC_RELAXED);
    *target = o;
  }
  if (options & DUPLICATE_CLOSE_SOURCE) CloseHandle(source);
  return TRUE;
}

/* Files. */

static inline HANDLE CreateFileW(LPCWSTR path, DWORD access, DWORD, LPSECURITY_ATTRIBUTES, DWORD disposition, DWORD,
                                 HANDLE) {
  std::string p = mn_win32_utf8(path);
  bool reads = access & GENERIC_READ, writes = access & (GENERIC_WRITE | FILE_APPEND_DATA);
  int flags = O_CLOEXEC | (reads && writes ? O_RDWR : writes ? O_WRONLY : O_RDONLY);
  if ((access & FILE_APPEND_DATA) && !(access & GENERIC_WRITE)) flags |= O_APPEND;
  struct stat st;
  bool existed = stat(p.c_str(), &st) == 0;
  switch (disposition) {
  case CREATE_NEW: flags |= O_CREAT | O_EXCL; break;
  case CREATE_ALWAYS: flags |= O_CREAT | O_TRUNC; break;
  case OPEN_EXISTING: break;
  case OPEN_ALWAYS: flags |= O_CREAT; break;
  case TRUNCATE_EXISTING: flags |= O_TRUNC; break;
  default: return SetLastError(ERROR_INVALID_PARAMETER), INVALID_HANDLE_VALUE;
  }
  int fd = open(p.c_str(), flags, 0644);
  if (fd < 0) return SetLastError(errno == EEXIST ? ERROR_FILE_EXISTS : mn_win32_from_errno(errno)), INVALID_HANDLE_VALUE;
  mn_win32_handle *o = mn_win32_new(MN_WIN32_FILE);
  o->fd = fd;
  /* As Windows: CREATE_ALWAYS and OPEN_ALWAYS say whether the file was there. */
  SetLastError(existed && (disposition == CREATE_ALWAYS || disposition == OPEN_ALWAYS) ? ERROR_ALREADY_EXISTS
                                                                                       : ERROR_SUCCESS);
  return o;
}

static inline BOOL ReadFile(HANDLE h, void *buffer, DWORD n, LPDWORD read_bytes, LPOVERLAPPED) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  if (read_bytes) *read_bytes = 0;
  if (!o) return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  DWORD got = 0;
  while (got < n) {
    ssize_t r = read(o->fd, (char *)buffer + got, n - got);
    if (r < 0 && errno == EINTR) continue;
    if (r < 0) return SetLastError(mn_win32_from_errno(errno)), FALSE;
    if (r == 0) break;
    got += (DWORD)r;
  }
  if (read_bytes) *read_bytes = got;
  return TRUE;
}

static inline BOOL WriteFile(HANDLE h, const void *buffer, DWORD n, LPDWORD written, LPOVERLAPPED) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  if (written) *written = 0;
  if (!o) return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  DWORD put = 0;
  while (put < n) {
    ssize_t r = write(o->fd, (const char *)buffer + put, n - put);
    if (r < 0 && errno == EINTR) continue;
    if (r <= 0) return SetLastError(mn_win32_from_errno(errno)), FALSE;
    put += (DWORD)r;
  }
  if (written) *written = put;
  return TRUE;
}

static inline DWORD SetFilePointer(HANDLE h, LONG distance, PLONG high, DWORD method) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  if (!o || method > FILE_END) return SetLastError(o ? ERROR_INVALID_PARAMETER : ERROR_INVALID_HANDLE), INVALID_SET_FILE_POINTER;
  int64_t d = high ? ((int64_t)*high << 32) | (uint32_t)distance : (int64_t)distance;
  off_t at = lseek(o->fd, d, method == FILE_BEGIN ? SEEK_SET : method == FILE_CURRENT ? SEEK_CUR : SEEK_END);
  if (at < 0) return SetLastError(mn_win32_from_errno(errno)), INVALID_SET_FILE_POINTER;
  if (high) *high = (LONG)(at >> 32);
  SetLastError(ERROR_SUCCESS);
  return (DWORD)at;
}

static inline BOOL SetFilePointerEx(HANDLE h, LARGE_INTEGER distance, PLARGE_INTEGER position, DWORD method) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  if (!o || method > FILE_END) return SetLastError(o ? ERROR_INVALID_PARAMETER : ERROR_INVALID_HANDLE), FALSE;
  off_t at = lseek(o->fd, distance.QuadPart, method == FILE_BEGIN ? SEEK_SET : method == FILE_CURRENT ? SEEK_CUR : SEEK_END);
  if (at < 0) return SetLastError(mn_win32_from_errno(errno)), FALSE;
  if (position) position->QuadPart = at;
  return TRUE;
}

static inline BOOL SetEndOfFile(HANDLE h) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  if (!o) return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  off_t at = lseek(o->fd, 0, SEEK_CUR);
  if (at < 0 || ftruncate(o->fd, at)) return SetLastError(mn_win32_from_errno(errno)), FALSE;
  return TRUE;
}

static inline BOOL FlushFileBuffers(HANDLE h) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  if (!o) return SetLastError(ERROR_INVALID_HANDLE), FALSE;
  return fsync(o->fd) == 0;
}

static inline BOOL GetFileSizeEx(HANDLE h, PLARGE_INTEGER size) {
  mn_win32_handle *o = mn_win32_get(h, MN_WIN32_FILE);
  struct stat st;
  if (!o || !size) return SetLastError(o ? ERROR_INVALID_PARAMETER : ERROR_INVALID_HANDLE), FALSE;
  if (fstat(o->fd, &st)) return SetLastError(mn_win32_from_errno(errno)), FALSE;
  size->QuadPart = st.st_size;
  return TRUE;
}

static inline DWORD GetFileAttributesW(LPCWSTR path) {
  struct stat st;
  if (stat(mn_win32_utf8(path).c_str(), &st)) return SetLastError(mn_win32_from_errno(errno)), INVALID_FILE_ATTRIBUTES;
  return S_ISDIR(st.st_mode) ? FILE_ATTRIBUTE_DIRECTORY : FILE_ATTRIBUTE_NORMAL;
}

static inline BOOL CreateDirectoryW(LPCWSTR path, LPSECURITY_ATTRIBUTES) {
  if (mkdir(mn_win32_utf8(path).c_str(), 0755)) return SetLastError(mn_win32_from_errno(errno)), FALSE;
  return TRUE;
}

static inline BOOL MoveFileExW(LPCWSTR from, LPCWSTR to, DWORD flags) {
  std::string a = mn_win32_utf8(from), b = mn_win32_utf8(to);
  int r = flags & MOVEFILE_REPLACE_EXISTING ? rename(a.c_str(), b.c_str())
                                            : renamex_np(a.c_str(), b.c_str(), RENAME_EXCL);
  if (r) return SetLastError(mn_win32_from_errno(errno)), FALSE;
  return TRUE;
}
static inline BOOL MoveFileW(LPCWSTR from, LPCWSTR to) { return MoveFileExW(from, to, 0); }

static inline BOOL DeleteFileW(LPCWSTR path) {
  if (unlink(mn_win32_utf8(path).c_str())) return SetLastError(mn_win32_from_errno(errno)), FALSE;
  return TRUE;
}

static inline HLOCAL LocalFree(HLOCAL p) {
  free(p);
  return nullptr;
}

/* Time. QueryPerformanceCounter counts nanoseconds. */

static inline BOOL QueryPerformanceCounter(LARGE_INTEGER *count) {
  count->QuadPart = (LONGLONG)clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
  return TRUE;
}
static inline BOOL QueryPerformanceFrequency(LARGE_INTEGER *frequency) {
  frequency->QuadPart = 1000000000;
  return TRUE;
}
static inline ULONGLONG GetTickCount64() { return clock_gettime_nsec_np(CLOCK_UPTIME_RAW) / 1000000; }
static inline DWORD GetTickCount() { return (DWORD)GetTickCount64(); }
static inline void Sleep(DWORD ms) {
  struct timespec t = {(time_t)(ms / 1000), (long)(ms % 1000) * 1000000};
  while (nanosleep(&t, &t) && errno == EINTR) {}
}
static inline void GetLocalTime(LPSYSTEMTIME st) {
  struct timespec now;
  clock_gettime(CLOCK_REALTIME, &now);
  struct tm tm;
  localtime_r(&now.tv_sec, &tm);
  st->wYear = (WORD)(tm.tm_year + 1900), st->wMonth = (WORD)(tm.tm_mon + 1), st->wDayOfWeek = (WORD)tm.tm_wday;
  st->wDay = (WORD)tm.tm_mday, st->wHour = (WORD)tm.tm_hour, st->wMinute = (WORD)tm.tm_min;
  st->wSecond = (WORD)tm.tm_sec, st->wMilliseconds = (WORD)(now.tv_nsec / 1000000);
}

/* Threads. Priorities stay the system's (games' threads get none natively either). */

static inline BOOL SetThreadPriority(HANDLE, int) { return TRUE; }

/* A thread of this process by its id (GetCurrentThreadId's), for GetThreadDescription; NULL if there is none. */
static inline HANDLE OpenThread(DWORD, BOOL, DWORD tid) {
  struct proc_threadinfo info;
  if (!tid || proc_pidinfo(getpid(), PROC_PIDTHREADID64INFO, tid, &info, sizeof info) != sizeof info)
    return SetLastError(ERROR_INVALID_PARAMETER), nullptr;
  mn_win32_handle *o = mn_win32_new(MN_WIN32_THREAD);
  o->tid = tid;
  return o;
}

/* A thread's name: the calling thread's (pthread_getname_np) or an opened one's (proc_pidinfo), freed with
 * LocalFree. */
static inline HRESULT GetThreadDescription(HANDLE thread, LPWSTR *description) {
  char name[MAXTHREADNAMESIZE] = "";
  mn_win32_handle *o = mn_win32_get(thread, MN_WIN32_THREAD);
  if (!description || (!o && thread != GetCurrentThread())) return E_INVALIDARG;
  if (o) {
    struct proc_threadinfo info;
    if (proc_pidinfo(getpid(), PROC_PIDTHREADID64INFO, o->tid, &info, sizeof info) != sizeof info) return E_FAIL;
    strlcpy(name, info.pth_name, sizeof name);
  } else {
    pthread_getname_np(pthread_self(), name, sizeof name);
  }
  size_t n = strlen(name);
  LPWSTR w = (LPWSTR)malloc((n + 1) * sizeof(WCHAR));
  if (!w) return E_OUTOFMEMORY;
  for (size_t i = 0; i <= n; i++) w[i] = (WCHAR)(unsigned char)name[i];
  *description = w;
  return S_OK;
}

/* Modules and the environment. */

static inline HMODULE LoadLibraryA(LPCSTR name) { return (HMODULE)dlopen(name, RTLD_NOW | RTLD_LOCAL); }
static inline HMODULE LoadLibraryW(LPCWSTR name) { return LoadLibraryA(mn_win32_utf8(name).c_str()); }
static inline FARPROC GetProcAddress(HMODULE module, LPCSTR name) { return (FARPROC)dlsym(module, name); }
static inline BOOL FreeLibrary(HMODULE module) { return dlclose(module) == 0; }
static inline BOOL DisableThreadLibraryCalls(HMODULE) { return TRUE; }

static inline BOOL SetEnvironmentVariableW(LPCWSTR name, LPCWSTR value) {
  std::string n = mn_win32_utf8(name);
  return (value ? setenv(n.c_str(), mn_win32_utf8(value).c_str(), 1) : unsetenv(n.c_str())) == 0;
}

/* No keyboard natively: the F9 pass dump keeps its environment trigger. */
static inline SHORT GetAsyncKeyState(int) { return 0; }

#endif
