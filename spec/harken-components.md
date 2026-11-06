# harken's own components

What harken draws that is harken's, so stays in the app — the same
component, the same inputs, in every shell (Android `harken/android`,
iPhone `harken/ios`, desktop `harken/iced`), and what a web client builds.
Everything generic they are made of is ArkUI's (`docs/arkui-parts.md`): a
page's header, a banner, the catalogue's banner, headings, entries and "See
All", chips, the scrubber and the volume.

| Component | Inputs | Made of | Android | iPhone | Desktop |
|---|---|---|---|---|---|
| TrackRow | track, under, playing, onPlay, onMore, leading | MoreButton | `TrackRow` | `TrackRow` | the table's row |
| TrackNumber | number, playing | — | `TrackNumber` | `TrackNumber` | the table's first column |
| EpisodeRow | episode, progress, people | ProgressBar-like progress | in `Pages.kt` | `EpisodeRow` | `view.rs` episode block (`progress`) |
| PlayHeader | cover, kind, title, by?, facts, canPlay, onPlay, onShuffle | PageHeader | `PlayHeader` | `PlayHeader` | `view_cover` |
| RecordHeader (CoverHeader) | cover, over, title, by, small, canPlay, onPlay, onShuffle | — | `CoverHeader` | `RecordHeader` | `view_cover` |
| PersonBanner | person's wide picture, kind, name | HeroBanner | `HeroBanner` with the derived square | `PersonBanner` | `hero::banner` |
| Featured | label, cover, title, facts, onPlay, onOpen | PlayPill | `Featured` | `Featured` | `view_featured` |
| WorkHeading | composer, work, length, first | — | `WorkHeading` | `WorkHeading` | `work_heading` |
| RecordFoot | year, tracks, label, licences | — | `Foot` | `RecordFoot` | `view_foot` |
| Credit | person's name, portrait, role, onOpen? | Cover | `Credit` | people on a recording | the recording's people |
| PlayPill / PlayShuffle | onPlay, onShuffle?, canPlay | — | `PlayPill` | `PlayShuffle` | the header's buttons |
| Offline | host | Banner | `Offline` | `Offline` | — |
| MiniPlayer | what plays, position, controls | — | `MiniFace` | `MiniPlayer` | the bar (`view_bar`) |
| FullPlayer | what plays, position, controls | Scrubber, VolumeSlider | `FullFace` | `FullPlayer` | — |
| PlayerScrubber | position, duration, chapters | Scrubber | `Scrubber` (PlayerScreens) | `PlayerScrubber` | `parts::scrubber` in the bar |
| Transport | playing, shuffle, repeat, jogs | — | in `FullFace` | `Transport` | the bar's transport |
| QueueRow / QueueSheet | queue, now, upNext | Reorder | `QueueSheet` | `QueueSheet`, `QueueRow` | the queue page |
| TranscriptSheet | transcript lines, position | — | in `PlayerScreens` | `TranscriptSheet` | transcript page |
| DevicePicker | devices, levels, groups | VolumeSlider | `DevicePicker` (`Sheets.kt`) | `DevicesSheet`, `DeviceLine` | the picker (`view_devices`) |
| NewPlaylist | name, problem | NameField | `NewPlaylist` | `NewPlaylist` | the name box |
| FollowSheet | kind, query, found shows | — | `FollowSheet` | `FollowSheet` | — |
