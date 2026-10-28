# The voices program (../voices): who says each line of an episode's
# captions, with sherpa-onnx and 3D-Speaker's ERes2Net (VoxCeleb, 26 MB).
# A nixpkgs with python3Packages.sherpa-onnx — the host's, as the module
# calls it; hear-here calls it with the one on its machine.
{
  writeShellApplication,
  python3,
  ffmpeg-headless,
  fetchurl,
}:
let
  model = fetchurl {
    url = "https://github.com/k2-fsa/sherpa-onnx/releases/download/speaker-recongition-models/3dspeaker_speech_eres2net_sv_en_voxceleb_16k.onnx";
    sha256 = "1d9dbhlsxfm7syvll8p3i7im2bfmz1mcm737c7hndbamj8vmi4f5";
  };
  python = python3.withPackages (p: [
    p.numpy
    p.scipy
    p.sherpa-onnx
  ]);
in
writeShellApplication {
  name = "harken-voices";
  runtimeInputs = [ ffmpeg-headless ];
  text = ''
    HARKEN_VOICES_MODEL=''${HARKEN_VOICES_MODEL:-${model}} exec ${python.interpreter} ${../voices} "$@"
  '';
}
