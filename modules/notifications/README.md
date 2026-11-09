# Notifications

`ark-notifications` (`modules/notifications`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `notifications` in its `modules.toml` and depending on `ark-notifications`.

A local notification at a time, or one taken back. A second notice of
one id replaces the first; tapped, it opens its route.

## API

```rust
ark_notifications::schedule(Notice { id, title, body, at_ms, opens }) // -> ModuleCall, answered Done
ark_notifications::cancel(id)
trait Notifications { fn schedule(&self, notice: Notice); fn cancel(&self, id: String); }
ark_notifications::Fake // .notices()
```

## Per Platform

No native implementation yet: `UNUserNotificationCenter` (iPhone, Mac),
`NotificationManager` with an `AlarmManager` for a later time (Android).
The desktop and the browser answer `Unavailable`.

## Permissions

- iPhone, Mac: none in Info.plist; the app asks at run time
  (`requestAuthorization`) before the first notice.
- Android: `android.permission.POST_NOTIFICATIONS` (Android 13 and later).
