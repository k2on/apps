# `checks.oidc-kanidm`: signing in to money and harken through Kanidm, on one
# NixOS machine — Kanidm from nixpkgs, provisioned through its module
# (`services.kanidm.provision`) with the two apps as confidential clients,
# each scope-mapped to its own group, and a person in both; money-server and
# harken-server through this repository's modules (`services.money.oidc`,
# `services.harken.oidc`). nix/oidc-kanidm.py is the browser: one cookie jar
# that signs in to money (`/auth/login` → Kanidm's form → `/auth/callback`
# → `/auth/exchange`), then to harken on the provider's session alone, with
# no password asked a second time — and checks that each server's token is
# its own.
#
# Every password, secret and key here is a test's, made for this machine
# and nothing else (as nixpkgs' own Kanidm tests do): none is a deployment's.
{ pkgs, moneyModule, harkenModule }:
let
  # A CA, and one certificate under it for Kanidm's name, which the machine
  # trusts (`security.pki`) — what a household's own CA is to its servers.
  certs = pkgs.runCommand "oidc-kanidm-certs" { nativeBuildInputs = [ pkgs.openssl ]; } ''
    mkdir $out && cd $out
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes -days 3650 \
      -keyout ca.key -out ca.crt -subj "/CN=oidc-kanidm test CA" \
      -addext "basicConstraints=critical,CA:TRUE" -addext "keyUsage=critical,keyCertSign,cRLSign" \
      -addext "subjectKeyIdentifier=hash"
    openssl req -newkey ec -pkeyopt ec_paramgen_curve:prime256v1 -nodes \
      -keyout idm.key -out idm.csr -subj "/CN=idm.test"
    printf '%s\n' "subjectAltName=DNS:idm.test" "basicConstraints=critical,CA:FALSE" \
      "keyUsage=critical,digitalSignature" "extendedKeyUsage=serverAuth" \
      "authorityKeyIdentifier=keyid" "subjectKeyIdentifier=hash" > ext
    openssl x509 -req -in idm.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 3650 \
      -extfile ext -out idm.crt
    cat idm.crt ca.crt > chain.crt
    chmod 644 *
  '';
  secret = name: value: pkgs.writeText "oidc-kanidm-${name}" value;
  idmAdminPassword = "test-idm-admin-6c1f0e2b9d7a4c58";
  personPassword = "test-person-a91d3e77f04c4b2f8e10";
  moneySecret = secret "money-secret" "test-money-client-secret-3f8a91c2";
  harkenSecret = secret "harken-secret" "test-harken-client-secret-7b2e04d9";
  issuer = client: "https://idm.test/oauth2/openid/${client}";
  money = "http://127.0.0.1:8788";
  harken = "http://127.0.0.1:8787";
in
pkgs.testers.runNixOSTest {
  name = "oidc-kanidm";

  nodes.machine = { pkgs, ... }: {
    imports = [ moneyModule harkenModule ];
    virtualisation.memorySize = 2048;
    networking.hosts."127.0.0.1" = [ "idm.test" ];
    security.pki.certificateFiles = [ "${certs}/ca.crt" ];
    environment.systemPackages = [ pkgs.python3 ];

    services.kanidm = {
      # The provision's client secrets are set from files only with the
      # secret-provisioning patches.
      package = pkgs.kanidmWithSecretProvisioning_1_5;
      enableServer = true;
      serverSettings = {
        origin = "https://idm.test";
        domain = "idm.test";
        bindaddress = "127.0.0.1:443";
        ldapbindaddress = "127.0.0.1:636";
        tls_chain = "${certs}/chain.crt";
        tls_key = "${certs}/idm.key";
      };
      provision = {
        enable = true;
        idmAdminPasswordFile = secret "idm-admin" idmAdminPassword;
        groups.money_users = { };
        groups.harken_users = { };
        persons.ann = {
          displayName = "Ann Example";
          mailAddresses = [ "ann@example.invalid" ];
          groups = [ "money_users" "harken_users" ];
        };
        systems.oauth2.money = {
          displayName = "Money";
          originUrl = "${money}/auth/callback";
          originLanding = "${money}/";
          basicSecretFile = moneySecret;
          preferShortUsername = true;
          scopeMaps.money_users = [ "openid" "profile" "email" ];
        };
        systems.oauth2.harken = {
          displayName = "Harken";
          originUrl = "${harken}/auth/callback";
          originLanding = "${harken}/";
          basicSecretFile = harkenSecret;
          preferShortUsername = true;
          scopeMaps.harken_users = [ "openid" "profile" "email" ];
        };
      };
    };

    services.money = {
      enable = true;
      port = 8788;
      publicUrl = money;
      oidc = {
        issuer = issuer "money";
        clientId = "money";
        clientSecretFile = moneySecret;
      };
    };
    services.harken = {
      enable = true;
      port = 8787;
      publicUrl = harken;
      # Nothing to hear, and whisper's model is half a gigabyte.
      transcripts.enable = false;
      oidc = {
        issuer = issuer "harken";
        clientId = "harken";
        clientSecretFile = harkenSecret;
      };
    };
    systemd.tmpfiles.rules = [ "d /srv/media 0755 root root -" ];
    # Each server discovers its provider as it starts: after Kanidm, whose
    # unit is up once its provisioning has run.
    systemd.services.money = { after = [ "kanidm.service" ]; requires = [ "kanidm.service" ]; };
    systemd.services.harken = { after = [ "kanidm.service" ]; requires = [ "kanidm.service" ]; };
  };

  testScript = ''
    machine.wait_for_unit("kanidm.service")
    machine.wait_for_unit("money.service")
    machine.wait_for_unit("harken.service")
    machine.wait_for_open_port(8788)
    machine.wait_for_open_port(8787)
    # money says which provider it signs in through, as it starts.
    machine.succeed("journalctl -u money.service | grep -q 'signing in through ${issuer "money"}'")
    print(machine.succeed(
        "IDM_ADMIN_PASSWORD=${idmAdminPassword} PERSON_PASSWORD=${personPassword} "
        "python3 -I ${./oidc-kanidm.py} --idm https://idm.test --person ann "
        "--money ${money} --harken ${harken} --ca /etc/ssl/certs/ca-certificates.crt"
    ))
  '';
}
