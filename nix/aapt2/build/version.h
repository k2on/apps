// What Soong's libbuildversion provides: the build number aapt2 prints in
// its fingerprint. This build's is its tag.
#pragma once
#include <string>
namespace android::build {
std::string GetBuildNumber();
}
