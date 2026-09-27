#include "build/version.h"
#ifndef AAPT2_BUILD_NUMBER
#define AAPT2_BUILD_NUMBER "nix"
#endif
namespace android::build {
std::string GetBuildNumber() {
  return AAPT2_BUILD_NUMBER;
}
}  // namespace android::build
