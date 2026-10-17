#pragma once

/*
 * The native build's HANDLE layout (win32_native.h) and SetEvent, in C as well as C++: winemetal's unix side, which is
 * C, sets the events a fence's SetEventOnCompletion names. refs is a plain int under __atomic builtins so the layout
 * is the same in both languages.
 */

#ifndef _WIN32

#include <pthread.h>
#include <stdint.h>

#define MN_WIN32_EVENT 0x6e657665u
#define MN_WIN32_SEMAPHORE 0x6d657373u
#define MN_WIN32_FILE 0x656c6966u
#define MN_WIN32_THREAD 0x64726874u

struct mn_win32_handle {
  uint32_t kind;
  int refs;
  /* events and semaphores */
  pthread_mutex_t m;
  pthread_cond_t c;
  int32_t count, max; /* an event: 1 when set; a semaphore: its count and maximum */
  int manual;
  /* files */
  int fd;
  /* threads (OpenThread): the thread's id */
  uint64_t tid;
};

/* Sets h if it is an event: 1, else 0. */
static inline int mn_win32_set_event(void *h) {
  struct mn_win32_handle *o = (struct mn_win32_handle *)h;
  if (!o || h == (void *)(intptr_t)-1 || h == (void *)(intptr_t)-2 || o->kind != MN_WIN32_EVENT) return 0;
  pthread_mutex_lock(&o->m);
  o->count = 1;
  pthread_cond_broadcast(&o->c);
  pthread_mutex_unlock(&o->m);
  return 1;
}

#endif
