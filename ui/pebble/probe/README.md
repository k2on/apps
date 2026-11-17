# Pebble probe

A watchapp that measures what a native app gets on a Pebble Time 2 (step 2
of `docs/north-star.md`): Rust linked through the SDK, persistent storage,
AppMessage, the phone script's network, and the device. It shows the results
on five pages (Up and Down page, Select runs the AppMessage test again) and
logs every one, so `pebble logs` has them too.

- `src/c/probe.c`: the watchapp.
- `rust/`: a `no_std` static library for `thumbv7m-none-eabi`, its global
  allocator the app heap. The SDK compiles apps for `-mcpu=cortex-m3` with
  the soft-float ABI, so `thumbv7em-none-eabihf` objects do not link with it.
- `src/pkjs/index.js`: the phone script. It echoes AppMessages, tries an XHR
  GET and a WebSocket held open for 30 s, and sends the results back.

## Build

With `uv`, Rust and `rustup target add thumbv7m-none-eabi`:

    uv tool install pebble-tool --python 3.13
    pebble sdk install latest        # 4.44 when this was written
    pebble build                     # cargo builds rust/ first

The app is `build/probe.pbw`.

## Install

On a computer, in the emulator (`emery` is the Time 2):

    pebble install --emulator emery --logs

On the watch, through the Pebble phone app's Developer Connection: sign in
to the Pebble account in the app (Settings, General), connect the watch, and
on the Devices tab open the watch's menu (the three dots) and turn on Dev
Connection. Then, on the computer:

    pebble login                          # the same account
    pebble install --cloudpebble --logs

For the LAN instead (no account; the same Wi-Fi), turn on Settings, Phone,
Connectivity, Use LAN developer connection, toggle Dev Connection off and
on, and use the IPv4 address shown under it:

    pebble install --phone 192.168.1.42 --logs

On Android over USB, with the LAN connection on:
`adb forward tcp:9000 tcp:9000`, then `pebble install --phone 127.0.0.1`.

Storage fills to about 1 MB and deletes it all again, which takes minutes on
the watch. Leave the app open until page 3 has its numbers.

## A native companion

`package.json` names harken's and money's Android apps (`us.koon.harken`,
`us.koon.money`) as PebbleKit 2 companions, `required: false`. A companion
that implements the probe's protocol gets the same round-trip test as the
script, shown as `native` on page 4:

- on `onAppOpened`, send `{CMD: 1, SRC: 2}` (HELLO);
- on `{CMD: 2, TARGET: 2, SEQ, ECHO_LEN, DATA}` (ECHO), send back
  `{CMD: 2, SRC: 2, SEQ, DATA: ECHO_LEN bytes}`.

The keys are `appKeys` in the built `appinfo.json` (CMD 10000, SRC 10001,
SEQ 10002, TARGET 10003, ECHO_LEN 10004, DATA 10005). Integers are
four-byte.
