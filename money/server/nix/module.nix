# The NixOS service: `services.money.enable = true` and the household's
# budget is on a port — the sync socket, the sign-in routes and the browser
# client together, as harken's are — with the bank importer inside it.
#
# A function of the flake's packages, so the defaults name this repository's
# builds:
#
#   nixosModules.money = import ./money/server/nix/module.nix { packages = self.packages; };
#
# Every secret of the deployment's is a file, handed to the service as a
# systemd credential and read by the server from `%d` (its credentials
# directory): the OpenID Connect client secret, and Plaid's client id and
# secret. None of them is in the store, the unit, the environment or the
# log.
#
# The banks are not configuration. The household connects them from the
# app, and the server keeps each one's login in its data directory
# (`banks.json`, mode 0600, beside the log): back up the whole of
# {option}`services.money.dataDir` and a restore keeps every connection.
{ packages }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.money;
  money = packages.${pkgs.stdenv.hostPlatform.system} or { };
  plaid = cfg.plaid;
  importing = plaid.clientIdFile != null || plaid.secretFile != null;
  amazon = cfg.amazon;
  # Every server watches its receipts folder, so its importer always runs.
  syncing = true;
  state = lib.removePrefix "/var/lib/" cfg.dataDir;
  moved = ''
    Banks are connected from the app now, and each one's login is kept in
    the server's data directory (banks.json): see money/server/src/store.rs
    for moving an existing connection there by hand, then remove this.
  '';
in
{
  imports = [
    (lib.mkRemovedOptionModule [ "services" "money" "plaid" "items" ] moved)
    (lib.mkRemovedOptionModule [ "services" "money" "accounts" ] ''
      Which budget account each bank account feeds is chosen in the app,
      in Settings, under Banks, when the bank is connected. ${moved}
    '')
  ];

  options.services.money = {
    enable = lib.mkEnableOption "money, the household's budget: its sync server, browser client and bank importer";

    package = lib.mkOption {
      type = lib.types.package;
      default = money.money-server;
      defaultText = lib.literalExpression "apps.packages.\${system}.money-server";
      description = "The server to run.";
    };

    web = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = money.money-web-server or null;
      defaultText = lib.literalExpression "apps.packages.\${system}.money-web-server";
      description = ''
        The browser client to serve at `/`: the build that signs in against
        the server serving it (`money-web-server`, not the demo
        `money-web`). Null serves the socket alone.
      '';
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8788;
      description = "Port for the sync socket, the sign-in routes and the browser client.";
    };

    address = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = ''
        Address to bind. The default is loopback, for a reverse proxy in front
        that terminates TLS: the login sends a browser here and back, and a
        session token crosses on every connect, neither of which belongs on
        plain HTTP off the machine.
      '';
    };

    publicUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://${cfg.address}:${toString cfg.port}";
      defaultText = lib.literalExpression ''"http://''${address}:''${toString port}"'';
      example = "https://money.example.com";
      description = ''
        Where a browser reaches this server: the identity provider sends
        people back to `''${publicUrl}/auth/callback`, which is the one
        redirect URI to register with it. Behind a reverse proxy it is the
        proxy's address.
      '';
    };

    oidc = lib.mkOption {
      default = null;
      description = ''
        The OpenID Connect provider people sign in through; whoever it lets in
        is the household and shares the one budget. The server is the only
        OpenID Connect client (a confidential one: it holds the secret) and
        hands each signed-in peer a session of its own, so the desktop, the
        browser and the phones need nothing but this server's address — a
        phone signs in through `/auth/login` and is sent back to `money://`.
        Null runs no login at all, which is refused unless
        {option}`services.money.devAuth` says a laptop.
      '';
      type = lib.types.nullOr (lib.types.submodule {
        options = {
          issuer = lib.mkOption {
            type = lib.types.str;
            example = "https://auth.example.com/oauth2/openid/money";
            description = "The issuer URL, exactly as the provider states it.";
          };
          clientId = lib.mkOption {
            type = lib.types.str;
            example = "money";
            description = "The client id the provider knows this server as.";
          };
          clientSecretFile = lib.mkOption {
            type = lib.types.path;
            example = "/run/secrets/money-oidc";
            description = ''
              A file holding the client secret and nothing else, loaded as a
              systemd credential. It has to exist when the unit starts.
            '';
          };
          scopes = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ "openid" "profile" "email" ];
            description = "What to ask the provider for. `openid` is required.";
          };
        };
      });
    };

    devAuth = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = ''
        Sign anyone in as whatever name they give, with no provider. What a
        laptop does; anywhere else it means anyone can write as anyone.
      '';
    };

    redirects = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "https://money-web.example.com/" ];
      description = ''
        Where a login may send its code back to, besides what every server
        allows: a loopback port (the desktop client), `money://` (the
        phones), and {option}`services.money.publicUrl` (the browser client
        it serves).
      '';
    };

    roles = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.str);
      default = { };
      description = ''
        Who holds which role, as account ids. The one role, `bank`, is the
        importer's by construction and nobody else's unless named here; only
        `banks.import` asks for it.
      '';
    };

    plaid = {
      env = lib.mkOption {
        type = lib.types.enum [ "production" "sandbox" ];
        default = "production";
        description = "Plaid environment. sandbox is for trying the importer with Plaid's fake banks.";
      };

      clientIdFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = "/run/secrets/money_plaid_client_id";
        description = "A file holding Plaid's client id. With secretFile; neither runs no importer.";
      };

      secretFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        example = "/run/secrets/money_plaid_secret";
        description = "A file holding Plaid's secret for {option}`services.money.plaid.env`.";
      };

      history = lib.mkOption {
        type = lib.types.ints.between 1 730;
        default = 90;
        description = "Days of history asked of Plaid the first time a bank is pulled.";
      };
    };

    amazon = {
      package = lib.mkOption {
        type = lib.types.nullOr lib.types.package;
        default = null;
        description = ''
          The amazon-orders program (its Python package), which the importer
          runs at each sync to read Amazon's orders as receipts. Null imports
          none from Amazon.
        '';
      };
      args = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "orders" "--last-30-days" "--full-details" "-o" "json" ];
        description = "What amazon-orders is run with: `--year 2026` in place of `--last-30-days` for a first pull.";
      };
      transactionsArgs = lib.mkOption {
        type = lib.types.listOf lib.types.str;
        default = [ "transactions" "--days" "60" "-o" "json" ];
        description = "What amazon-orders is run with for how each order was charged: shipment by shipment, and refunds.";
      };
      usernameFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "A file holding the Amazon account's email. Null (and passwordFile null): the login given in the app's Settings.";
      };
      passwordFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "A file holding the Amazon account's password.";
      };
      otpSecretFile = lib.mkOption {
        type = lib.types.nullOr lib.types.path;
        default = null;
        description = "A file holding the account's one-time-code secret, when it has two-step verification.";
      };
    };

    # Costco is read in the browser below, signed in to there by a person.
    costco.days = lib.mkOption {
      type = lib.types.ints.positive;
      default = 90;
      description = "How many days back Costco's warehouse and gas receipts are asked for at each sync.";
    };

    browser = {
      devtools = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "http://192.168.77.2:9223";
        description = ''
          A headed browser's DevTools address, which the server drives: it
          reads Costco's receipts on costco.com's own page there, in the
          sign-in a person made, and amazon-orders runs Amazon's JavaScript
          sign-in challenge in it. Null reads nothing from Costco.
        '';
      };
      page = lib.mkOption {
        type = lib.types.nullOr lib.types.str;
        default = null;
        example = "https://sivrad.example.com/browser";
        description = "The page where people see that browser and sign in to costco.com, which Settings opens.";
      };
    };

    receiptsDir = lib.mkOption {
      type = lib.types.nullOr lib.types.str;
      default = null;
      example = "/var/lib/money/receipts";
      description = ''
        A folder of stores' exports the importer reads at each sync:
        `target/*.json`, `costco/*.json`, `amazon/*.json`, each moved to
        `imported/` (or `failed/`) beside it once read.
      '';
    };

    schedule = lib.mkOption {
      type = lib.types.str;
      default = "*-*-* 06,18:00:00";
      description = ''
        systemd OnCalendar expression for the import, twice a day by default:
        a timer that sends the running server `SIGUSR1`. It also imports
        once at every start.
      '';
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/money";
      description = ''
        Everything the server keeps: the log (the budget and the
        sign-ins), the session key, the system's events, and `banks.json`
        — every bank the household connected, its Plaid login (access
        token) and its import cursor, readable by the service alone. Back
        all of it up: a restore of this directory, here or on another
        machine with the same Plaid keys, keeps every bank connected, and
        without `banks.json` every bank has to be connected again. Under
        `/var/lib`: it is the service's StateDirectory, owned by its
        dynamic user (`/var/lib/private/money` on disk), so a file put
        there by hand takes that owner and mode 0600.
      '';
    };

    retainEntries = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 10000;
      description = "Entries of the log kept above its snapshot.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open {option}`services.money.port` in the firewall.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.oidc != null || cfg.devAuth;
        message = ''
          services.money: nobody could sign in. Set services.money.oidc to an
          OpenID Connect provider, or services.money.devAuth = true on a
          machine nobody else can reach.
        '';
      }
      {
        assertion = (plaid.clientIdFile == null) == (plaid.secretFile == null);
        message = "services.money.plaid: clientIdFile and secretFile go together; set both.";
      }
      {
        assertion = lib.hasPrefix "/var/lib/" cfg.dataDir && state != "";
        message = "services.money.dataDir is the service's state directory, under /var/lib: not ${cfg.dataDir}.";
      }
    ] ++ [
      {
        assertion = (amazon.usernameFile == null) == (amazon.passwordFile == null);
        message = "services.money.amazon: usernameFile and passwordFile go together; set both, or neither and give the login in the app's Settings.";
      }
    ];

    systemd.services.money = {
      description = "money, the household's budget";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];

      environment = {
        MONEY_DATA = cfg.dataDir;
        MONEY_PUBLIC_URL = cfg.publicUrl;
        MONEY_REDIRECTS = lib.concatStringsSep "," cfg.redirects;
        MONEY_RETAIN_ENTRIES = toString cfg.retainEntries;
      } // lib.optionalAttrs (cfg.roles != { }) {
        MONEY_ROLES = lib.concatStringsSep ";"
          (lib.mapAttrsToList (role: ids: "${role}=${lib.concatStringsSep "," ids}") cfg.roles);
      } // lib.optionalAttrs (cfg.web != null) {
        MONEY_WEB = "${cfg.web}";
      } // lib.optionalAttrs (cfg.oidc != null) {
        MONEY_OIDC_ISSUER = cfg.oidc.issuer;
        MONEY_OIDC_CLIENT_ID = cfg.oidc.clientId;
        MONEY_OIDC_SCOPES = lib.concatStringsSep " " cfg.oidc.scopes;
        # systemd copies each credential into a directory only this service
        # can read; `%d` is that directory.
        MONEY_OIDC_CLIENT_SECRET_FILE = "%d/oidc-secret";
      } // lib.optionalAttrs cfg.devAuth {
        MONEY_DEV_AUTH = "1";
      } // lib.optionalAttrs importing {
        MONEY_PLAID_ENV = plaid.env;
        MONEY_PLAID_CLIENT_ID_FILE = "%d/plaid-client-id";
        MONEY_PLAID_SECRET_FILE = "%d/plaid-secret";
        MONEY_PLAID_HISTORY = toString plaid.history;
      } // lib.optionalAttrs (amazon.package != null) {
        MONEY_AMAZON_ORDERS = "${amazon.package}/bin/amazon-orders";
        MONEY_AMAZON_ARGS = lib.concatStringsSep " " amazon.args;
        MONEY_AMAZON_TRANSACTIONS_ARGS = lib.concatStringsSep " " amazon.transactionsArgs;
      } // lib.optionalAttrs (amazon.package != null && amazon.usernameFile != null) {
        MONEY_AMAZON_USERNAME_FILE = "%d/amazon-username";
        MONEY_AMAZON_PASSWORD_FILE = "%d/amazon-password";
      } // lib.optionalAttrs (amazon.package != null && amazon.otpSecretFile != null) {
        MONEY_AMAZON_OTP_SECRET_FILE = "%d/amazon-otp-secret";
      } // {
        MONEY_COSTCO_DAYS = toString cfg.costco.days;
      } // lib.optionalAttrs (cfg.browser.devtools != null) {
        MONEY_BROWSER = cfg.browser.devtools;
      } // lib.optionalAttrs (cfg.browser.page != null) {
        MONEY_BROWSER_PAGE = cfg.browser.page;
      } // lib.optionalAttrs (cfg.receiptsDir != null) {
        MONEY_RECEIPTS_DIR = cfg.receiptsDir;
      };

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/money-server ${cfg.address}:${toString cfg.port}";
        Restart = "on-failure";
        LoadCredential =
          lib.optional (cfg.oidc != null) "oidc-secret:${cfg.oidc.clientSecretFile}"
          ++ lib.optionals importing [
            "plaid-client-id:${plaid.clientIdFile}"
            "plaid-secret:${plaid.secretFile}"
          ]
          ++ lib.optionals (amazon.package != null && amazon.usernameFile != null) [
            "amazon-username:${amazon.usernameFile}"
            "amazon-password:${amazon.passwordFile}"
          ]
          ++ lib.optional (amazon.package != null && amazon.otpSecretFile != null) "amazon-otp-secret:${amazon.otpSecretFile}";

        DynamicUser = true;
        StateDirectory = state;
        StateDirectoryMode = "0700";

        # Nothing here needs any of it.
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectSystem = "strict";
        # amazon-orders' browser challenge runs Playwright's Node driver,
        # which talks to it over Unix sockets.
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ] ++ lib.optional (amazon.package != null) "AF_UNIX";
        RestrictNamespaces = true;
        RestrictRealtime = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
      };
    };

    # The schedule: the running server told to import now. A signal rather
    # than a second process, so the importer stays the one peer that holds
    # the bank's role and the cursors. Persistent: a run missed while the
    # machine was off is sent at boot, and the server imports at every
    # start anyway.
    systemd.services.money-import = lib.mkIf syncing {
      description = "Ask money's server to import from the banks now";
      serviceConfig = {
        Type = "oneshot";
        # `-`: a server not running now imports when it starts.
        ExecStart = "-${config.systemd.package}/bin/systemctl kill --signal=SIGUSR1 --kill-whom=main money.service";
      };
    };
    systemd.timers.money-import = lib.mkIf syncing {
      wantedBy = [ "timers.target" ];
      timerConfig = {
        OnCalendar = cfg.schedule;
        Persistent = true;
        RandomizedDelaySec = "10m";
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
  };
}
