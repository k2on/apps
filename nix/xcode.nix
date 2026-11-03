# The Xcode every iPhone crate is compiled with (flake.nix, `iosPhone`): its
# version as `xcodebuild -version` says it, at the path the runner has it
# (.github/workflows/ios-nix.yaml checks it is there). Part of each crate's
# derivation, so a crate in the cache is never another Xcode's.
{
  version = "26.3";
  developerDir = "/Applications/Xcode_26.3.app/Contents/Developer";
}
