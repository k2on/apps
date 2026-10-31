# The NixOS module, evaluated under the configurations that matter and held
# to what it must say — its assertions and the units it writes — without
# building or booting a system, as harken's module test is:
#
#   checks.money-module = pkgs.callPackage ./money/server/nix/module-test.nix {
#     module = import ./money/server/nix/module.nix { packages = self.packages; };
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
        services.money = { enable = true; package = hello; web = null; } // settings;
      }
    ];
  }).config;
  failing = c: map (a: a.message) (lib.filter (a: !a.assertion) c.assertions);
  unit = c: c.systemd.services.money;
  env = c: (unit c).environment;
  says = c: needle: lib.any (m: lib.hasInfix needle m) (failing c);

  oidc = { issuer = "https://auth.example.com"; clientId = "money"; clientSecretFile = "/run/secrets/oidc"; };
  keys = { clientIdFile = "/run/secrets/plaid-id"; secretFile = "/run/secrets/plaid-secret"; };
  dev = eval { devAuth = true; };
  nobody = eval { };
  signedIn = eval { inherit oidc; publicUrl = "https://money.example.com"; };
  banks = eval {
    inherit oidc;
    plaid = keys // { env = "sandbox"; history = 30; };
  };
  halfKeys = eval { devAuth = true; plaid.clientIdFile = "/run/secrets/plaid-id"; };
  # What a server was told before its banks were connected from the app.
  oldItems = eval { devAuth = true; plaid = keys // { items.chase.accessTokenFile = "/run/secrets/plaid-chase"; }; };
  oldAccounts = eval { devAuth = true; plaid = keys; accounts."Chase Checking" = { item = "chase"; mask = "1234"; }; };
  elsewhere = eval { devAuth = true; dataDir = "/srv/money"; };
  daily = eval { devAuth = true; plaid = keys; schedule = "daily"; };

  checks = [
    [ (says nobody "nobody could sign in") "a server nobody can sign in to is refused" ]
    [ (failing dev == [ ]) "dev auth evaluates" ]
    [ ((env dev).MONEY_DEV_AUTH == "1") "dev auth reaches the server" ]
    [ ((env dev).MONEY_DATA == "/var/lib/money" && (unit dev).serviceConfig.StateDirectory == "money") "the state is the state directory" ]
    [ ((unit dev).serviceConfig.DynamicUser) "as a dynamic user" ]
    [ (lib.hasSuffix "/bin/money-server 127.0.0.1:8788" (unit dev).serviceConfig.ExecStart) "it listens on loopback by default" ]
    [ (!(env dev ? MONEY_PLAID_CLIENT_ID_FILE) && !(dev.systemd.timers ? money-import)) "no keys, no importer and no timer" ]
    [ (failing signedIn == [ ]) "a provider evaluates" ]
    [ ((env signedIn).MONEY_OIDC_CLIENT_SECRET_FILE == "%d/oidc-secret") "the client secret is a credential, by file" ]
    [ ((unit signedIn).serviceConfig.LoadCredential == [ "oidc-secret:/run/secrets/oidc" ]) "loaded from where it lives" ]
    [ ((env signedIn).MONEY_PUBLIC_URL == "https://money.example.com" && !(env signedIn ? MONEY_DEV_AUTH)) "the public URL reaches the server, and dev auth is off" ]
    [ (failing banks == [ ]) "banks evaluate" ]
    [ ((env banks).MONEY_PLAID_ENV == "sandbox" && (env banks).MONEY_PLAID_HISTORY == "30") "the environment and the history reach the server" ]
    [ ((env banks).MONEY_PLAID_CLIENT_ID_FILE == "%d/plaid-client-id" && (env banks).MONEY_PLAID_SECRET_FILE == "%d/plaid-secret") "Plaid's keys are credentials" ]
    [ ((unit banks).serviceConfig.LoadCredential == [ "oidc-secret:/run/secrets/oidc" "plaid-client-id:/run/secrets/plaid-id" "plaid-secret:/run/secrets/plaid-secret" ]) "loaded from where they live" ]
    [ (!(env banks ? MONEY_PLAID_ITEMS) && !(env banks ? MONEY_ACCOUNTS)) "and no bank is configuration: they are the data directory's" ]
    [ (!(lib.any (v: lib.hasInfix "/run/secrets" v) (lib.attrValues (env banks)))) "no secret's path is in the environment, only the credentials'" ]
    [ (banks.systemd.timers.money-import.timerConfig.OnCalendar == "*-*-* 06,18:00:00") "twice a day by default" ]
    [ (lib.hasInfix "kill --signal=SIGUSR1 --kill-whom=main money.service" banks.systemd.services.money-import.serviceConfig.ExecStart) "by telling the server" ]
    [ (daily.systemd.timers.money-import.timerConfig.OnCalendar == "daily") "or when told" ]
    [ (says halfKeys "go together") "half of Plaid's keys is refused" ]
    [ (says oldItems "plaid.items" && says oldItems "banks.json") "banks still configured are refused, saying where they live now" ]
    [ (says oldAccounts "services.money.accounts" && says oldAccounts "Banks page") "accounts still configured are refused, saying where they are chosen now" ]
    [ (says elsewhere "under /var/lib") "the data outside the state directory is refused" ]
  ];
  wrong = map (c: builtins.elemAt c 1) (lib.filter (c: !(builtins.elemAt c 0)) checks);
in
assert lib.assertMsg (wrong == [ ]) "services.money: ${lib.concatStringsSep "; " wrong}";
runCommand "money-module-test" { } ''
  echo ${toString (builtins.length checks)} checks > $out
''
