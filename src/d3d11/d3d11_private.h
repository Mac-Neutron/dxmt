#pragma once

extern "C" void _massert (const char *_Message, const char *_File, unsigned _Line) __attribute__((noreturn));

#ifdef NDEBUG
#define D3D11_ASSERT(_Expression) ((void)0)
#else
#define D3D11_ASSERT(_Expression) \
 (void) \
 ((!!(_Expression)) || \
  (_massert(#_Expression,__FILE__,__LINE__),0))
#endif

#ifdef _WIN32
#define UNIMPLEMENTED_END abort();
#else
// MacNeutron, the native build: a VM host's stubs catch it and fail the call, rather than lose the VM.
#include <stdexcept>
#define UNIMPLEMENTED_END throw std::logic_error("not implemented");
#endif

#define UNIMPLEMENTED(text)                                                                                            \
  Logger::err(str::format(__FILE__, ":", __FUNCTION__, ": ", #text, "."));                                             \
  UNIMPLEMENTED_END                                                                                                    \
  __builtin_unreachable();

#define UNREACHABLE                                                                                                    \
  Logger::err(str::format(__FILE__, ":", __FUNCTION__, " runs into an unreachable path."));                            \
  __builtin_unreachable();

template <class... Ts> struct patterns : Ts... {
  using Ts::operator()...;
};
template <class... Ts> patterns(Ts...) -> patterns<Ts...>;
