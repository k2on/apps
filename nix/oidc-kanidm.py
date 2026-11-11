"""checks.oidc-kanidm's browser (nix/oidc-kanidm.nix): one cookie jar, as a
person's browser keeps one, signing in to money and then to harken through
the same Kanidm.

First the person is given a password and a TOTP — Kanidm asks persons for
a second factor — through its credential-update API as idm_admin (the
provision makes people, not credentials). Then each
app's own sign-in, as its clients start it: `/auth/login` with a loopback
redirect, the provider's pages as they come (the name, the password, the
consent), the server's `/auth/callback`, and the code at `/auth/exchange`.
money asks for the password; harken must not — the provider's session in
the jar is the sign-in. Each login is the same person (the provider's
`sub`), and each server's token opens that server and not the other.

No token, secret or password is printed. The standard library only.
"""

import argparse
import hashlib
import hmac
import html.parser
import http.cookiejar
import json
import os
import ssl
import struct
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

# Where each app's login sends its code: a loopback port nothing listens on;
# the code is read off the redirect, as a desktop client's listener would.
BACK = "http://127.0.0.1:9/"


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, *args, **kwargs):
        return None


class Browser:
    def __init__(self, ca):
        self.jar = http.cookiejar.CookieJar()
        ctx = ssl.create_default_context(cafile=ca)
        self.open = urllib.request.build_opener(
            urllib.request.HTTPSHandler(context=ctx),
            urllib.request.HTTPCookieProcessor(self.jar),
            NoRedirect,
        ).open

    def send(self, url, data=None, headers=None, method=None):
        """(status, headers, body) — a redirect answered, not followed."""
        req = urllib.request.Request(url, data=data, headers=headers or {}, method=method)
        try:
            with self.open(req, timeout=30) as r:
                return r.status, r.headers, r.read().decode()
        except urllib.error.HTTPError as e:
            return e.code, e.headers, e.read().decode(errors="replace")

    def json(self, url, body, headers=None, method="POST"):
        h = {"Content-Type": "application/json", **(headers or {})}
        status, hdrs, text = self.send(url, json.dumps(body).encode(), h, method)
        if status != 200:
            sys.exit(f"{method} {url}: {status} {text[:300]}")
        return hdrs, json.loads(text) if text else None


class Forms(html.parser.HTMLParser):
    """Each form's action and the values its inputs send."""

    def __init__(self):
        super().__init__()
        self.forms = []

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if tag == "form":
            self.forms.append((a.get("action", ""), {}))
        elif tag == "input" and self.forms and a.get("name"):
            self.forms[-1][1][a["name"]] = a.get("value") or ""


def forms(body):
    p = Forms()
    p.feed(body)
    return dict(p.forms)


def admin_token(b, idm, password):
    """A privileged session of idm_admin's, through the auth API."""
    hdrs, _ = b.json(f"{idm}/v1/auth", {"step": {"init2": {"username": "idm_admin", "issue": "token", "privileged": True}}})
    session = {"X-KANIDM-AUTH-SESSION-ID": hdrs["X-KANIDM-AUTH-SESSION-ID"]}
    b.json(f"{idm}/v1/auth", {"step": {"begin": "password"}}, session)
    _, said = b.json(f"{idm}/v1/auth", {"step": {"cred": {"password": password}}}, session)
    token = said.get("state", {}).get("success")
    if not token:
        sys.exit(f"idm_admin did not sign in: {list(said.get('state', {}))}")
    return token


class Totp:
    """The person's authenticator: the secret Kanidm enrolled, and the last
    step a code was given for (a code is not given twice)."""

    def __init__(self, secret):
        self.key = bytes(secret["secret"])
        self.algo = {"sha1": hashlib.sha1, "sha256": hashlib.sha256, "sha512": hashlib.sha512}[secret["algo"].lower()]
        self.step = int(secret["step"])
        self.digits = int(secret["digits"])
        self.used = None

    def code(self):
        while (now := int(time.time()) // self.step) == self.used:
            time.sleep(1)
        self.used = now
        mac = hmac.new(self.key, struct.pack(">Q", now), self.algo).digest()
        at = mac[-1] & 0x0F
        n = struct.unpack(">I", mac[at : at + 4])[0] & 0x7FFFFFFF
        return str(n % 10**self.digits).zfill(self.digits)


def give_credentials(idm, ca, person, admin_password, password):
    b = Browser(ca)
    bearer = {"Authorization": f"Bearer {admin_token(b, idm, admin_password)}"}
    status, _, text = b.send(f"{idm}/v1/person/{person}/_credential/_update_intent", headers=bearer)
    if status != 200:
        sys.exit(f"the credential update intent: {status} {text[:300]}")
    _, (session, _) = b.json(f"{idm}/v1/credential/_exchange_intent", json.loads(text)["token"])
    _, cu = b.json(f"{idm}/v1/credential/_update", [{"password": password}, session])
    _, cu = b.json(f"{idm}/v1/credential/_update", ["totpgenerate", session])
    totp = Totp(cu["mfaregstate"]["TotpCheck"])
    _, cu = b.json(f"{idm}/v1/credential/_update", [{"totpverify": [int(totp.code()), "test"]}, session])
    if not cu.get("can_commit"):
        sys.exit(f"the credentials were not taken: {cu.get('warnings')} {cu.get('mfaregstate')}")
    b.json(f"{idm}/v1/credential/_commit", session)
    print(f"{person} has a password and a TOTP")
    return totp


def sign_in(b, app, idm, person, password, totp):
    """App `app`'s login through the provider, in `b`; the login, and
    whether the provider asked for the password."""
    url = f"{app}/auth/login?redirect={urllib.parse.quote(BACK, safe='')}"
    status, hdrs, body = b.send(url)
    asked = False
    for _ in range(20):
        if status in (301, 302, 303, 307, 308):
            url = urllib.parse.urljoin(url, hdrs["Location"])
            if url.startswith(BACK):
                break
            print(f"  -> {urllib.parse.urlsplit(url).path}")
            status, hdrs, body = b.send(url)
            continue
        if status != 200:
            sys.exit(f"{url}: {status} {body[:400]}")
        page = forms(body)
        if "/ui/login/begin" in page:
            fields = {"username": person}
            step = "/ui/login/begin"
        elif "/ui/login/mech_choose" in page:
            fields = {"mech": "password"}
            step = "/ui/login/mech_choose"
        elif "/ui/login/totp" in page:
            fields = {"totp": totp.code()}
            if "password" in page["/ui/login/totp"]:
                fields["password"] = password
            step = "/ui/login/totp"
            asked = True
        elif "/ui/login/pw" in page:
            fields = {"password": password}
            step = "/ui/login/pw"
            asked = True
        elif "/ui/oauth2/consent" in page:
            fields = page["/ui/oauth2/consent"]
            step = "/ui/oauth2/consent"
        else:
            sys.exit(f"{url}: a page this browser does not know: {list(page)} {body[:400]}")
        print(f"  {step}")
        url = urllib.parse.urljoin(idm, step)
        data = urllib.parse.urlencode(fields).encode()
        status, hdrs, body = b.send(url, data, {"Content-Type": "application/x-www-form-urlencoded"})
    else:
        sys.exit(f"{app}: the sign-in went round too many times")
    code = urllib.parse.parse_qs(urllib.parse.urlsplit(url).query).get("code", [None])[0]
    if not code:
        sys.exit(f"{app}: came back with no code")
    _, login = b.json(f"{app}/auth/exchange", {"code": code})
    return login, asked


def opens(b, app, token):
    """Whether `app` takes `token` as a signed-in device's: it makes a
    device code only for one."""
    status, _, _ = b.send(f"{app}/auth/device-code", b"", {"Authorization": f"Bearer {token}"}, "POST")
    return status == 200


def main():
    p = argparse.ArgumentParser()
    for a in ("--idm", "--person", "--money", "--harken", "--ca"):
        p.add_argument(a, required=True)
    args = p.parse_args()
    password = os.environ["PERSON_PASSWORD"]
    totp = give_credentials(args.idm, args.ca, args.person, os.environ["IDM_ADMIN_PASSWORD"], password)

    b = Browser(args.ca)
    print("money:")
    money, asked = sign_in(b, args.money, args.idm, args.person, password, totp)
    if not asked:
        sys.exit("money: signed in without the password, on an empty jar")
    print("harken:")
    harken, asked = sign_in(b, args.harken, args.idm, args.person, password, totp)
    if asked:
        sys.exit("harken: the provider asked for the password or a code again")

    for name, login in (("money", money), ("harken", harken)):
        user = login["user"]
        print(f"{name}: signed in as {user['name']!r} <{user['email']}>")
        if not user["id"] or not login["token"]:
            sys.exit(f"{name}: a login with no one in it")
    if money["user"]["id"] != harken["user"]["id"]:
        sys.exit("money and harken signed in different subjects")
    if money["user"]["email"] != "ann@example.invalid":
        sys.exit(f"money's login has the wrong email: {money['user']['email']!r}")

    checks = [
        ("money", args.money, money["token"], True),
        ("harken", args.harken, harken["token"], True),
        ("money's token at harken", args.harken, money["token"], False),
        ("harken's token at money", args.money, harken["token"], False),
    ]
    for name, app, token, want in checks:
        got = opens(b, app, token)
        print(f"{name}: {'accepted' if got else 'refused'}")
        if got != want:
            sys.exit(f"{name}: expected {'accepted' if want else 'refused'}")
    print("one sign-in, two apps")


if __name__ == "__main__":
    main()
