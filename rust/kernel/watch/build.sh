#!/usr/bin/env bash
# Builds the kernel for the watch (thumbv7m-none-eabi: a Cortex-M3 with
# soft floats, as a Pebble app is), as checks.kernel-size and
# checks.kernel-qemu do: into OUT, the size probe
# at the budget's settings (opt-level "z", LTO, panic abort) and the
# selftest (the same, with the tests built in and overflow checked). Then
# prints the probe's code, .text and .rodata, in bytes.
#
#   rust/kernel/watch/build.sh OUT
#
# rustc is the one on PATH, which needs the target's std (the flake's
# toolchain has it; rustup: `rustup target add thumbv7m-none-eabi`).
set -euo pipefail
out=$1
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
target=(--edition 2021 --target thumbv7m-none-eabi -C opt-level=z -C panic=abort -C codegen-units=1 -C debuginfo=0 -D warnings)

rustc "${target[@]}" --crate-type rlib --crate-name ark_kernel "$here/../src/lib.rs" -o "$out/libark_kernel.rlib"
rustc "${target[@]}" -C lto=fat --crate-type bin --crate-name probe "$here/probe.rs" \
  --extern ark_kernel="$out/libark_kernel.rlib" -C link-arg=-T"$here/link.x" -o "$out/probe"

mkdir -p "$out/selftest-lib"
rustc "${target[@]}" -C overflow-checks=on --cfg ark_selftest --crate-type rlib --crate-name ark_kernel "$here/../src/lib.rs" \
  -o "$out/selftest-lib/libark_kernel.rlib"
rustc "${target[@]}" -C overflow-checks=on -C lto=fat --crate-type bin --crate-name selftest "$here/selftest.rs" \
  --extern ark_kernel="$out/selftest-lib/libark_kernel.rlib" -C link-arg=-T"$here/link.x" -o "$out/selftest"

# Each section's size, from its header; the code is .text and .rodata.
size() { echo $((16#$(readelf -S -W "$out/probe" | awk -v s="$1" '{ for (i = 1; i < NF; i++) if ($i == s) print $(i + 4) }'))); }
text=$(size .text)
rodata=$(size .rodata)
echo "$((text + rodata))" > "$out/code-bytes"
echo "the kernel's probe on thumbv7m-none-eabi: .text $text + .rodata $rodata = $((text + rodata)) bytes"
