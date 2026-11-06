# The screens workflows' own tools, apart from the rest of the flake: the
# docs' recordings job keeps them bundled in GitHub's cache keyed on this
# file and flake.lock (.github/workflows/screens.yaml), so that a change to
# flake.nix that is none of theirs does not make it build them again.
{ pkgs }:
{
  # The ffmpeg the screens workflow cuts the phones' recordings with
  # (.github/screens/encode.sh), and nothing else: H.264 out through
  # x264, the native decoders and the MP4 and MOV containers, the
  # filters (setpts, fps, scale, format, mpdecimate) and ffprobe. No
  # other codec, no network, no device, no docs — a tenth of what
  # Homebrew's ffmpeg builds and pulls in on the macOS runner.
  screens-ffmpeg = pkgs.ffmpeg-headless.override {
    withHeadlessDeps = false;
    withGPL = true;
    withX264 = true;
    withZlib = true;
    # Its own tests build against it.
    withPixelutils = true;
    withMultithread = true;
    buildFfmpeg = true;
    buildFfprobe = true;
    buildAvcodec = true;
    buildAvformat = true;
    buildAvfilter = true;
    buildAvutil = true;
    buildSwscale = true;
    buildSwresample = true;
    # Empty with no device on, but nixpkgs links its tools against it.
    buildAvdevice = true;
  };

  # XcodeGen, which writes showcase/ios's Xcode project for the
  # screens workflow: upstream's own release, a universal macOS binary
  # and its presets, pinned here, as nixpkgs 24.11 has none.
  xcodegen = pkgs.stdenvNoCC.mkDerivation rec {
    pname = "xcodegen";
    version = "2.46.0";
    src = pkgs.fetchzip {
      url = "https://github.com/yonaskolb/XcodeGen/releases/download/${version}/xcodegen.zip";
      hash = "sha256-7NjCsInBjkOL1RqWSaWKGDJxp/gn71rmhTWG2ErHDqA=";
    };
    # Signed as it is: stripping or patching it would break that.
    dontFixup = true;
    installPhase = "mkdir -p $out && cp -r bin share $out/";
    meta.platforms = [ "aarch64-darwin" "x86_64-darwin" ];
  };
}
