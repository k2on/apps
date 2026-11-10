# `checks.money-web-sync`: money's page as a real client, end to end, in the
# sandbox (money/web/tests/sync.py) — money-server on the demo's seeded
# household with dev auth, the SPA it serves in Playwright's headless
# Chromium signing in by name, and `money-peer` (another device, an
# ark-peer over the socket) seeing what the page adds and adding what the
# page then shows, live; a change made while the server is down sent once
# it is back; a reload still signed in from the session cookie. The output
# is the run's screenshots.
{ runCommand, python3, playwright-driver, makeFontsConf, dejavu_fonts, money-web, money-server, money-seed }:
runCommand "money-web-sync" {
  nativeBuildInputs = [ (python3.withPackages (ps: [ ps.playwright ])) ];
  # nixpkgs' Chromium for its Playwright, and fonts: DejaVu alone, as the
  # screenshots draw with (.github/screens/app-web.sh).
  PLAYWRIGHT_BROWSERS_PATH = playwright-driver.browsers;
  PLAYWRIGHT_SKIP_VALIDATE_HOST_REQUIREMENTS = "true";
  FONTCONFIG_FILE = makeFontsConf { fontDirectories = [ dejavu_fonts ]; };
} ''
  export HOME=$TMPDIR
  python3 -I ${../tests/sync.py} \
    --server ${money-server}/bin/money-server --peer ${money-server}/bin/money-peer \
    --seed ${money-seed}/bin/money-seed --web ${money-web} --out $out
''
