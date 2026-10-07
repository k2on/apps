# The NixOS module, evaluated under the configurations that matter and held
# to what it must say — its assertions, its warning, and the unit it writes —
# without building or booting a system:
#
#   checks.harken-module = pkgs.callPackage ./harken/server/nix/module-test.nix {
#     module = import ./harken/server/nix/module.nix { packages = self.packages; };
#   };
{ lib, path, runCommand, hello, stdenv, module }:
let
  eval = settings: (import "${path}/nixos/lib/eval-config.nix" {
    system = stdenv.hostPlatform.system;
    modules = [
      module
      {
        boot.isContainer = true;
        system.stateVersion = "24.11";
        # Any package will do: nothing here runs it.
        services.harken = { enable = true; package = hello; } // settings;
      }
    ];
  }).config;
  failing = c: map (a: a.message) (lib.filter (a: !a.assertion) c.assertions);
  unit = c: c.systemd.services.harken;
  env = c: (unit c).environment;
  says = c: needle: lib.any (m: lib.hasInfix needle m) (failing c);
  warns = c: needle: lib.any (w: lib.hasInfix needle w) c.warnings;

  dev = eval { devAuth = true; };
  nobody = eval { };
  oidc = eval {
    oidc = { issuer = "https://auth.example.com"; clientId = "harken"; clientSecretFile = "/run/secrets/oidc"; };
    publicUrl = "https://harken.example.com";
  };
  house = extra: eval ({
    devAuth = true;
    homeAssistant = { url = "http://ha:8123"; tokenFile = "/run/secrets/ha"; players = [ "media_player.kitchen" ]; } // extra;
  });
  loopbackSpeaker = house { };
  loopbackMedia = house { mediaUrl = "http://127.0.0.1:8787"; };
  lanUnbound = house { mediaUrl = "http://10.0.0.2:8787"; };
  bound = eval {
    devAuth = true;
    address = "0.0.0.0";
    homeAssistant = { url = "http://ha:8123"; tokenFile = "/run/secrets/ha"; players = [ "media_player.kitchen" ]; mediaUrl = "http://10.0.0.2:8787"; };
  };
  noMedia = eval { devAuth = true; mediaPath = null; };
  roles = eval { devAuth = true; roles = { library = [ "alice" "bob" ]; admin = [ "alice" ]; }; };
  oldModules = eval { devAuth = true; oldModules = [ "/srv/old/a.ark" "/srv/old/b.ark" ]; };

  checks = [
    [ (says nobody "nobody could sign in") "a server nobody can sign in to is refused" ]
    [ (failing dev == [ ]) "dev auth evaluates" ]
    [ ((env dev).HARKEN_DEV_AUTH == "1") "dev auth reaches the server" ]
    [ ((env dev).HARKEN_DATA == "%S/harken") "the state is in the state directory" ]
    [ (lib.hasSuffix "/bin/harken-server 127.0.0.1:8787" (unit dev).serviceConfig.ExecStart) "it listens where it is told" ]
    [ (lib.elem "d /srv/media/music 0755 root root -" dev.systemd.tmpfiles.rules) "the media directory is made, with music/ in it" ]
    [ ((unit dev).serviceConfig.BindReadOnlyPaths == [ "/srv/media" ]) "the media is bound read-only" ]
    [ ((env dev).HARKEN_MEDIA == "/srv/media") "and scanned" ]
    [ (failing oidc == [ ]) "a provider evaluates" ]
    [ ((env oidc).HARKEN_OIDC_CLIENT_SECRET_FILE == "%d/oidc-secret") "the secret is a credential, by file" ]
    [ ((unit oidc).serviceConfig.LoadCredential == [ "oidc-secret:/run/secrets/oidc" ]) "loaded from where it lives" ]
    [ (!(env oidc ? HARKEN_DEV_AUTH)) "and dev auth is off" ]
    [ (says loopbackSpeaker "talking to itself") "a speaker handed the default loopback URL is refused" ]
    [ (says loopbackMedia "talking to itself") "a speaker handed an explicit loopback mediaUrl is refused" ]
    [ (failing lanUnbound == [ ] && warns lanUnbound "answers on this machine and nowhere else") "a LAN mediaUrl on a loopback bind is a warning" ]
    [ ((env lanUnbound).HARKEN_HA_TOKEN_FILE == "%d/ha-token" && lib.elem "ha-token:/run/secrets/ha" (unit lanUnbound).serviceConfig.LoadCredential) "the house's token is a credential" ]
    [ ((env lanUnbound).HARKEN_HA_MEDIA == "http://10.0.0.2:8787") "and speakers fetch from the LAN" ]
    [ (failing bound == [ ] && bound.warnings == [ ]) "bound to every interface, nothing to warn about" ]
    [ (!(lib.any (lib.hasInfix "/srv/media") noMedia.systemd.tmpfiles.rules) && !(env noMedia ? HARKEN_MEDIA) && (unit noMedia).serviceConfig.BindReadOnlyPaths == [ ]) "no media path, no media" ]
    [ ((env roles).HARKEN_ROLES == "admin=alice;library=alice,bob") "roles reach the server, role by role" ]
    [ (!(env dev ? HARKEN_ROLES)) "and none, nothing" ]
    [ ((env oldModules).HARKEN_OLD_MODULES == "/srv/old/a.ark,/srv/old/b.ark") "the old modules reach the server" ]
    [ (!(env dev ? HARKEN_OLD_MODULES)) "and none, nothing" ]
  ];
  wrong = map (c: builtins.elemAt c 1) (lib.filter (c: !(builtins.elemAt c 0)) checks);
in
assert lib.assertMsg (wrong == [ ]) "services.harken: ${lib.concatStringsSep "; " wrong}";
runCommand "harken-module-test" { } ''
  echo ${toString (builtins.length checks)} checks > $out
''
