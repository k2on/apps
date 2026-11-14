# Background

`ark-background` (`modules/ark-background`): the periodic background sync
(ui/docs/app.md, Background And Notifications). Its update asks through
the port (`ark_app::modules::background`), never this crate.

**Off by default, opt-in per app and per person.** An app whose app.toml
says no `[background] sync` has nothing of it in its projects: no
background mode, no task identifier, no job service, no boot receiver. One
that declares it still schedules nothing until the person turns it on in
the app (`ark_app::background`'s setting), and stops when they turn it
off.

## API

```rust
ark_app::modules::background::set(Some(every_s)) // SetSync, (): woken about that often
ark_app::modules::background::set(None)          // never again
ark_app::modules::background::wanted()           // Wanted, Option<u32>: what was last set
ark_background::Fake                             // .every()
```

Each run wakes the process's one runtime with `Woken::Sync`
(`ArkProcess::wake`): the app's update syncs (`Command::Sync`) and tells
the person what came from its own rows.

## Per Platform

| | How | What app.toml's `[background] sync` writes |
|---|---|---|
| iPhone, iPad | `BGAppRefreshTask` `<bundle id>.sync`, registered at launch (`PhoneBackground.shared.register(process:)`) | `UIBackgroundModes: fetch`, `BGTaskSchedulerPermittedIdentifiers` |
| Mac | `NSBackgroundActivityScheduler` | nothing |
| Android | a persisted periodic `JobScheduler` job (`ArkSyncJob`); WorkManager is not in the offline Maven graph, and runs on JobScheduler itself | the job service (`BIND_JOB_SERVICE`), `RECEIVE_BOOT_COMPLETED` |
| Desktop, browser | none (`Unavailable`) | — |

The OS decides when: at least fifteen minutes apart, later on a phone in
low power or rarely used. `[background] boot = true` (Android) also wakes
the runtime when the device starts (`ArkBootReceiver`, `Woken::Boot`).
