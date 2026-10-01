# The fleet on machines (docs/plan-fleet.md §3): the VM-only half of what
# harken/server/tests/fleet.rs does with processes. Three NixOS machines —
# `server` under the real `services.harken` module, `alice` and `bob` each
# running `harken-peer` against it by hostname — and the network taken away
# the way only a machine can: an interface down, the service restarted
# underneath, the whole server crashed and booted from its state directory,
# files copied into the bound media directory for the scanner.
#
#   nix build .#fleet-vm        # needs /dev/kvm to be quick; runs under TCG without
#
# A package rather than a check, because without KVM it takes the better
# part of an hour and every `nix flake check` would pay it.
#
# Each peer is driven a batch at a time: `harken-peer` run under
# `machine.succeed` with its commands in a heredoc, answering one JSON line
# each. A batch is a process that opens the replica, does what it is told,
# and quits — so every batch is also a restart from disk, which is half of
# what the fleet exists to exercise, and a batch's transcript is its
# stdout, with nothing to plumb. A long-lived peer on a FIFO would read
# worse and prove less.
#
# **Run mixed** (docs/plan-db.md D1): `old` is the previous pinned revision's
# `harken-server` package (nix/versions.nix, the last entry). A fourth
# machine, `oldserver`, runs it under the same module, with alice on that
# revision's `harken-peer` and bob on this one's; then it is upgraded in
# place — `switch-to-configuration` into a specialisation whose only change
# is the package — over the same state directory, under both, and both go
# on and converge. And bob runs the previous revision's peer against this
# revision's `server`. Peers of two revisions are compared by what they
# read, not by their state hashes, which are not comparable across them.
{ pkgs, module, harken-server, old }:
let
  # A WAV is a header and some samples; lofty reads a real one, so the
  # scanner does too. Four seconds of silence each, made at build time.
  media = pkgs.runCommand "fleet-media" { nativeBuildInputs = [ pkgs.python3 ]; } ''
    mkdir -p $out
    python3 - <<'PY'
    import struct, os
    out = os.environ["out"]
    for name in ["seeded-one", "seeded-two", "copied-one", "copied-two"]:
        rate, n = 8000, 8000 * 4
        data = b"\0\0" * n
        head = b"RIFF" + struct.pack("<I", 36 + len(data)) + b"WAVEfmt " + struct.pack("<IHHIIHH", 16, 1, 1, rate, rate * 2, 2, 16) + b"data" + struct.pack("<I", len(data))
        open(f"{out}/{name}.wav", "wb").write(head + data)
    PY
  '';

  peer = { ... }: {
    environment.systemPackages = [ harken-server ];
    systemd.tmpfiles.rules = [ "d /var/lib/peer 0755 root root -" ];
  };
in
pkgs.testers.runNixOSTest {
  name = "harken-fleet";

  nodes = {
    server = { ... }: {
      imports = [ module ];
      services.harken = {
        enable = true;
        devAuth = true;
        address = "0.0.0.0";
        openFirewall = true;
        publicUrl = "http://server:8787";
        package = harken-server;
      };
      # Two tracks there before the service first starts; two more are
      # copied in by the test, for the watch.
      systemd.tmpfiles.rules = [
        "C /srv/media/music/seeded-one.wav 0644 root root - ${media}/seeded-one.wav"
        "C /srv/media/music/seeded-two.wav 0644 root root - ${media}/seeded-two.wav"
      ];
      environment.etc."fleet-media".source = media;
    };
    alice = peer;
    bob = peer;
    # The previous revision's server, and the same machine on this one's:
    # what `switch-to-configuration` moves it to mid-test.
    oldserver = { lib, ... }: {
      imports = [ module ];
      services.harken = {
        enable = true;
        devAuth = true;
        address = "0.0.0.0";
        openFirewall = true;
        publicUrl = "http://oldserver:8787";
        package = old;
      };
      specialisation.upgraded.configuration = {
        services.harken.package = lib.mkForce harken-server;
      };
    };
  };

  testScript = { nodes, ... }: ''
    import json

    def run(machine, user, *cmds):
        """One batch: a harken-peer process, its commands, its answers."""
        lines = "\n".join(json.dumps(c) for c in cmds)
        out = machine.succeed(
            f"harken-peer --dir /var/lib/peer --server http://server:8787 --user {user} "
            f"--pump-ms 50 --backoff-ms 100,1000 <<'EOF'\n{lines}\nEOF"
        )
        answers = [json.loads(l) for l in out.splitlines() if l.strip()]
        assert len(answers) == len(cmds), f"{machine.name}: {out}"
        return answers

    def head():
        h = server.succeed("curl -sf http://127.0.0.1:8787/healthz")
        return int([l for l in h.splitlines() if l.startswith("head ")][0].split()[1])

    def caught_up(machine, user, n):
        # `settle` alone is "nothing pending, cursor still for two pumps",
        # which a peer under TCG can satisfy before its first batch lands;
        # `wait` for the head the server reports is what caught up means.
        a = run(machine, user,
                {"cmd": "wait", "cursor": n, "timeout_ms": 300000},
                {"cmd": "settle", "timeout_ms": 300000},
                {"cmd": "hash"})
        assert a[0]["ok"] and a[1]["ok"], f"{machine.name} did not catch up to {n}: {a}"
        return a[2]

    def converged(want):
        # What each peer made offline is pushed by a peer that is running:
        # one batch each to push it, then the head, then the comparison.
        for machine, user in ((alice, "alice"), (bob, "bob")):
            a = run(machine, user, {"cmd": "settle", "timeout_ms": 300000})
            assert a[0]["ok"], f"{machine.name} could not push: {a}"
        server.wait_until_succeeds(
            f"test $(curl -sf http://127.0.0.1:8787/healthz | grep '^head' | cut -d' ' -f2) -ge {want}", timeout=300)
        assert head() == want, f"head {head()}, wanted {want}"
        a, b = caught_up(alice, "alice", want), caught_up(bob, "bob", want)
        assert a["cursor"] == b["cursor"] == want, f"cursors {a['cursor']} {b['cursor']}, head {want}"
        assert a["hash"] == b["hash"], f"hashes {a['hash']} {b['hash']}"
        assert a["hash"] == a["view"] and b["hash"] == b["view"], "something is still pending"

    def mutate(machine, user, verb, **args):
        a = run(machine, user, {"cmd": "mutate", "name": verb, "args": args})
        assert a[0]["ok"], f"{machine.name} {verb}: {a}"

    start_all()
    server.wait_for_unit("harken.service")
    server.wait_for_open_port(8787)
    alice.wait_for_unit("multi-user.target")
    bob.wait_for_unit("multi-user.target")

    with subtest("both peers sign in by hostname, and the scanner's two songs arrive"):
        converged(2)

    with subtest("offline on both sides, the server restarted underneath, back"):
        alice.succeed("ip link set eth1 down")
        bob.succeed("ip link set eth1 down")
        mutate(alice, "alice", "create_playlist", name="Made with the cable out")
        mutate(bob, "bob", "create_playlist", name="Favorites")
        server.succeed("systemctl restart harken")
        server.wait_for_open_port(8787)
        alice.succeed("ip link set eth1 up")
        bob.succeed("ip link set eth1 up")
        converged(4)

    with subtest("the server crashes and boots from its state directory"):
        mutate(alice, "alice", "create_playlist", name="Before the crash")
        converged(5)
        server.crash()
        mutate(bob, "bob", "create_playlist", name="While it was down")
        server.start()
        server.wait_for_unit("harken.service")
        server.wait_for_open_port(8787)
        converged(6)

    with subtest("files copied into the media directory become songs everywhere"):
        server.succeed("cp /etc/fleet-media/copied-*.wav /srv/media/music/")
        converged(8)
        rows = run(alice, "alice", {"cmd": "query", "name": "library", "args": {"playlist_id": {"$id": "00000000-0000-0000-0000-000000000000"}}})[0]["rows"]
        files = sorted(r["file"] for r in rows)
        assert files == ["music/copied-one.wav", "music/copied-two.wav", "music/seeded-one.wav", "music/seeded-two.wav"], files

    # -- run mixed (docs/plan-db.md D1) ----------------------------------------

    OLD = "${old}/bin/harken-peer"
    NEW = "harken-peer"
    NOPL = {"$id": "00000000-0000-0000-0000-000000000000"}

    def run_at(machine, binary, d, host, user, *cmds):
        lines = "\n".join(json.dumps(c) for c in cmds)
        out = machine.succeed(
            f"{binary} --dir {d} --server http://{host}:8787 --user {user} "
            f"--pump-ms 50 --backoff-ms 100,1000 <<'EOF'\n{lines}\nEOF"
        )
        answers = [json.loads(l) for l in out.splitlines() if l.strip()]
        assert len(answers) == len(cmds), f"{machine.name}: {out}"
        return answers

    def song(title):
        i = lambda n: {"$int": str(n)}
        return {"title": title, "artist": "The Fleet", "album": "Mixed", "duration_ms": i(1000),
                "file": f"typed/{title}.wav", "track": i(0), "part": "", "catalogue": "",
                "performer": "", "bpm": i(0), "album_art": "", "artist_art": "", "disc": i(0),
                "work_title": "", "movement_no": i(0)}

    def head_of(host):
        h = host.succeed("curl -sf http://127.0.0.1:8787/healthz")
        return int([l for l in h.splitlines() if l.startswith("head ")][0].split()[1])

    def mixed(host, peers, want):
        # Each peer pushes what it has, the head is waited for, and every
        # peer reads the same library at it.
        for machine, binary, d, user in peers:
            a = run_at(machine, binary, d, host.name, user, {"cmd": "settle", "timeout_ms": 300000})
            assert a[0]["ok"], f"{machine.name} could not push: {a}"
        host.wait_until_succeeds(
            f"test $(curl -sf http://127.0.0.1:8787/healthz | grep '^head' | cut -d' ' -f2) -ge {want}", timeout=300)
        assert head_of(host) == want, f"head {head_of(host)}, wanted {want}"
        libraries = []
        for machine, binary, d, user in peers:
            a = run_at(machine, binary, d, host.name, user,
                       {"cmd": "wait", "cursor": want, "timeout_ms": 300000},
                       {"cmd": "settle", "timeout_ms": 300000},
                       {"cmd": "query", "name": "library", "args": {"playlist_id": NOPL}})
            assert a[0]["ok"] and a[1]["ok"], f"{machine.name} did not reach {want}: {a}"
            libraries.append(sorted(r["file"] for r in a[2]["rows"]))
        assert all(l == libraries[0] for l in libraries), f"libraries differ: {libraries}"
        return libraries[0]

    with subtest("mixed: the previous revision's server, both revisions' peers, then upgraded in place under them"):
        oldserver.wait_for_unit("harken.service")
        oldserver.wait_for_open_port(8787)
        assert "module " not in oldserver.succeed("curl -sf http://127.0.0.1:8787/healthz"), "the old server lists no modules"
        peers = [(alice, OLD, "/var/lib/mixed", "alice"), (bob, NEW, "/var/lib/mixed", "bob")]
        run_at(alice, OLD, "/var/lib/mixed", "oldserver", "alice", {"cmd": "mutate", "name": "add_song", "args": song("old-peer")})
        run_at(bob, NEW, "/var/lib/mixed", "oldserver", "bob", {"cmd": "mutate", "name": "add_song", "args": song("new-peer")})
        mixed(oldserver, peers, 2)
        oldserver.succeed(
            "${nodes.oldserver.system.build.toplevel}/specialisation/upgraded/bin/switch-to-configuration test")
        oldserver.wait_for_unit("harken.service")
        oldserver.wait_for_open_port(8787)
        oldserver.wait_until_succeeds("curl -sf http://127.0.0.1:8787/healthz | grep -q '^module .* current'", timeout=120)
        assert head_of(oldserver) == 2, "the upgraded server keeps the log"
        run_at(alice, OLD, "/var/lib/mixed", "oldserver", "alice", {"cmd": "mutate", "name": "add_song", "args": song("old-after")})
        run_at(bob, NEW, "/var/lib/mixed", "oldserver", "bob", {"cmd": "mutate", "name": "add_song", "args": song("new-after")})
        files = mixed(oldserver, peers, 4)
        assert files == ["typed/new-after.wav", "typed/new-peer.wav", "typed/old-after.wav", "typed/old-peer.wav"], files

    with subtest("mixed: the previous revision's peer against this revision's server"):
        n = head_of(server)
        run_at(bob, OLD, "/var/lib/old-peer", "server", "dave", {"cmd": "mutate", "name": "add_song", "args": song("from-the-old-peer")})
        files = mixed(server, [(bob, OLD, "/var/lib/old-peer", "dave"), (alice, NEW, "/var/lib/peer", "alice")], n + 1)
        assert "typed/from-the-old-peer.wav" in files, files
  '';
}
