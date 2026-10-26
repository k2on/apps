# An Android SDK for a Linux that Google ships none for.
#
# What `assembleDebug` reads from an SDK is architecture-free almost
# entirely: the platform's `android.jar`, and the build tools' jars and
# `source.properties`. The one native program the Android Gradle Plugin runs
# is aapt2, so this takes Google's zips for the rest and puts the host's
# aapt2 (nix/aapt2.nix, from Debian) where AGP is pointed at. The x86_64
# binaries left in `build-tools/` are inert — nothing here runs zipalign or
# aidl — and nix is told not to touch them.
#
# Platform-tools too, though nothing runs adb: an SDK without it is one AGP
# would install it into, and installing reads Google's repository indexes,
# which Google rewrites whenever it publishes anything — a recorded hash of
# one goes stale on its own. Every zip here is one release's, which never
# changes, and the builds forbid AGP to download (`android.builder.sdkDownload`).
{ lib, stdenv, fetchurl, unzip, aapt2 }:

let
  platform = fetchurl {
    url = "https://dl.google.com/android/repository/platform-35_r01.zip";
    sha1 = "c84ed39cecaeec13bc06c67639fcf86734013d98";
  };
  platformTools = fetchurl {
    url = "https://dl.google.com/android/repository/platform-tools_r37.0.1-linux.zip";
    hash = "sha256-0jDxOEL2D3gqhkX5yBP4+EW/NgiepyifKMSPF5eTE/E=";
  };
  buildTools = fetchurl {
    url = "https://dl.google.com/android/repository/build-tools_r35_linux.zip";
    sha1 = "2cfaa0bbb2336e9ec18ed3ecea84fa2e2af607bc";
  };
in
stdenv.mkDerivation {
  pname = "android-sdk-for-gradle";
  version = "35";
  dontUnpack = true;
  nativeBuildInputs = [ unzip ];
  dontFixup = true;

  installPhase = ''
    runHook preInstall
    sdk=$out/libexec/android-sdk
    mkdir -p $sdk/platforms $sdk/build-tools $sdk/licenses
    unzip -q ${platform} -d $sdk/platforms
    unzip -q ${platformTools} -d $sdk
    unzip -q ${buildTools} -d $sdk/build-tools
    mv $sdk/build-tools/android-15 $sdk/build-tools/35.0.0
    rm $sdk/build-tools/35.0.0/aapt2
    ln -s ${aapt2}/bin/aapt2 $sdk/build-tools/35.0.0/aapt2
    # The licence AGP would otherwise ask to have accepted; the same text
    # nixpkgs' androidenv writes for `android_sdk.accept_license = true`.
    echo "24333f8a63b6825ea9c5514f83c2829b004d1fee" > $sdk/licenses/android-sdk-license
    runHook postInstall
  '';

  passthru = { inherit aapt2; root = "libexec/android-sdk"; };

  meta = {
    description = "platform 35, platform-tools 37.0.1 and build-tools 35.0.0 with the host's aapt2, for gradle";
    platforms = [ "aarch64-linux" "x86_64-linux" ];
  };
}
