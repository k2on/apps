# Device Policy

`ark-dpc` (`modules/ark-dpc`): a device module, one package with its crate
and its Kotlin (ui/docs/app.md, The Device). An app opts in by listing
`ark-dpc` in its app.toml; its update asks through the runtime's port
(`ark_app::modules::dpc`), and its bridge depends on `ark-dpc` with the
`ffi` feature and registers what the phone gives
(`ark_dpc::phone::port(PhoneDpc(context))`).

A DPC is Android's *device policy controller*: the one app a phone's owner
makes its device owner, and the only one `DevicePolicyManager` lets hold
the phone to a policy. With this module an app is that app: it keeps the
phone's VPN always on and locked down (nothing leaves but through it),
sets user restrictions (`no_config_vpn`, `no_factory_reset`), keeps apps
from being uninstalled, decides apps' permissions, and turns the cameras
and screen capture off. What the policy is, where it comes from and when it
is applied are the app's (In the Background, below).

## API

```rust
// The port: ark_module::dpc (ark_app::modules::dpc), re-exported here.
dpc::apply(Change::AlwaysOnVpn { app: Some("com.tailscale.ipn".into()), lockdown: true }) // -> Apply, output ()
dpc::read(asked)                       // -> Read, output State: owner, the VPN, restrictions, … sdk, security_patch
dpc::plan(&policy, &state) -> Vec<Change>
dpc::asked(&policy) -> Asked           // what a read must look at for the plan
trait Dpc { fn apply(&self, change: Change) -> Answer<()>; fn read(&self, asked: Asked) -> Answer<State>; }
// The module's own:
ark_dpc::Fake                          // a phone's state, .state(), .changes(); Fake::not_owner()
ark_dpc::phone::{Dpc, DpcError, port}  // (ffi) the foreign trait Kotlin implements, and it as the port
```

A `Policy` says only what it names. Its maps are `true` (on, blocked,
disabled) or `false`, and what a map leaves out is left as the phone has
it; the VPN is `Vpn::Unmanaged` unless it says `AlwaysOn` or `Off`. So
undoing a line is saying `false`, never deleting it, and **a policy that
forgets the VPN never takes it away**: with lockdown on, a phone whose VPN
was cleared by mistake would be cut off from the server that could put it
back. `plan` puts the VPN first, so nothing after it is fetched around it,
and plans nothing when the app is not the device owner.

An app reconciles: `read(asked(&policy))`, `plan`, `apply` each change,
and reports what it read. Read again, the plan is empty (the tests hold
it).

## Per Platform

| Platform | |
|---|---|
| Android | `PhoneDpc(context)` (kotlin/), over `DevicePolicyManager` as the admin `ArkDpcAdmin`. Not the owner, a read says `owner = false` and a change fails, said. Lockdown is read from Android 10. |
| iPhone, iPad, Mac | Unavailable: no app can manage an Apple device. An MDM server does, over Apple's push service (NanoHUB). |
| Desktop, browser | Unavailable |

## Becoming the Device Owner

Once, on a phone with no account added since its last reset, with USB
debugging on (GrapheneOS's setup has no QR enrollment yet:
GrapheneOS/platform_packages_apps_SetupWizard2#35):

    adb install <app>.apk
    adb shell dpm set-device-owner <applicationId>/dev.arkui.modules.ArkDpcAdmin

then USB debugging off, which `no_debugging_features` keeps off. Its
`applicationId` is the flake's `bundlePrefix` and app.toml's `id`
(`us.koon.<id>`).

**The APK's key.** Every update of a device owner must be signed by the key
it was installed with, and whoever holds that key can replace it. An app
that lists ark-dpc is never signed with the committed debug key (app.toml's
`keystore`): its key is CI's secret, as the iPhone's team is.

## No Icon

An app that is only the phone's owner says `launcher = false` in app.toml's
`[android]` (ui/prebuild.py), and has no icon: since Android 10 an app with
no launcher activity is given one that opens its settings page, except on a
fully managed phone (one with a device owner), where Android draws none for
any app (`LauncherApps.getActivityList`). It is reached instead by:

- a notification it posts, while it has something to say (a policy it
  could not apply);
- a Quick Settings tile (`TileService`), for its status at a glance;
- a dialer code (`*#*#<code>#*#*`, `android.telephony.action.SECRET_CODE`);
- `adb shell am start` on its activity, for whoever set it up.

## In the Background

The module makes a change when asked; when to ask is the app's. A device
owner can do more of it in the background than any other app:

1. **WorkManager's periodic work** (every 15 minutes at the shortest, with
   a network): read, plan, apply, report. It survives a reboot and an
   update; it does not survive a force stop, which is why the app's own
   package is the first it keeps the person from stopping (6). Work is a
   Maven dependency (`androidx.work:work-runtime-ktx`), which the app's
   `[android] dependencies` lists.
2. **UnifiedPush** to wake it at once: the server POSTs to the phone's
   endpoint after a deploy, ntfy's app on the phone hands the message to
   the app's `PushService` (a `[[android.services]]` entry), and that
   enqueues one expedited run of the same work. The message says nothing:
   the app always fetches the policy itself, so a stranger with the
   endpoint can only make it look sooner. Periodic work stays as the
   fallback.
3. **What the system tells a device owner anyway**: its admin's
   `onEnabled` and `onProfileProvisioningComplete` (the first run, right
   after `dpm set-device-owner`), `ACTION_MY_PACKAGE_REPLACED` (after its
   own update), `BOOT_COMPLETED`, and `onSystemUpdatePending`. Each is one
   more reason to run the work now.
4. **Exemptions it has**: a device owner may start a foreground service
   from the background (Android 12's restriction does not apply to it),
   and an active admin is out of App Standby's buckets, so its work is not
   deferred for days the way an unused app's is. It still sleeps through
   Doze with the rest; periodic work runs in Doze's windows.
5. **The kernel without a screen**: the work loads the app's library and
   runs its Rust without an activity, through the bridge, as a test does.
   Either a peer opened for one sync (ArkDB: the policy is the server's,
   the phone's state a record of its own, the last good policy kept on the
   phone) or a fetch of a signed file (the phone checks the server's
   signature with a key it was given at enrollment). Either way it applies
   the last good policy while offline.
6. **What it keeps running**: `setUserControlDisabledPackages` (Android 11)
   keeps the person from force-stopping or clearing the data of the app,
   the VPN and the push distributor; `setUninstallBlocked` keeps them
   installed. The next change this module would add is that one.
7. **A foreground service holding the sync socket**, only if seconds matter
   more than the battery; UnifiedPush gives the same with one shared
   connection, and is the default.

## Permissions

None: a device owner's powers are its admin's. Its manifest takes the
receiver module.toml declares (`ArkDpcAdmin`, guarded by
`BIND_DEVICE_ADMIN`) and its policies, `res/xml/ark_dpc_admin.xml`.
