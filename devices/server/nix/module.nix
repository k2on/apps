# The NixOS service: `services.devices.enable = true` and the household's
# managed phones have their server on a port — the sync socket and the
# sign-in routes — holding the policies declared here.
#
# A function of the flake's packages, so the defaults name this repository's
# builds:
#
#   nixosModules.devices = import ./devices/server/nix/module.nix { packages = self.packages; };
#
# Policies and who each applies to are configuration
# ({option}`services.devices.policies`, {option}`services.devices.assignments`):
# written to a file the server reads at every start and holds the store to,
# so a deploy that changes them changes every phone, the ones not connected
# woken by push. The OpenID Connect client secret is a file, handed to the
# service as a systemd credential.
{ packages }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.devices;
  devices = packages.${pkgs.stdenv.hostPlatform.system} or { };
  state = lib.removePrefix "/var/lib/" cfg.dataDir;

  settings = attrs: lib.mapAttrsToList (name: on: { inherit name on; }) attrs;
  # A policy as devices-core's `Policy` reads it (serde's names).
  policyJson = name: p: {
    inherit name;
    vpn = p.vpn.mode;
    vpn_app = p.vpn.app;
    vpn_lockdown = p.vpn.lockdown;
    restrictions = settings p.restrictions;
    uninstall_blocked = settings p.uninstallBlocked;
    user_control_disabled = settings p.userControlDisabled;
    permissions = map (r: { inherit (r) app permission grant; }) p.permissions;
    camera_disabled = p.cameraDisabled;
    screen_capture_disabled = p.screenCaptureDisabled;
  };
  declared = pkgs.writeText "devices-policies.json" (builtins.toJSON {
    policies = lib.mapAttrsToList policyJson cfg.policies;
    inherit (cfg) assignments;
  });

  policy = lib.types.submodule {
    options = {
      vpn = {
        mode = lib.mkOption {
          type = lib.types.enum [ "Unmanaged" "AlwaysOn" "Off" ];
          default = "Unmanaged";
          description = ''
            The always-on VPN: left as the phone has it, always on through
            {option}`app`, or none. Only `Off` takes one away.
          '';
        };
        app = lib.mkOption {
          type = lib.types.str;
          default = "";
          example = "com.tailscale.ipn";
          description = "The VPN app's package, for `AlwaysOn`.";
        };
        lockdown = lib.mkOption {
          type = lib.types.bool;
          default = true;
          description = "Nothing leaves the phone but through the VPN.";
        };
      };
      restrictions = lib.mkOption {
        type = lib.types.attrsOf lib.types.bool;
        default = { };
        example = { no_config_vpn = true; no_factory_reset = true; no_debugging_features = true; };
        description = ''
          Android user restrictions by name: true on, false off; one not named
          is left as the phone has it.
        '';
      };
      uninstallBlocked = lib.mkOption {
        type = lib.types.attrsOf lib.types.bool;
        default = { };
        example = { "com.tailscale.ipn" = true; };
        description = "Apps by package that cannot be uninstalled (true) or can again (false).";
      };
      userControlDisabled = lib.mkOption {
        type = lib.types.attrsOf lib.types.bool;
        default = { };
        example = { "com.tailscale.ipn" = true; "us.koon.devices" = true; };
        description = ''
          Apps by package the person cannot force-stop or clear (Android 11
          and later): the VPN, the push distributor and devices itself, whose
          jobs a force stop would cancel.
        '';
      };
      permissions = lib.mkOption {
        type = lib.types.listOf (lib.types.submodule {
          options = {
            app = lib.mkOption { type = lib.types.str; };
            permission = lib.mkOption { type = lib.types.str; };
            grant = lib.mkOption {
              type = lib.types.enum [ "Default" "Granted" "Denied" ];
              default = "Granted";
            };
          };
        });
        default = [ ];
        description = "What apps are given of runtime permissions; one not named is the person's.";
      };
      cameraDisabled = lib.mkOption {
        type = lib.types.nullOr lib.types.bool;
        default = null;
        description = "The cameras, every app's; null leaves them.";
      };
      screenCaptureDisabled = lib.mkOption {
        type = lib.types.nullOr lib.types.bool;
        default = null;
        description = "Screenshots and recording, every app's; null leaves them.";
      };
    };
  };
in
{
  options.services.devices = {
    enable = lib.mkEnableOption "devices, the household's managed phones: their server";

    package = lib.mkOption {
      type = lib.types.package;
      default = devices.devices-server;
      defaultText = lib.literalExpression "apps.packages.\${system}.devices-server";
      description = "The server to run.";
    };

    port = lib.mkOption {
      type = lib.types.port;
      default = 8790;
      description = "Port for the sync socket and the sign-in routes.";
    };

    address = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      description = ''
        Address to bind: loopback, for a reverse proxy in front that
        terminates TLS and passes the WebSocket upgrade through.
      '';
    };

    publicUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://${cfg.address}:${toString cfg.port}";
      defaultText = lib.literalExpression ''"http://''${address}:''${toString port}"'';
      example = "https://devices.example.com";
      description = ''
        Where a phone reaches this server; the identity provider sends people
        back to `''${publicUrl}/auth/callback`.
      '';
    };

    oidc = lib.mkOption {
      default = null;
      description = ''
        The OpenID Connect provider people sign in through. Null runs no login
        at all, which is refused unless {option}`services.devices.devAuth`.
      '';
      type = lib.types.nullOr (lib.types.submodule {
        options = {
          issuer = lib.mkOption {
            type = lib.types.str;
            description = "The issuer URL, exactly as the provider states it.";
          };
          clientId = lib.mkOption {
            type = lib.types.str;
            description = "The client id the provider knows this server as.";
          };
          clientSecretFile = lib.mkOption {
            type = lib.types.path;
            description = "A file holding the client secret, loaded as a systemd credential.";
          };
          scopes = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            default = [ "openid" "profile" "email" ];
            description = "What to ask the provider for.";
          };
        };
      });
    };

    devAuth = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Sign anyone in as whatever name they give: a laptop's, never a server's.";
    };

    redirects = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      description = "Where a login may send its code back to, besides `devices://` and the public URL.";
    };

    roles = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.str);
      default = { };
      example = { admin = [ "max" ]; };
      description = ''
        Roles granted to people by their user id: `admin` sees every phone's
        report. The server's own `config` account holds `admin` regardless.
      '';
    };

    policies = lib.mkOption {
      type = lib.types.attrsOf policy;
      default = { };
      example = lib.literalExpression ''
        {
          kids = {
            vpn = { mode = "AlwaysOn"; app = "com.tailscale.ipn"; };
            restrictions = { no_config_vpn = true; no_factory_reset = true; };
            uninstallBlocked."com.tailscale.ipn" = true;
            userControlDisabled = { "com.tailscale.ipn" = true; "us.koon.devices" = true; };
          };
        }
      '';
      description = ''
        What phones are held to, by name. The server holds its store to
        exactly these at every start: a policy removed here is removed there.
      '';
    };

    assignments = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      example = { ann = "kids"; };
      description = ''
        Which policy applies to each person's phones, by their user id; a
        person not named here is held to none (and their phones left as they
        are).
      '';
    };

    dataDir = lib.mkOption {
      type = lib.types.str;
      default = "/var/lib/devices";
      description = ''
        Everything the server keeps: the log (policies, reports, the
        sign-ins) and the session key. Under `/var/lib`: the service's
        StateDirectory.
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
      description = "Open {option}`services.devices.port` in the firewall.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.oidc != null || cfg.devAuth;
        message = "services.devices: nobody could sign in. Set services.devices.oidc, or devAuth on a machine nobody else can reach.";
      }
      {
        assertion = lib.hasPrefix "/var/lib/" cfg.dataDir && state != "";
        message = "services.devices.dataDir is the service's state directory, under /var/lib: not ${cfg.dataDir}.";
      }
      {
        assertion = lib.all (p: lib.elem p (lib.attrNames cfg.policies)) (lib.attrValues cfg.assignments);
        message = "services.devices.assignments: every policy assigned is one of services.devices.policies.";
      }
      {
        assertion = lib.all (p: p.vpn.mode != "AlwaysOn" || p.vpn.app != "") (lib.attrValues cfg.policies);
        message = "services.devices.policies: an AlwaysOn VPN names its app (vpn.app).";
      }
    ];

    systemd.services.devices = {
      description = "devices, the household's managed phones";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [ "network-online.target" ];
      # A change of policies is a restart, which holds the store to them.
      restartTriggers = [ declared ];

      environment = {
        DEVICES_DATA = cfg.dataDir;
        DEVICES_PUBLIC_URL = cfg.publicUrl;
        DEVICES_REDIRECTS = lib.concatStringsSep "," cfg.redirects;
        DEVICES_RETAIN_ENTRIES = toString cfg.retainEntries;
        DEVICES_POLICIES = "${declared}";
      } // lib.optionalAttrs (cfg.roles != { }) {
        DEVICES_ROLES = lib.concatStringsSep ";"
          (lib.mapAttrsToList (role: ids: "${role}=${lib.concatStringsSep "," ids}") cfg.roles);
      } // lib.optionalAttrs (cfg.oidc != null) {
        DEVICES_OIDC_ISSUER = cfg.oidc.issuer;
        DEVICES_OIDC_CLIENT_ID = cfg.oidc.clientId;
        DEVICES_OIDC_SCOPES = lib.concatStringsSep " " cfg.oidc.scopes;
        DEVICES_OIDC_CLIENT_SECRET_FILE = "%d/oidc-secret";
      } // lib.optionalAttrs cfg.devAuth {
        DEVICES_DEV_AUTH = "1";
      };

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/devices-server ${cfg.address}:${toString cfg.port}";
        Restart = "on-failure";
        LoadCredential = lib.optional (cfg.oidc != null) "oidc-secret:${cfg.oidc.clientSecretFile}";
        DynamicUser = true;
        StateDirectory = state;
        StateDirectoryMode = "0700";

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
        RestrictAddressFamilies = [ "AF_INET" "AF_INET6" ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [ "@system-service" "~@privileged" ];
      };
    };

    networking.firewall.allowedTCPPorts = lib.mkIf cfg.openFirewall [ cfg.port ];
  };
}
