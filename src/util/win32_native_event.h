#pragma once

/*
 * The native build's HANDLE layout (win32_native.h) and SetEvent, in C as well as C++: winemetal's unix side, which is
 * C, sets the events a fence's SetEventOnCompletion names. refs is a plain int under __atomic builtins so the layout
 * is the same in both languages.
 */

#ifndef _WIN32

#include <pthread.h>
#include <stdint.h>
#include <stdlib.h>
#include <unistd.h>

#define MN_WIN32_EVENT 0x6e657665u
#define MN_WIN32_SEMAPHORE 0x6d657373u
#define MN_WIN32_FILE 0x656c6966u
#define MN_WIN32_THREAD 0x64726874u
#define MN_WIN32_RELAY 0x79616c72u

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
  /* relays: what SetEvent calls instead of waking waiters, and what the last reference's drop calls */
  void (*relay)(void *ctx);
  void (*relay_end)(void *ctx);
  void *relay_ctx;
};

static inline int mn_win32_is_handle(void *h) {
  struct mn_win32_handle *o = (struct mn_win32_handle *)h;
  return o && h != (void *)(intptr_t)-1 && h != (void *)(intptr_t)-2 &&
         (o->kind == MN_WIN32_EVENT || o->kind == MN_WIN32_SEMAPHORE || o->kind == MN_WIN32_FILE ||
          o->kind == MN_WIN32_THREAD || o->kind == MN_WIN32_RELAY);
}

/* Another reference to h: a fence's listener holds the event it will set, as the Windows kernel does, so the program
 * may close it first. */
static inline void mn_win32_ref(void *h) {
  if (mn_win32_is_handle(h)) __atomic_fetch_add(&((struct mn_win32_handle *)h)->refs, 1, __ATOMIC_RELAXED);
}

/* Drops a reference (CloseHandle is one); the last frees the object. 1 if h was a handle, else 0. */
static inline int mn_win32_unref(void *h) {
  struct mn_win32_handle *o = (struct mn_win32_handle *)h;
  if (!mn_win32_is_handle(h)) return 0;
  if (__atomic_fetch_sub(&o->refs, 1, __ATOMIC_ACQ_REL) != 1) return 1;
  if (o->kind == MN_WIN32_FILE) {
    close(o->fd);
  } else if (o->kind == MN_WIN32_RELAY) {
    if (o->relay_end) o->relay_end(o->relay_ctx);
  } else if (o->kind != MN_WIN32_THREAD) {
    pthread_mutex_destroy(&o->m);
    pthread_cond_destroy(&o->c);
  }
  o->kind = 0;
  free(o);
  return 1;
}

/* An event whose SetEvent calls fn(ctx) on the setter's thread instead of waking waiters (it can't be waited on): how
 * a host that holds DXMT learns a fence's completion. Its last reference's drop calls end(ctx) (if not NULL), whether
 * it was set or not (a fence released before reaching its value), then frees it. */
static inline void *mn_win32_relay_event(void (*fn)(void *ctx), void (*end)(void *ctx), void *ctx) {
  struct mn_win32_handle *o = (struct mn_win32_handle *)calloc(1, sizeof *o);
  if (!o) return 0;
  o->kind = MN_WIN32_RELAY, o->refs = 1, o->fd = -1, o->relay = fn, o->relay_end = end, o->relay_ctx = ctx;
  return o;
}

/* Sets h if it is an event, or calls a relay's function: 1, else 0. */
static inline int mn_win32_set_event(void *h) {
  struct mn_win32_handle *o = (struct mn_win32_handle *)h;
  if (!o || h == (void *)(intptr_t)-1 || h == (void *)(intptr_t)-2) return 0;
  if (o->kind == MN_WIN32_RELAY) {
    o->relay(o->relay_ctx);
    return 1;
  }
  if (o->kind != MN_WIN32_EVENT) return 0;
  pthread_mutex_lock(&o->m);
  o->count = 1;
  pthread_cond_broadcast(&o->c);
  pthread_mutex_unlock(&o->m);
  return 1;
}

#endif
