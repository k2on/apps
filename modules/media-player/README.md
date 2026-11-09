# Media Player

`ark-media-player` (`modules/media-player`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `media-player` in its `modules.toml` and depending on `ark-media-player`.

One sound at a time: loaded from a URL at a position, played and paused,
sought, at a rate and a volume, with the lock screen's now-playing
record. harken's `PhoneSound` and `harken_player::Sound`, generalised.
What the player did (`Loaded`, `Position`, `Playing`, `Paused`, `Ended`,
`Remote` from the lock screen or a headset, `Failed`) arrives through the
`MediaListener` the shell registers, which turns each into the app's
message.

## API

```rust
ark_media_player::{load, play, pause, seek, set_rate, set_volume, set_now_playing} // -> ModuleCall
trait MediaPlayer { fn load(..); fn play(&self); …; fn set_listener(&self, Option<Arc<dyn MediaListener>>); }
trait MediaListener { fn heard(&self, event: MediaEvent); }
ark_media_player::Fake // .asked(), .hear(event)
```

## Per Platform

No native implementation yet. Moving harken onto it: implement the trait
over AVPlayer and `MPNowPlayingInfoCenter` (iPhone, Mac), Media3 with a
`MediaSessionService` (Android) and `harken_player` (desktop), register it,
and move the player's state machine from the shells into harken's update.

## Permissions

Declared in `module.toml`:

- iPhone: `UIBackgroundModes = [audio]`, to keep playing locked.
- Android: `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK`.
