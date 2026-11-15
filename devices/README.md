# devices

A household's phones held to the policy their own server declares. devices is
an Ark app (ui/docs/app.md) made the phone's **device owner** (Android's device
policy controller, through modules/ark-dpc): it keeps the phone's VPN always on
and locked down, sets restrictions (no changing the VPN, no factory reset),
keeps apps installed and running, and decides permissions — whatever the
policy assigned to the phone's person says, and nothing it does not say.

- `core/` — the domain: policies (the server's alone), who each applies to,
  and what each phone last reported (its own session's alone).
- `app/` — the status screen, which does the work: woken by the OS with no
  screen (every fifteen minutes, by a push when the policies change, at boot,
  and when the app is made the device owner) or from its Apply Now, it syncs,
  reads the phone, makes each change the plan says (the VPN first), reads the
  phone again and reports. The shell around it is the app library's.
- `server/` — ark-peer's hub over the domain, signed in to through the
  household's OpenID provider, holding the policies its NixOS module declares
  (`server/nix/module.nix`: `services.devices.policies`, `.assignments`), and
  waking phones by UnifiedPush when they change. `devices-enroll` is beside it.

## Enrolling a Phone

On a phone reset with no account added (GrapheneOS has no QR enrollment
yet), with USB debugging on and the laptop on the phone's USB cable:

```bash
devices-enroll https://devices.example.com --apk devices.apk --user ann
```

It installs devices, makes it the device owner (`dpm set-device-owner
us.koon.devices/dev.arkui.modules.ArkDpcAdmin`), signs ann in **in the
laptop's browser**, makes a one-time code for her session and opens its join
link on the phone (`devices://join?server=…&code=…`). The phone asks to join,
signs in as her, and its first sync holds it to her policy. Then turn USB
debugging off; a policy with `no_debugging_features` keeps it off.

Install Tailscale (and sign it in) and a UnifiedPush distributor (ntfy) before
the policy locks the VPN down, and name them in the policy's
`uninstallBlocked` and `userControlDisabled`.

## The Key

`debug.keystore` is committed and public, as money's is: fine for building and
trying devices. A phone made a device owner trusts whoever holds the key its
app was installed with — every update must be signed by it — so a phone that
is really managed runs a build signed with a key of the household's own, kept
as a secret.

## What Has Not Been Seen

No APK has been built here (the Android build is past this machine's disk),
and no phone has been made devices' device owner. The domain, the app's work
over the modules' fakes, the server against a real hub, the module evaluated
in a NixOS system, and the generated Android project are what is verified.
