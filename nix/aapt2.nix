# aapt2 for the host, from the Android sources.
#
# Google ships the Android SDK's native tools for x86_64 Linux and for macOS
# and nothing else, and aapt2 is the one native tool the Android Gradle
# Plugin runs for `assembleDebug` (d8, apksigner and the rest are Java). An
# ARM Linux machine that wants to build the phone without emulating x86
# needs an aapt2 built for it, at the level of the platform it links
# against: platform 35's android.jar carries Android 15's resource table
# format, which the newest prebuilt anyone else offers for aarch64 glibc
# (Debian's, at Android 14) refuses to read. So this compiles it.
#
# The sources are frameworks/base and system/core at the platform-tools
# 35.0.2 tag — sparse checkouts of the few directories aapt2 is made of, from
# GitHub's aosp-mirror — and, for the libraries that live in repositories the
# mirror does not carry (libbase, liblog, libziparchive, incfs's map_ptr, the
# native headers, fmtlib), Debian's source tarball of the same release. The
# build is aapt2/CMakeLists.txt beside this file, with the host's protobuf,
# libpng, expat and zlib. One recipe for every architecture; only the
# compiler differs.
{ lib, stdenv, fetchgit, fetchurl, cmake, ninja, protobuf_21, libpng, expat, zlib, gtest }:

let
  tag = "platform-tools-35.0.2";

  base = fetchgit {
    name = "platform_frameworks_base";
    url = "https://github.com/aosp-mirror/platform_frameworks_base";
    rev = "refs/tags/${tag}";
    sparseCheckout = [ "tools/aapt2" "libs/androidfw" "cmds/idmap2/libidmap2_policies" ];
    hash = "sha256-Wwgiog7yvrXzcg4xRUGqDVnAWtaPhlVoGXKIwCoyeTE=";
  };
  core = fetchgit {
    name = "platform_system_core";
    url = "https://github.com/aosp-mirror/platform_system_core";
    rev = "refs/tags/${tag}";
    sparseCheckout = [ "libutils" "libcutils" "libsystem" "include" "libprocessgroup/include" "libvndksupport/include" ];
    hash = "sha256-aT1MkTezg7p5sSN8qX2Yp8Vr06jNiPQgbXl9gEcVRT8=";
  };
  debian = fetchurl {
    url = "https://deb.debian.org/debian/pool/main/a/android-platform-tools/android-platform-tools_35.0.2.orig.tar.xz";
    sha256 = "ec1d317608db3328bfbddf7152c8d7f185c7c87b2175081416344434546a43da";
  };
in
stdenv.mkDerivation {
  pname = "aapt2";
  version = "35.0.2";

  src = ./aapt2;

  nativeBuildInputs = [ cmake ninja protobuf_21 ];
  # gtest for gtest_prod.h alone: libziparchive's header names FRIEND_TEST.
  buildInputs = [ protobuf_21 libpng expat zlib gtest ];

  # Lay the three source trees out where CMakeLists.txt expects them, and
  # give protoc a root under which "frameworks/base/tools/aapt2/X.proto"
  # resolves, which is how the .proto files import one another.
  postUnpack = ''
    mkdir -p "$sourceRoot/src/deb" "$sourceRoot/proto-root/frameworks/base/tools"
    cp -r ${base} "$sourceRoot/src/base"
    cp -r ${core} "$sourceRoot/src/core"
    tar -xJf ${debian} -C "$sourceRoot/src/deb" \
      ./system/libbase ./system/logging ./system/libziparchive \
      ./system/incremental_delivery ./frameworks/native/include ./external/fmtlib
    chmod -R u+w "$sourceRoot/src"
    # Debian's: libstdc++'s std::lower_bound decrements map_ptr's iterator,
    # which Android's libc++ never asked of it.
    patch -d "$sourceRoot/src/deb" -p1 < "$sourceRoot/patches/map_ptr-const_iterator-decrement.patch"
    ln -s "$PWD/$sourceRoot/src/base/tools/aapt2" "$sourceRoot/proto-root/frameworks/base/tools/aapt2"
  '';

  env.NIX_CFLAGS_COMPILE = "-DAAPT2_BUILD_NUMBER=\"nix-${tag}\"";

  doInstallCheck = true;
  installCheckPhase = ''
    $out/bin/aapt2 version
  '';

  meta = {
    description = "aapt2 at Android 15 level, built from source for the host";
    license = lib.licenses.asl20;
    platforms = lib.platforms.linux;
  };
}
