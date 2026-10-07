# The NixOS service: `services.harken.enable = true` and there is a music
# system on a port — the sync socket, the sign-in routes, /media and the
# browser client, together, because they are one deployment and splitting
# them across two ports buys an origin to configure and nothing else.
#
# A function of the flake's packages, so the defaults name this repository's
# builds:
#
#   nixosModules.default = import ./harken/server/nix/module.nix { packages = self.packages; };
{ packages }:
{ config, lib, pkgs, ... }:
let
  cfg = config.services.harken;
  harken = packages.${pkgs.stdenv.hostPlatform.system} or { };

  # A URL nothing off this machine can fetch: this machine talking to
  # itself, or a wildcard that is not an address at all.
  selfAddressed = url:
    lib.any (needle: lib.hasInfix needle url) [
      "127.0.0.1"
      "localhost"
      "::1"
      "0.0.0.0"
    ];

  # Whether what is bound answers on this machine and nowhere else. `0.0.0.0`
  # is deliberately not in here: as a *bind* it means every interface, which
  # is the opposite of what it means in a URL.
  loopbackOnly = lib.elem cfg.address [
    "127.0.0.1"
    "localhost"
    "::1"
    "[::1]"
  ];

  # What a speaker resolves a `file` against. Total rather than guarded at
  # each use, so neither the assertion's message nor the warning's has to
  # care whether `homeAssistant` is there.
  mediaBase =
    if cfg.homeAssistant == null then cfg.publicUrl
    else if cfg.homeAssistant.mediaUrl != null then cfg.homeAssistant.mediaUrl
    else cfg.publicUrl;
in
{
  options.services.harken = {
    enable = lib.mkEnableOption "the Harken sync server and its browser client";

    port = lib.mkOption {
      type = lib.types.port;
      default = 8787;
      description = "Port for the sync socket, the sign-in routes, /media and the browser client.";
    };

    address = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = ''
        Address to bind. The default is loopback, for a reverse proxy in front
        that terminates TLS — the login sends a browser here and back, and a
        session token crosses on every connect, neither of which belongs on
        plain HTTP off the machine.

        {option}`services.harken.homeAssistant` is the one thing that pulls the
        other way: a speaker fetches its own bytes, so it needs an address of
        its own to reach — and naming one in `mediaUrl` does not bind it.
        Either widen this to `0.0.0.0`, or put something in front that
        forwards `mediaUrl` here.
      '';
    };

    publicUrl = lib.mkOption {
      type = lib.types.str;
      default = "http://${cfg.address}:${toString cfg.port}";
      defaultText = lib.literalExpression ''"http://''${address}:''${toString port}"'';
      example = "https://harken.example.com";
      description = ''
        Where a browser reaches this server: the identity provider sends
        people back to `''${publicUrl}/auth/callback`, and the browser client
        served at `/` signs in against it. Behind a reverse proxy it is the
        proxy's address, and the provider must be told the callback under it.
      '';
    };

    oidc = lib.mkOption {
      default = null;
      description = ''
        The OpenID Connect provider people sign in through. The server is the
        only OpenID Connect client — it holds the secret and hands each
        signed-in peer a session of its own — so the desktop, the browser and
        the phone need nothing but this server's address. Null runs no login
        at all, which the server refuses unless
        {option}`services.harken.devAuth` says a laptop.
      '';
      type = lib.types.nullOr (lib.types.submodule {
        options = {
          issuer = lib.mkOption {
            type = lib.types.str;
            example = "https://auth.example.com/application/o/harken/";
            description = ''
              The issuer URL, exactly as the provider states it: the server
              reads `/.well-known/openid-configuration` under it and checks
              every ID token names it.
            '';
          };
          clientId = lib.mkOption {
            type = lib.types.str;
            example = "harken";
            description = "The client id the provider knows this server as.";
          };
          clientSecretFile = lib.mkOption {
            type = lib.types.path;
            example = "/run/secrets/harken-oidc";
            description = ''
              A file holding the client secret and nothing else, handed to the
              service as a systemd credential — never the store, which is
              world-readable. It has to exist when the unit starts, and the
              failure when it does not names nothing (see
              `homeAssistant.tokenFile`); order this unit after whatever
              writes it (sops-nix, agenix).
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
        laptop does; on a machine anyone else can reach it means anyone can
        write as anyone.
      '';
    };

    redirects = lib.mkOption {
      type = lib.types.listOf lib.types.str;
      default = [ ];
      example = [ "https://harken-web.example.com/" ];
      description = ''
        Where a login may send its code back to, besides the places every
        server allows: a loopback port (the desktop client), `harken://` (the
        phone), and {option}`services.harken.publicUrl` (the browser client it
        serves). A browser client served from somewhere else goes here.
      '';
    };

    roles = lib.mkOption {
      type = lib.types.attrsOf (lib.types.listOf lib.types.str);
      default = { };
      example = { library = [ "a1b2c3-provider-sub" ]; };
      description = ''
        Who holds which role, as the account ids holding each: the provider's
        `sub`, or the name under {option}`services.harken.devAuth`. The
        server's own scanner holds `library` by construction and nobody else
        does unless named here; the library's mutations are guarded by that
        role (`is_library`, `docs/plan-guards.md` D1). Asked at every
        connection and stamped on every entry it pushes, so changing this
        and restarting grants or revokes at once.
      '';
    };

    retainDays = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 30;
      description = ''
        How long a device's place in the log holds the log for it: every entry
        above the lowest cursor of a device heard from within this many days
        is kept, so a laptop closed for a fortnight catches up by paging. A
        device away longer is sent a snapshot when it returns and rebases what
        it did offline onto it — nothing is lost either way; this only decides
        which of the two it costs.
      '';
    };

    retainEntries = lib.mkOption {
      type = lib.types.ints.unsigned;
      default = 10000;
      description = ''
        Entries of the log kept below its head whoever has been heard from:
        what bounds the server's memory and its log on disk once every device
        is caught up. The log is compacted when it holds half as much again as
        it has to keep.
      '';
    };

    web = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      example = lib.literalExpression "harken.packages.\${system}.harken-web";
      description = ''
        The browser client to serve at `/`, with the build as its validator
        (a store path's hash, since nix dates every file in one 1970). Null
        serves the socket alone, for a deployment that only wants the phone
        and the desktop.
      '';
    };

    homeAssistant = lib.mkOption {
      default = null;
      description = ''
        The house's media players, as devices in every listening session.

        A speaker is a *device*: it joins a session, becomes the output, is
        told things and reports what it is doing, exactly as a phone does.
        Which is why this is Home Assistant rather than Sonos — you get every
        `media_player` entity it knows about, for the same six service calls.
        The whole queue is pushed to the player, so its own buttons and its
        own app keep working; where it is in that queue is read back from
        what it says it is playing.
      '';
      example = lib.literalExpression ''
        {
          url = "http://homeassistant.local:8123";
          tokenFile = "/run/secrets/harken-ha";
          players = [ "media_player.kitchen" "media_player.study=Study" ];
          mediaUrl = "http://10.0.0.2:8787";
        }
      '';
      type = lib.types.nullOr (lib.types.submodule {
        options = {
          url = lib.mkOption {
            type = lib.types.str;
            example = "http://homeassistant.local:8123";
            description = "Where Home Assistant is.";
          };

          tokenFile = lib.mkOption {
            type = lib.types.path;
            example = "/run/secrets/harken-ha";
            description = ''
              A file holding a long-lived access token, handed over as a
              systemd credential for the reason the OpenID Connect secret is.

              Missing when the unit starts, it takes the whole server down
              before the binary runs, and says only

              ```
              harken.service: Failed to set up credentials: No such file or directory
              harken.service: Failed at step CREDENTIALS spawning …/harken-server
              ```

              which names neither the credential nor the path. Both this and
              `oidc.clientSecretFile` load the same way, so
              `systemctl show harken -p LoadCredential` and then `ls` on each
              source is what tells them apart. Setting this is what makes a
              speaker's token able to stop the music.
            '';
          };

          players = lib.mkOption {
            type = lib.types.listOf lib.types.str;
            example = [ "media_player.kitchen" "media_player.study=The study" ];
            description = ''
              Which entities to offer, and what to call them: `id=Name` when
              the entity id is not what you would say out loud, and the id
              alone when it is — `media_player.the_kitchen` becomes "The
              kitchen". Named rather than discovered: a picker with the
              television and the doorbell in it is a picker nobody reads.
            '';
          };

          mediaUrl = lib.mkOption {
            type = lib.types.nullOr lib.types.str;
            default = null;
            example = "http://10.0.0.2:8787";
            description = ''
              Where a *speaker* fetches bytes from, which is not necessarily
              where a phone does: the phone may be on `publicUrl` while the
              speaker only knows an address on the LAN. Falls back to
              `publicUrl`. `/media` is served with no authentication at all —
              which is what lets a speaker fetch — so this wants to be an
              address only the house can reach.
            '';
          };
        };
      });
    };

    mediaPath = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = "/srv/media";
      example = "/mnt/library";
      description = ''
        The directory the library is made of, and the one `/media/` serves.
        Created on activation if it is not there, with a `music/` inside it.

        **Music goes in `music/` under this**, not directly in it: the root is
        kind-neutral because the `file` column is. A track at
        `''${mediaPath}/music/Bach/air.flac` is served as
        `/media/music/Bach/air.flac`, and that relative path is what the log
        carries. The server walks `music/` at startup and watches the root
        after, so a file copied in appears without a rescan; a restart authors
        nothing already in the library. A file removed is not a song removed.

        The service reads it and never writes to it; it has to be readable by
        a dynamic user. `null` serves and scans no files at all.
      '';
    };

    package = lib.mkOption {
      type = lib.types.package;
      default = harken.harken-server;
      defaultText = lib.literalExpression "harken.packages.\${system}.harken-server";
      description = "The server to run.";
    };

    openFirewall = lib.mkOption {
      type = lib.types.bool;
      default = false;
      description = "Open {option}`services.harken.port` in the firewall.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.oidc != null || cfg.devAuth;
        message = ''
          services.harken: nobody could sign in. Set services.harken.oidc to an
          OpenID Connect provider, or services.harken.devAuth = true on a
          machine nobody else can reach.
        '';
      }
      {
        # Asked of what a speaker is *given*: the default publicUrl is
        # loopback, and so is an explicitly loopback mediaUrl — the one case
        # no proxy in front can rescue. A Sonos handed
        # `http://127.0.0.1:8787/media/…` fetches from itself, which presents
        # as "the speaker plays nothing".
        assertion = cfg.homeAssistant == null || !(selfAddressed mediaBase);
        message = ''
          services.harken: the house's speakers would be told to fetch from
          ${mediaBase}, which is this machine talking to itself. Set
          services.harken.homeAssistant.mediaUrl to an address a speaker can
          reach.
        '';
      }
    ];

    # The other half: naming an address is not binding one. A LAN mediaUrl
    # beside the default loopback `address` evaluates, starts cleanly, and
    # hands the house a URL nothing is listening on — the queue lands, the
    # speaker holds exactly the right URLs, and sits `paused`. A warning, not
    # an assertion, because a proxy in front of loopback is exactly how the
    # browser half is meant to be served, and a mediaUrl naming that proxy is
    # right; nix cannot know which.
    warnings =
      lib.optional (cfg.homeAssistant != null && loopbackOnly && !(selfAddressed mediaBase)) ''
        services.harken: the house's speakers are told to fetch from
        ${mediaBase}, and this server is bound to ${cfg.address} — which
        answers on this machine and nowhere else. Unless something in front
        forwards ${mediaBase} to it, a speaker gets no bytes and sits paused
        holding the right queue.

        Either set services.harken.address = "0.0.0.0" (with openFirewall, or
        a firewall rule of your own) so the LAN can reach it, or point
        mediaUrl at a proxy that can.
      '';

    # Made rather than required, so the default works on a machine where
    # nobody has put anything in it yet — and required to exist, because the
    # bind mount below fails the unit when its source is missing.
    systemd.tmpfiles.rules = lib.optionals (cfg.mediaPath != null) [
      "d ${cfg.mediaPath} 0755 root root -"
      "d ${cfg.mediaPath}/music 0755 root root -"
    ];

    systemd.services.harken = {
      description = "Harken sync server";
      wantedBy = [ "multi-user.target" ];
      after = [ "network.target" ];

      environment = {
        # The log, the kept rooms, the sessions and the scanner's replica:
        # the whole of the state, in the state directory.
        HARKEN_DATA = "%S/harken";
        HARKEN_PUBLIC_URL = cfg.publicUrl;
        HARKEN_REDIRECTS = lib.concatStringsSep "," cfg.redirects;
        HARKEN_RETAIN_DAYS = toString cfg.retainDays;
        HARKEN_RETAIN_ENTRIES = toString cfg.retainEntries;
      } // lib.optionalAttrs (cfg.roles != { }) {
        # `library=sub1,sub2;admin=sub1`, as `harken_server::roles_of` reads it.
        HARKEN_ROLES = lib.concatStringsSep ";"
          (lib.mapAttrsToList (role: ids: "${role}=${lib.concatStringsSep "," ids}") cfg.roles);
      } // lib.optionalAttrs (cfg.mediaPath != null) {
        HARKEN_MEDIA = "${cfg.mediaPath}";
      } // lib.optionalAttrs (cfg.web != null) {
        HARKEN_WEB = "${cfg.web}";
      } // lib.optionalAttrs (cfg.oidc != null) {
        HARKEN_OIDC_ISSUER = cfg.oidc.issuer;
        HARKEN_OIDC_CLIENT_ID = cfg.oidc.clientId;
        HARKEN_OIDC_SCOPES = lib.concatStringsSep " " cfg.oidc.scopes;
        # systemd copies the file into a directory only this service can read
        # and says where in `CREDENTIALS_DIRECTORY`; `%d` is that directory.
        HARKEN_OIDC_CLIENT_SECRET_FILE = "%d/oidc-secret";
      } // lib.optionalAttrs cfg.devAuth {
        HARKEN_DEV_AUTH = "1";
      } // lib.optionalAttrs (cfg.homeAssistant != null) {
        HARKEN_HA_URL = cfg.homeAssistant.url;
        HARKEN_HA_TOKEN_FILE = "%d/ha-token";
        HARKEN_HA_PLAYERS = lib.concatStringsSep "," cfg.homeAssistant.players;
      } // lib.optionalAttrs (cfg.homeAssistant != null && cfg.homeAssistant.mediaUrl != null) {
        HARKEN_HA_MEDIA = cfg.homeAssistant.mediaUrl;
      };

      serviceConfig = {
        ExecStart = "${cfg.package}/bin/harken-server ${cfg.address}:${toString cfg.port}";
        Restart = "on-failure";
        LoadCredential =
          lib.optional (cfg.oidc != null) "oidc-secret:${cfg.oidc.clientSecretFile}"
          ++ lib.optional (cfg.homeAssistant != null) "ha-token:${cfg.homeAssistant.tokenFile}";

        DynamicUser = true;
        StateDirectory = "harken";

        # The media, and nothing else of the filesystem. A bind mount rather
        # than `ReadOnlyPaths`, because `ProtectHome` masks `/home` outright
        # and a library under there would simply not be visible — a bind
        # overrides that for the one directory without opening the rest.
        BindReadOnlyPaths = lib.optional (cfg.mediaPath != null) cfg.mediaPath;

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
