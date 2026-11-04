// Opt-in diagnostic (LIVEKIT_CODEC44_NATIVE_TRACE=1) for issue #44.
// Emits only fixed-prefix lines with bools/numbers/sanitized profile hex.
#pragma once

#include <cstdarg>
#include <cstdio>
#include <cstdlib>

namespace livekit_ffi {

inline bool Codec44TraceEnabled() {
  static const bool enabled = [] {
    const char* v = std::getenv("LIVEKIT_CODEC44_NATIVE_TRACE");
    return v && v[0] == '1' && v[1] == '\0';
  }();
  return enabled;
}

// `fmt` must only use %d/%u/%s with sanitized tokens. Callers rate-limit.
inline void Codec44Trace(const char* fmt, ...) {
  if (!Codec44TraceEnabled()) return;
  char buffer[256];
  va_list args;
  va_start(args, fmt);
  std::vsnprintf(buffer, sizeof(buffer), fmt, args);
  va_end(args);
  std::fprintf(stderr, "CODEC44_NATIVE %s\n", buffer);
  std::fflush(stderr);
}

}  // namespace livekit_ffi
