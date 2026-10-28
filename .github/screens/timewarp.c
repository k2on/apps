// Time, as the frameworks that animate read it, running `TIMEWARP` times
// slower than life while the showcase moves something: loaded into it on the
// simulator (SIMCTL_CHILD_DYLD_INSERT_LIBRARIES), it stands in for the
// system's clocks by dyld interposing.
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
// The showcase says when a move begins and ends (the Darwin notifications
// dev.arkui.showcase.slow and .fast), so a still screen between moves takes
// life's time to record, not thirty times it. Each change is written to
// Documents/timewarp.log in the app's container, as `<seconds> <factor>`
// by the host's clock (CACurrentMediaTime's), which is how encode.sh puts
// the recording back into life's time.
//
//   xcrun -sdk iphonesimulator clang -arch arm64 -mios-simulator-version-min=17.0 \
//     -dynamiclib -o timewarp.dylib timewarp.c
#include <dlfcn.h>
#include <mach/mach_time.h>
#include <notify.h>
#include <dispatch/dispatch.h>
#include <stdatomic.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define INTERPOSE(replacement, replacee)                                                 \
    __attribute__((used)) static struct {                                               \
        const void *with;                                                               \
        const void *instead;                                                            \
    } interpose_##replacee __attribute__((section("__DATA,__interpose"))) = {          \
        (const void *)(uintptr_t)&replacement, (const void *)(uintptr_t)&replacee}

// How many times slower a move is taken.
static double slower = 1;

// Time from `real` on (mach ticks) runs `factor` times slower, from `warped`:
// each change a new span, written once and then only read.
enum { SPANS = 256 };
static struct { uint64_t real, warped; double factor; } spans[SPANS];
static _Atomic int current = -1;

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

// `now`, as the span it falls in has it.
static uint64_t warp(uint64_t now) {
    int i = atomic_load(&current);
    while (i >= 0 && now < spans[i].real) i--;
    if (i < 0) return now;
    return spans[i].warped + (uint64_t)((double)(now - spans[i].real) / spans[i].factor);
}

// `now`, slowed if it is read by an animating image.
static uint64_t slowed(uint64_t now, const void *caller) {
    return atomic_load(&current) < 0 || !animates(caller) ? now : warp(now);
}

// From this image, the calls reach the real clocks: dyld does not
// interpose an image's calls to what it interposes itself.
static uint64_t warped_absolute(void) {
    return slowed(mach_absolute_time(), __builtin_return_address(0));
}
INTERPOSE(warped_absolute, mach_absolute_time);

static uint64_t warped_continuous(void) {
    // The continuous clock runs ahead of the absolute by the time asleep,
    // which does not change while a shot is recorded.
    uint64_t now = mach_absolute_time();
    return mach_continuous_time() - now + slowed(now, __builtin_return_address(0));
}
INTERPOSE(warped_continuous, mach_continuous_time);

static uint64_t warped_nsec(clockid_t clock) {
    uint64_t now = clock_gettime_nsec_np(clock);
    if (clock != CLOCK_UPTIME_RAW && clock != CLOCK_MONOTONIC_RAW && clock != CLOCK_MONOTONIC) return now;
    uint64_t ticks = mach_absolute_time();
    uint64_t behind = ticks - slowed(ticks, __builtin_return_address(0));
    mach_timebase_info_data_t base;
    mach_timebase_info(&base);
    return now - behind * base.numer / base.denom;
}
INTERPOSE(warped_nsec, clock_gettime_nsec_np);

static char log_path[1024];

// From now on, time runs `factor` times slower, carrying on from where it is.
static void change(double factor) {
    int i = atomic_load(&current);
    if (i + 1 >= SPANS || (i >= 0 && spans[i].factor == factor)) return;
    uint64_t now = mach_absolute_time();
    spans[i + 1].real = now;
    spans[i + 1].warped = warp(now);
    spans[i + 1].factor = factor;
    atomic_store(&current, i + 1);
    mach_timebase_info_data_t base;
    mach_timebase_info(&base);
    FILE *log = fopen(log_path, "a");
    if (log) {
        fprintf(log, "%.9f %g\n", (double)now * base.numer / base.denom / 1e9, factor);
        fclose(log);
    }
}

// Each move is slowed from when the showcase says it begins to when it says
// it ends; a launch, and the stills between, run at life's speed.
__attribute__((constructor)) static void timewarp(void) {
    const char *asked = getenv("TIMEWARP");
    slower = asked ? atof(asked) : 1;
    if (slower <= 1) return;
    const char *home = getenv("HOME");
    snprintf(log_path, sizeof log_path, "%s/Documents/timewarp.log", home ? home : "/tmp");
    unlink(log_path);
    dispatch_queue_t queue = dispatch_queue_create("timewarp", DISPATCH_QUEUE_SERIAL);
    int token;
    notify_register_dispatch("dev.arkui.showcase.slow", &token, queue, ^(int t) { (void)t; change(slower); });
    notify_register_dispatch("dev.arkui.showcase.fast", &token, queue, ^(int t) { (void)t; change(1); });
}
