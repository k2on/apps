"""money's page as a real client, end to end: money-server on the demo's
seeded household with dev auth, the SPA it serves in a headless Chromium
(Playwright), and another device beside it (`money-peer`, an ark-peer
over the socket as the phones and the desktop are).

  python3 sync.py --server BIN --peer BIN --seed BIN --web DIR [--out DIR]

0. A code that is no login's is said on the sign-in page, which offers
   Sign In rather than going round by itself.
1. Sign In goes to the server's sign-in, is signed in by name (dev auth's
   form), and comes back to the seeded household. Its session is the
   server's HttpOnly cookie: no script reads it, and nothing the page
   keeps in localStorage holds the token.
2. A transaction the page adds through New Transaction reaches the other
   device; one the other device adds appears on the page, live.
3. The server stops; the page adds one while it is down (kept, pending);
   the server starts again on the same data, the page dials it again and
   the other device sees what was added offline.
4. A reload is still signed in (no sign-in page) and holds all three.

Screenshots of each step go to --out.
"""

import argparse
import os
import signal
import socket
import subprocess
import sys
import tempfile
import time
import urllib.request

from playwright.sync_api import sync_playwright, expect


def free_port():
    s = socket.socket()
    s.bind(("127.0.0.1", 0))
    port = s.getsockname()[1]
    s.close()
    return port


class Server:
    """money-server with dev auth on `data`, serving `web` at `port`."""

    def __init__(self, binary, data, web, port):
        self.binary, self.data, self.web, self.port = binary, data, web, port
        self.base = f"http://127.0.0.1:{port}"
        self.proc = None

    def start(self):
        env = dict(os.environ, MONEY_DEV_AUTH="1", MONEY_DATA=self.data, MONEY_WEB=self.web, MONEY_PUBLIC_URL=self.base)
        env.pop("ARK_DEV_USER", None)
        self.proc = subprocess.Popen([self.binary, f"127.0.0.1:{self.port}"], env=env)
        for _ in range(200):
            try:
                if urllib.request.urlopen(f"{self.base}/healthz", timeout=1).status == 200:
                    return
            except OSError:
                time.sleep(0.05)
        raise SystemExit("money-server did not come up")

    def stop(self):
        self.proc.send_signal(signal.SIGTERM)
        self.proc.wait(timeout=30)


def say(what):
    print(f"sync.py: {what}", flush=True)


def main():
    ap = argparse.ArgumentParser()
    for a in ["--server", "--peer", "--seed", "--web"]:
        ap.add_argument(a, required=True)
    ap.add_argument("--out", default=None)
    ap.add_argument("--chromium", default=None, help="a Chromium to launch, else Playwright's own")
    args = ap.parse_args()
    out = args.out or tempfile.mkdtemp(prefix="money-sync-shots-")
    os.makedirs(out, exist_ok=True)
    data = os.path.join(tempfile.mkdtemp(prefix="money-sync-"), "data")
    subprocess.run([args.seed, data], check=True)
    server = Server(args.server, data, args.web, free_port())
    server.start()
    try:
        run(args, server, out)
    finally:
        if server.proc.poll() is None:
            server.stop()


def run(args, server, out):

    def peer(*words, ok=True):
        r = subprocess.run([args.peer, server.base, "pat", *words], timeout=120)
        if ok and r.returncode != 0:
            raise SystemExit(f"money-peer {' '.join(words)} failed")
        return r.returncode

    failures = []
    with sync_playwright() as pw:
        launch = {"executable_path": args.chromium} if args.chromium else {}
        browser = pw.chromium.launch(**launch)
        ctx = browser.new_context(viewport={"width": 1440, "height": 900}, reduced_motion="reduce")
        page = ctx.new_page()
        page.on("pageerror", lambda e: failures.append(f"page error: {e}"))
        page.set_default_timeout(30_000)

        def shot(name):
            page.screenshot(path=os.path.join(out, f"{name}.png"))

        # 0. A code that is no login's: said on the sign-in page, which
        # offers to sign in again rather than going round by itself.
        page.goto(server.base + "/?code=no-such-code")
        expect(page.get_by_text("Signing in didn't finish", exact=False)).to_be_visible()
        assert "code=" not in page.url, page.url
        shot("0-sign-in-page")
        say("a code that is no login's is said, and Sign In offered")

        # 1. Signed in, then the household.
        page.get_by_role("button", name="Sign In").click()
        page.wait_for_selector("input[name=user]")
        say(f"sent to sign in: {page.url}")
        page.fill("input[name=user]", "sam")
        page.click("button")
        expect(page.get_by_text("Ready to Assign")).to_be_visible()
        expect(page.get_by_text("Dining Out").first).to_be_visible()
        assert "code=" not in page.url, page.url
        shot("1-signed-in")
        say("signed in as sam, the seeded household shown")
        cookies = [c for c in ctx.cookies() if c["name"] == "ark-session"]
        assert len(cookies) == 1 and cookies[0]["httpOnly"] and cookies[0]["sameSite"] == "Strict", cookies
        token = cookies[0]["value"]
        assert "ark-session" not in page.evaluate("document.cookie"), "a script reads the session"
        kept = page.evaluate("Object.keys(localStorage).map(k => localStorage.getItem(k)).join('\\n')")
        assert token not in kept and token.split(".")[-1] not in kept, "the token is in localStorage"
        say("the session is an HttpOnly, SameSite=Strict cookie; localStorage holds no token")

        def add(payee, amount):
            page.get_by_text("Transactions", exact=True).first.click()
            page.get_by_role("button", name="New Transaction").first.click()
            page.get_by_placeholder("Payee", exact=True).fill(payee)
            page.get_by_placeholder("Amount", exact=True).fill(amount)
            page.get_by_text("Add Transaction", exact=True).click()
            expect(page.get_by_text(payee, exact=True).first).to_be_visible()

        # 2. The page's change reaches the other device; the other's the page.
        add("Web Bakery", "12.50")
        shot("2-added-on-the-page")
        peer("wait", "Web Bakery", "30")
        say("the other device sees what the page added")
        peer("add", "Peer Grocer", "7.25")
        expect(page.get_by_text("Peer Grocer", exact=True).first).to_be_visible()
        shot("3-the-other-device-s-change")
        say("the page sees what the other device added, live")

        # 3. Offline: authored while the server is down, sent once it is back.
        server.stop()
        page.wait_for_timeout(500)
        add("Offline Cafe", "3.10")
        shot("4-added-offline")
        server.start()
        peer("wait", "Offline Cafe", "60")
        say("what the page added offline reached the other device once the server was back")

        # 4. A reload is still signed in, and holds all three.
        page.reload()
        page.get_by_text("Transactions", exact=True).first.click()
        for payee in ["Web Bakery", "Peer Grocer", "Offline Cafe"]:
            expect(page.get_by_text(payee, exact=True).first).to_be_visible()
        assert page.locator("input[name=user]").count() == 0
        shot("5-reloaded")
        say("a reload is still signed in, from the cookie")
        browser.close()
    if failures:
        raise SystemExit("\n".join(failures))
    say(f"passed; screenshots in {out}")


if __name__ == "__main__":
    sys.exit(main())
