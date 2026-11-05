# `nix run .#<app>-demo [ADDR]`: an app's server on the demo's data, for a
# person to try: a fresh directory under the temp dir seeded by
# `<app>-seed`, then the server on it with dev auth (no provider, anyone is
# whoever they say, said loudly at startup), as the screenshots run it
# (`<app>/screens/server.sh`). The directory is left behind, and its path
# said, so a second run starts from the seed again rather than from what the
# first one did.
{ writeShellApplication, coreutils, name, seed, server, listen, env }:
writeShellApplication {
  name = "${name}-demo";
  runtimeInputs = [ coreutils seed server ];
  text = ''
    dir=$(mktemp -d -t ${name}-demo.XXXXXX)
    ${name}-seed "$dir"
    echo "${name}-demo: seeded in $dir" >&2
    ${env} exec ${name}-server "''${1:-${listen}}"
  '';
}
