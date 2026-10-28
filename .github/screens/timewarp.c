// Time, as the frameworks that animate read it, running `TIMEWARP` times
// slower than life, from the moment a shot is told to go: loaded into the
// showcase on the simulator (SIMCTL_CHILD_DYLD_INSERT_LIBRARIES), it stands
// in for the system's clocks by dyld interposing.
//
// A recording on a runner whose simulator composes ten frames a second of
// the system's zoom then holds ten times the frames of its motion, and is
// sped back up after (encode.sh). The zoom runs in the app on its own clock,
// which nothing else slows: the window's layer speed slows Core Animation's
// server-side clock only, and the simulator's Slow Animations stops its screen.
//
// Only Core Animation, UIKit and SwiftUI are given the slow time; everything
// else (dispatch, run loops, timers) keeps life's, as it sets the kernel's
// deadlines by it, and a slow clock there would have every timer fire at once.
//
//   xcrun -sdk iphonesimulator clang -arch arm64 -mios-simulator-version-min=17.0 \
//     -dynamiclib -o timewarp.dylib timewarp.c
#include <dlfcn.h>
#include <mach/mach_time.h>
#include <notify.h>
#include <dispatch/dispatch.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>

#define INTERPOSE(replacement, replacee)                                                 \
    __attribute__((used)) static struct {                                               \
        const void *with;                                                               \
        const void *instead;                                                            \
    } interpose_##replacee __attribute__((section("__DATA,__interpose"))) = {          \
        (const void *)(uintptr_t)&replacement, (const void *)(uintptr_t)&replacee}

// How many times slower, and the life's instant it began (0: not yet).
static double factor = 1;
static _Atomic uint64_t origin;

// The images whose reading of the time is slowed, by their path.
static const char *animating[] = {
    "/QuartzCore.framework/",
    "/UIKitCore.framework/",
    "/SwiftUI.framework/",
    "/SwiftUICore.framework/",
};

// Whether the code at `caller` belongs to an animating image, remembered by
// image so each is looked up once.
enum { IMAGES = 64 };
static struct { const void *base; int animates; } seen[IMAGES];
static _Atomic int seen_count;

static int animates(const void *caller) {
    Dl_info info;
    if (!dladdr(caller, &info) || !info.dli_fbase) return 0;
    int n = atomic_load(&seen_count);
    for (int i = 0; i < n; i++)
        if (seen[i].base == info.dli_fbase) return seen[i].animates;
    int yes = 0;
    for (size_t i = 0; i < sizeof animating / sizeof *animating; i++)
        if (info.dli_fname && strstr(info.dli_fname, animating[i])) yes = 1;
    if (n < IMAGES) {
        seen[n].base = info.dli_fbase;
        seen[n].animates = yes;
        atomic_store(&seen_count, n + 1);
    }
    return yes;
}

// `now`, slowed if it is read by an animating image once the warp began.
static uint64_t slowed(uint64_t now, uint64_t start, const void *caller) {
    if (!start || now < start || !animates(caller)) return now;
    return start + (uint64_t)((double)(now - start) / factor);
}

// From this image, the calls reach the real clocks: dyld does not
// interpose an image's calls to what it interposes itself.
static uint64_t warped_absolute(void) {
    return slowed(mach_absolute_time(), atomic_load(&origin), __builtin_return_address(0));
}
INTERPOSE(warped_absolute, mach_absolute_time);

static uint64_t warped_continuous(void) {
    uint64_t start = atomic_load(&origin);
    // The continuous clock runs ahead of the absolute by the time asleep,
    // which does not change while a shot is recorded.
    uint64_t ahead = mach_continuous_time() - mach_absolute_time();
    return slowed(mach_continuous_time(), start ? start + ahead : 0, __builtin_return_address(0));
}
INTERPOSE(warped_continuous, mach_continuous_time);

static uint64_t warped_nsec(clockid_t clock) {
    uint64_t now = clock_gettime_nsec_np(clock);
    if (clock != CLOCK_UPTIME_RAW && clock != CLOCK_MONOTONIC_RAW && clock != CLOCK_MONOTONIC) return now;
    uint64_t start = atomic_load(&origin);
    if (!start) return now;
    mach_timebase_info_data_t base;
    mach_timebase_info(&base);
    uint64_t start_ns = start * base.numer / base.denom;
    uint64_t at = clock_gettime_nsec_np(CLOCK_UPTIME_RAW);
    // The clock asked for, from its own zero, the way UPTIME_RAW is slowed.
    return now - (at - slowed(at, start_ns, __builtin_return_address(0)));
}
INTERPOSE(warped_nsec, clock_gettime_nsec_np);

// The warp begins when the shot is told to go: the showcase's own
// notification (ios.sh sends it), so a launch runs at life's speed.
__attribute__((constructor)) static void timewarp(void) {
    const char *asked = getenv("TIMEWARP");
    factor = asked ? atof(asked) : 1;
    if (factor <= 1) return;
    int token;
    notify_register_dispatch("dev.arkui.showcase.go", &token, dispatch_get_main_queue(), ^(int t) {
        (void)t;
        uint64_t expected = 0;
        atomic_compare_exchange_strong(&origin, &expected, mach_absolute_time());
    });
}
