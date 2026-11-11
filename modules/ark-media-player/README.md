# Media Player

`ark-media-player` (`modules/ark-media-player`): a device module, one package with its crate,
its Swift and its Kotlin (ui/docs/app.md, The Device). An app opts in by
listing `ark-media-player` in its app.toml's `modules` and asking through
the port (`ark_module::media_player`).

One sound at a time: loaded from a URL at a position, played and paused,
sought, at a rate and a volume; what the system shows of it (the lock
screen, the notification, Control Center, a headset, the desktop's
widgets, a car's and a watch's now-playing) and which of its controls it
offers; and a browse tree a car or a watch reads. Nothing in it is any
app's: harken's player (harken-player's `MediaSound`, a queue and the
listening session's reconciler) is one app over it.

## API

The port is the runtime's (`ark_module::media_player`), re-exported here:

```rust
// The calls, each a MediaOp whose output is ():
load(url, at_ms)  play()  pause()  seek(at_ms)  set_rate(rate)  set_volume(0.0..=1.0)  stop()
set_now_playing(Option<NowPlaying>)   // title, artist, album, artwork URL, placeholder seed, duration, position, rate
set_controls(MediaControls)                // has_next, has_previous, skip_forward_s, skip_back_s, can_seek, can_rate, can_add
set_children(parent, Vec<BrowseItem>) // the browse tree, answering MediaEvent::Browse

trait MediaPlayer  { fn load(..); fn play(&self); …; fn set_listener(&self, Option<Arc<dyn MediaListener>>); }
trait MediaListener { fn heard(&self, event: MediaEvent); }

enum MediaEvent {
    Loaded { duration_ms }, Position { at_ms }, Playing, Waiting, Paused, Ended,
    Remote { command: RemoteCommand }, Failed { why }, Interrupted, Resumed,
    Route { change: RouteChange },             // Unplugged (it paused itself), Plugged, Other
    Browse { parent }, PlayItem { id }, Add { ids, next },  // a car or a watch
}
enum RemoteCommand { Play, Pause, Toggle, Stop, Next, Previous, SkipForward { seconds },
                     SkipBack { seconds }, SeekTo { at_ms }, Rate { rate } }
struct BrowseItem { id, title, subtitle, artwork, playable, browsable }   // ROOT is ""

ark_media_player::Fake       // .asked(), .take(), .now_playing(), .controls(), .browse(parent), .hear(event)
ark_media_player::phone      // (ffi) the foreign NativeMediaPlayer and MediaListener, port(..)
ark_media_player::Browser    // (web, wasm32)
ark_media_player::Desktop    // (desktop, Linux)
```

**The player never acts on a control by itself.** The lock screen's
pause, a headset's button, a car's skip are each `Remote { command }`, and
the app decides what it means. What the player does by itself is what the
system does to it, each said: an interruption (`Interrupted`, and
`Resumed` when the system lets it go on), the headphones out
(`RouteChange { Unplugged }`, paused), the end (`Ended`).

## Per Platform

- **iPhone, iPad, Mac** (swift/PhoneMediaPlayer.swift, through `phone`):
  `AVPlayer`, one item at a time; on the iPhone and iPad the audio session
  in the `.playback` category with the `.longFormAudio` policy, its
  interruptions (`Interrupted`; `Resumed` when the system says to go on,
  and it goes on) and route changes (the headphones out pause it:
  `Route { Unplugged }`); `MPNowPlayingInfoCenter` (title, artist, album,
  artwork fetched from its URL — or, with none or none to be had, ArkUI's
  derived square of the `placeholder` seed in the palette the app gives
  `PhoneMediaPlayer(squares:)` — duration, elapsed time, rate; the player's
  own position while it has a sound, the app's otherwise) and
  `MPRemoteCommandCenter` (play, pause, toggle, stop, next, previous, skip
  forward and back by the controls' seconds, the scrubber, the rate), each
  command a `Remote`. **The lock screen, Control Center, the Dynamic
  Island, AirPods' and a headset's buttons, CarPlay's now-playing screen and
  a paired Apple Watch's Now Playing then work with no more code**: they
  all read the now-playing record and send the remote commands.
- **Android** (kotlin/: `PhoneMediaPlayer`, `ArkMediaService`, through
  `phone`): Media3's ExoPlayer with audio focus handled (a transient loss is
  `Interrupted` then `Resumed`, a lasting one `Interrupted`) and becoming
  noisy (`Route { Unplugged }`), a network wake lock, inside `ArkMediaService`,
  a `MediaLibraryService` Media3 puts in the foreground with its media
  notification while the sound plays. The media session shows `ShownPlayer`:
  the ExoPlayer's sound with the app's words and artwork (with no artwork,
  ark-ui's derived square of the `placeholder` seed, in `ArkMedia.squares`,
  as the picture's bytes), or — nothing
  loaded — the app's `NowPlaying` alone (a remote for a sound made
  elsewhere), the buttons the controls offer; every command a `Remote`, a
  car's or a watch's choice a `PlayItem`, and — while the controls say
  `can_add` — what one adds to the queue an `Add` of its items' ids (before
  the one item shown is `next`, past it the end). The service is a media library:
  the tree is asked for a parent at a time (`Browse`, answered by
  `set_children`; an app that does not answer in five seconds is an empty
  list). It is open: an app with more to say to the media session (harken's
  car search and its remote volume) subclasses it and declares its own.
- **Browser** (feature `web`, wasm32): `Browser`, an `<audio>` element and
  the Media Session API (`navigator.mediaSession`: metadata, action
  handlers, `setPositionState`, `playbackState`), reached by name so a
  browser without it plays all the same. No tree, and no placeholder: the
  Media Session takes a picture only by its address.
- **Linux desktop** (feature `desktop`): `Desktop`, symphonia's decoders on
  the sound card through rodio (cpal, ALSA), a stream read by range
  requests, and MPRIS (`org.mpris.MediaPlayer2.<name>.instance<pid>`) on the
  session bus for the media keys, the desktop's widgets and `playerctl`.
  No sound card is `Failed`, said; no session bus is no MPRIS. No tree, and
  no placeholder: MPRIS's `mpris:artUrl` is an address.
- **Tests**: `Fake` keeps what it was asked (`asked`, `take`), what it shows
  (`now_playing`, `controls`) and its tree (`browse`, which asks as the
  system would), and tells its listener what a test says (`hear`).

## CarPlay, Android Auto and the Watch

- **Now playing** needs nothing more: CarPlay's now-playing screen and the
  Watch's Now Playing read `MPNowPlayingInfoCenter` and send
  `MPRemoteCommandCenter`'s commands; Android Auto's player and Wear OS's
  media controls are the media session's.
- **Android Auto browsing** is `ArkMediaService`'s library and the browse
  tree; an app also needs `res/xml/automotive_app_desc.xml`
  (`<automotiveApp><uses name="media"/></automotiveApp>`) and the
  `com.google.android.gms.car.application` meta-data pointing at it, which
  module.toml cannot carry yet (it has no resources), and artwork a car can
  load (a `content://` URI the app grants: a car will not fetch `http`).
  Search and voice ("play X on …") are `onSearch` and a search query in
  `onSetMediaItems`, which the port does not have yet.
- **CarPlay browsing** needs Apple's `com.apple.developer.carplay-audio`
  entitlement, a `CPTemplateApplicationScene` and a `CPListTemplate` tree
  over the same browse items: not built. The tree `set_children` gives is
  kept on the iPhone for it.
- **The Apple Watch's own app** (browsing and playing on the watch) is a
  watchOS target of its own: not built.

## Permissions

Declared in `module.toml`:

- iPhone, iPad: `UIBackgroundModes = [audio]`, to keep playing locked and
  in the background.
- Android: `FOREGROUND_SERVICE` and `FOREGROUND_SERVICE_MEDIA_PLAYBACK` for
  the service in the foreground while it plays, `WAKE_LOCK` for the network
  wake lock; the service itself (`dev.arkui.modules.ArkMediaService`,
  exported, `foregroundServiceType="mediaPlayback"`, the three library and
  session actions) and Media3's two libraries. The media notification asks
  for `POST_NOTIFICATIONS` on Android 13 and later, which is the app's to
  ask; without it the sound still plays.
- Linux: none; the session bus is the user's.
