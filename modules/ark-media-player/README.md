# Media Player

`ark-media-player` (`modules/ark-media-player`): a device module, one package with its crate,
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

The port is the runtime's (`ark_module::media_player`), re-exported here:

```rust
// The calls, each a MediaOp whose output is ():
load(url, at_ms)  play()  pause()  seek(at_ms)  set_rate(rate)  set_volume(0.0..=1.0)  stop()
set_now_playing(Option<NowPlaying>)   // title, artist, album, artwork URL, duration, position, rate
set_controls(Controls)                // has_next, has_previous, skip_forward_s, skip_back_s, can_seek, can_rate
set_children(parent, Vec<BrowseItem>) // the browse tree, answering MediaEvent::Browse

trait MediaPlayer  { fn load(..); fn play(&self); …; fn set_listener(&self, Option<Arc<dyn MediaListener>>); }
trait MediaListener { fn heard(&self, event: MediaEvent); }

enum MediaEvent {
    Loaded { duration_ms }, Position { at_ms }, Playing, Waiting, Paused, Ended,
    Remote { command: RemoteCommand }, Failed { why }, Interrupted, Resumed,
    Route { change: Route },             // Unplugged (it paused itself), Plugged, Other
    Browse { parent }, PlayItem { id },  // a car or a watch
}
enum RemoteCommand { Play, Pause, Toggle, Stop, Next, Previous, SkipForward { seconds },
                     SkipBack { seconds }, SeekTo { at_ms }, Rate { rate } }
struct BrowseItem { id, title, subtitle, artwork, playable, browsable }   // ROOT is ""

ark_media_player::Fake       // .asked(), .take(), .now_playing(), .controls(), .browse(parent), .hear(event)
ark_media_player::phone      // (ffi) the foreign MediaPlayer and MediaListener, port(..)
ark_media_player::Browser    // (web, wasm32)
ark_media_player::Desktop    // (desktop, Linux)
```

**The player never acts on a control by itself.** The lock screen's
pause, a headset's button, a car's skip are each `Remote { command }`, and
the app decides what it means. What the player does by itself is what the
system does to it, each said: an interruption (`Interrupted`, and
`Resumed` when the system lets it go on), the headphones out
(`Route { Unplugged }`, paused), the end (`Ended`).

## Per Platform

- **Browser** (feature `web`, wasm32): `Browser`, an `<audio>` element and
  the Media Session API (`navigator.mediaSession`: metadata, action
  handlers, `setPositionState`), reached by name so a browser without it
  plays all the same.
- **Linux desktop** (feature `desktop`): `Desktop`, symphonia's decoders on
  the sound card through rodio (cpal, ALSA), a stream read by range
  requests, and MPRIS (`org.mpris.MediaPlayer2.<name>.instance<pid>`) on the
  session bus for the media keys, the desktop's widgets and `playerctl`.
  No sound card is `Failed`, said; no session bus is no MPRIS.
- **Tests**: `Fake`.

## Permissions

Declared in `module.toml`:

- iPhone: `UIBackgroundModes = [audio]`, to keep playing locked.
- Android: `FOREGROUND_SERVICE`, `FOREGROUND_SERVICE_MEDIA_PLAYBACK`.
