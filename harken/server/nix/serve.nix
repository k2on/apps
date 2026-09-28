# `nix run .#harken-serve [ADDR]`: a dev server — no provider, anyone is
# whoever they say, said loudly at startup — keeping its state in the temp
# dir. Pass 0.0.0.0:8787 to reach it from a phone on the same network. The
# NixOS module is the one that signs people in for real.
{ writeShellApplication, harken-server }:
writeShellApplication {
  name = "harken-serve";
  runtimeInputs = [ harken-server ];
  text = ''HARKEN_DEV_AUTH=1 exec harken-server "''${1:-127.0.0.1:8787}"'';
}
