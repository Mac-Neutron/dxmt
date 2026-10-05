#include "dxmt_shader_cache.hpp"
#include "util_env.hpp"
#include "util_string.hpp"
#include <version.h>
#include "dxmt_translator_key.h"

namespace dxmt {

// MacNeutron: keyed on the translator's key when the build has one (a hash of what changes translated output), so a
// re-fetch of the same patches, or a change outside the translator, keeps the cache; otherwise on DXMT's git version.
uint64_t
ShaderCacheVersion() {
  uint64_t hash = 0xcbf29ce484222325ull; // FNV-1a
  for (const char *p = DXMT_TRANSLATOR_KEY[0] ? DXMT_TRANSLATOR_KEY : DXMT_VERSION; *p; p++)
    hash = (hash ^ (uint8_t)*p) * 0x100000001b3ull;
  for (int i = 0; i < 4; i++)
    hash = (hash ^ (uint8_t)((uint32_t)AIRCONV_VERSION >> (8 * i))) * 0x100000001b3ull;
  return hash;
}

ShaderCache &
ShaderCache::getInstance(WMTMetalVersion version) {
  static dxmt::mutex mutex;
  static std::unordered_map<WMTMetalVersion, std::unique_ptr<ShaderCache>> caches;

  std::lock_guard<dxmt::mutex> lock(mutex);
  auto iter = caches.find(version);
  if (iter == caches.end()) {
    auto inserted = caches.insert({version, std::make_unique<ShaderCache>(version)});
    return *inserted.first->second;
  }
  return *iter->second;
}

ShaderCache::ShaderCache(WMTMetalVersion metal_version) {
  if (env::getEnvVar("DXMT_SHADER_CACHE") == "0")
    return;
  std::string path;
  if (path = env::getEnvVar("DXMT_SHADER_CACHE_PATH"); !path.empty() && path.starts_with("/")) {
    if (!path.ends_with('/'))
      path += "/";
  } else {
    path = str::format("dxmt/", env::getCacheExeName(), "/");
  }
  path += str::format("shaders_", (unsigned int)metal_version, ".db");
  scache_writer_ = WMT::CacheWriter::alloc_init(path.c_str(), ShaderCacheVersion());
  scache_reader_ = WMT::CacheReader::alloc_init(path.c_str(), ShaderCacheVersion());
}

} // namespace dxmt