# Notifications

`ark-notifications` (`modules/ark-notifications`): a device module, one
package with its crate, its Swift and its Kotlin (ui/docs/app.md, The
Device; Background And Notifications). An app opts in by listing
`ark-notifications` in its app.toml's `modules` and registering the
platform's class; its update asks through the port
(`ark_app::modules::notifications`), never this crate.

A notice now or at a time, taken back; the person asked once whether the
app may show any; the device registered for push. A second notice of one
id replaces the first. Tapped, a notice opens its route through the app's
links (`<scheme>://<path>`), as a link from anywhere else would; a button
on it is heard by the app without opening it.

## API

```rust
ark_app::modules::notifications::schedule(Notice { id, title, body, at_ms, opens, actions, channel }) // NotificationsOp, ()
ark_app::modules::notifications::cancel(id)
ark_app::modules::notifications::authorize()       // Authorize, bool: asked the first time
ark_app::modules::notifications::register_push()   // the endpoint comes back as an event
ark_app::modules::notifications::unregister_push()
trait Notifications { schedule, cancel, authorize, register_push, unregister_push, set_listener }
enum NotificationEvent { Opened { id, opens }, Action { notice, action }, Registered { endpoint }, Failed { why }, Pushed }
ark_notifications::Fake // .notices(), .allow(), .hand_out(endpoint), .hear(event)
```

What comes later is told to the process's `NotificationListener`: the
process's runtime turns an `Action` and a `Pushed` into a wake-up
(`Woken`), so the app's update hears them with no screen, and writes a
`Registered` endpoint as its session's push row.

## Push

Off by default, opt-in per app and per person: only an app whose app.toml
says `[background] push = true` has any of it in its projects, and that
app registers only once the person turns it on (`ark_app::background`'s
"Notify Me"); turning it off unregisters and takes the session's push row
away. Local notices are every listing app's, as before.

**A push is only a wake-up.** The server never puts the app's data in one:
no name, amount or word from a row. Its hub wakes the devices of the
sessions that are not connected when the app's rows change (`ark_peer`'s
push), and a woken device's one runtime dials its server, syncs as it
always does, and builds what it tells the person from the rows it then
holds, scheduling it here (`schedule`). ui/docs/app.md, Background And
Notifications, draws the whole path.

A device registers by writing its endpoint as its own session's push row
(`ark::auth::Push`, the function `session.push`) over its signed-in
connection: `register_push` hands the endpoint back as `Registered`, and
the process writes it. Signing out takes it away with the session.

- **iPhone, iPad, Mac: APNs.** `register_push` asks the system for a
  device token (`registerForRemoteNotifications`); the app delegate hands
  it on (`PhoneNotifications.registered(token:)`), with the bundle id as
  the topic and whether the build is a development one (its provisioning
  profile's `aps-environment`). Apple throttles silent pushes and may drop
  them, so a wake is visible by default, with the app's own generic words
  ("New activity in Money", the server's to say); an app with a
  Notification Service Extension shows its own words in their place.
- **Android: UnifiedPush**, the receiver protocol implemented directly
  (`ArkPush.kt`), not the connector library: the library is not in the
  offline Maven graph the builds replay, and the protocol is a handful of
  broadcasts. The person's distributor carries the wake — ntfy, a
  self-hosted one, or Google's FCM through a distributor app; the F-Droid
  builds link nothing of Google's. It is sent `Urgency: high`, shows
  nothing, and is sealed to the device's Web Push keys (RFC 8291:
  `web_push_keys`, kept in the module's private preferences), so only this
  app's server can wake it. With several distributors installed, the
  person's default one (`unifiedpush://link`) is used, else the first.
- **The desktop and the browser**: none yet; every operation answers
  `Unavailable`.

### The Notification Service Extension (iPhone)

Generated only when app.toml says `[background] service-extension = true`.
It is a separate process by Apple's design: the system starts it, alone,
for each alert push marked `mutable-content`, gives it about 30 seconds
and about 24 MB, and shows whatever it hands back. It cannot reach the
app's process, its runtime or its replica. What it does
(`swift-extension/`) is swap the wake's generic words for the device's
own: the last notice the app's runtime worked out from its replica, which
`PhoneNotifications` leaves in their App Group (`group.<bundle id>`,
`ark.notice`). It does not sync over ark itself: that would mean linking
the app's library and a replica into a 24 MB process that may not hold
the person's keys; until then its words are as fresh as the app's last
sync.

## Per Platform

| | Local notices | Push | Permission |
|---|---|---|---|
| iPhone, iPad, Mac | `UNUserNotificationCenter`: a time trigger for later, a category per set of buttons, a thread per channel | APNs | asked at run time (`requestAuthorization`) |
| Android | `NotificationManager`, a channel per kind, an `AlarmManager` alarm for later | UnifiedPush | `POST_NOTIFICATIONS`, asked at run time on Android 13 and later (`NotificationsPermission`, put once at the app's root) |
| Desktop, browser | Unavailable | Unavailable | — |

## Permissions

- iPhone, Mac: none in Info.plist; the app asks at run time
  (`authorize`) before its first notice. Push: the `aps-environment`
  entitlement and the `remote-notification` background mode, written only
  for an app whose app.toml says `[background] push = true`.
- Android: `POST_NOTIFICATIONS` (Android 13 and later, asked at run time).
  Nothing runs at boot: a notice for later is set again after a restart
  only in an app that declares `[background]` (ark-background's boot
  receiver, with `RECEIVE_BOOT_COMPLETED`).
  Push adds the UnifiedPush receiver (`ArkPushReceiver`, exported: a
  distributor is another app; it takes only what names this install's
  token), only for an app that declares it.
