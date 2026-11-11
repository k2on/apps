# `nix run .#<app>-demo [ADDR]`: an app's server on the demo's data, for a
# person to try: a fresh directory under the temp dir seeded by
# `<app>-seed`, then the server on it with dev auth (no provider, anyone is
# whoever they say, said loudly at startup), as the screenshots run it
# (`<app>/demo/server.sh`). The directory is left behind, and its path
# said, so a second run starts from the seed again rather than from what the
# first one did.
{ writeShellApplication, coreutils, name, seed, server, listen, env, seedArgs ? "" }:
writeShellApplication {
  name = "${name}-demo";
  runtimeInputs = [ coreutils seed server ];
  text = ''
    dir=$(mktemp -d -t ${name}-demo.XXXXXX)
    ${name}-seed ${seedArgs} "$dir"
    echo "${name}-demo: seeded in $dir" >&2
    # Signed in as the demo's person, alex, wearing their picture
    # (nix/demo/README.md), with no page to fill in.
    ARK_DEV_USER=alex ARK_DEV_PICTURE=${./demo/alex.jpg} ${env} exec ${name}-server "''${1:-${listen}}"
  '';
}
