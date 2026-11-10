# Screens

Listen opens any screen, player state or piece of transient UI from launch
arguments (`-key value`, read from UserDefaults' argument domain). With
`-screen` set, the playhead does not move, so every capture is still.
`ci/shots.sh` takes all of the shots below, light and dark, in
`.github/workflows/reference-listen-shots.yaml`.

## Launch arguments

| Argument | Values | Effect |
|---|---|---|
| `-screen` | `home`, `library`, `songs`, `albums`, `artists`, `composers`, `playlists`, `album:<id>`, `artist:<id>`, `composer:<name>`, `playlist:<id>`, `podcasts` / `shows`, `show:<id>`, `episode:<id>`, `downloads`, `household`, `search:<query>`, `settings` | Selects the tab (sidebar item) and pushes the route. On compact width, library and podcast routes go through the Library and Podcasts tabs; on iPad and the Mac they select the sidebar item. `settings` opens the Settings sheet (iOS) or window (Mac). |
| `-settingsTab` | `server`, `household`, `playback`, `downloads`, `appearance` | The Settings pane shown. |
| `-playing` | `track:<id>` (default `track:harbor-lights-2`), `episode:<id>` | What is loaded, with a queue and a playhead on a lyric/transcript line. Episodes play at 1.25×. |
| `-player expanded` | | Opens the full player (full-screen cover on iOS, over the window content on the Mac). |
| `-panel` | `lyrics`, `upnext` | The full player's panel (lyrics, or transcript for spoken items). |
| `-inspector` | `lyrics`, `upnext` | Opens the inspector (iPad, Mac; a sheet on iPhone) on that pane. |
| `-popover` | `output`, `upnext` | Opens the output picker (toolbar on iPad/Mac; the full player's when `-player expanded`) or the Up Next popover. |
| `-dialog` | `handoff:<device-id>`, `remove-download:<episode-id>` | Shows the hand-off or remove-download confirmation. |
| `-sheet` | `new-playlist` | The New Playlist sheet (prefilled). |
| `-searchActive YES` | | Activates the search field: scopes and suggestions show. |
| `-appearance` | `light`, `dark`, `system` | The app's colour scheme (Settings ▸ Appearance). |
| `-window` | `WxH`, e.g. `1440x900` | The Mac window's default size. |
| `-frozen YES` | | Stops the playhead without `-screen`. |

Menus cannot be opened from launch arguments; the drive UI test
(`ci/drive/Shots/Shots.swift`) opens them by accessibility identifier:
`menu.sort` (Songs, Albums), `menu.speed` (full player, spoken items),
`menu.more-current`, `menu.album-more`, `more.<episode-id>`, `button.output`,
`button.upnext`, `panel.lyrics` / `panel.upnext`, and rows
`song.<track-id>`, `album.<album-id>`, `episode.<episode-id>`,
`show.<show-id>`, `device.<device-id>`. Steps: `tap:<q>`, `context:<q>`
(long-press / right-click), `hover:<q>` (open a submenu), `wait:<s>`.

Useful ids: albums `harbor-lights`, `paper-satellites`, `clair-de-lune`,
`goldberg`, `four-seasons`, `night-bus`, `glasshouse`; tracks
`<album>-<n>`; shows `workbench`, `slow-science`, `grace-hill`, `vespers`;
episodes `<show>-<n>` (1 is the newest); playlists `favorites`,
`recently-added`, `most-played`, `sunday-morning`, `focus`, `road-trip`;
devices `this`, `living-room`, `kitchen`, `studio`, `irises-ipad`, `den-tv`.

## What to capture

### iPhone 17 Pro (portrait)
| Shot | Arguments | Shows |
|---|---|---|
| 01-home | `-screen home` | Tab bar, mini player accessory, shelves |
| 02-library | `-screen library` | Library categories, Recently Added grid |
| 03-songs | `-screen songs` | The Table collapsed to its title column |
| 04-album-harbor-lights | `-screen album:harbor-lights` | Stacked album header, track list, playing row |
| 05-podcasts | `-screen podcasts` | Show grid with unplayed badges, latest episodes |
| 06-show-grace-hill | `-screen show:grace-hill` | Sermon series, filter, episode rows |
| 07-episode-transcript | `-screen episode:grace-hill-1 -playing episode:grace-hill-1` | Episode detail and tappable transcript |
| 08-player | `-screen home -player expanded` | Full player |
| 09-player-lyrics | `… -panel lyrics` | Lyrics panel |
| 10-player-transcript | `… -panel lyrics -playing episode:grace-hill-1` | Transcript, speed pill |
| 11-player-upnext | `… -panel upnext` | Up Next with shuffle/repeat |
| 12-household | `-screen household` | Who's listening where; hand-off tiles |
| 13-search-bach | `-screen search:bach` | Results |
| 14-settings-playback | `-screen settings -settingsTab playback` | Settings sheet, pane pushed |
| 20-popover-output | `-screen home -player expanded -popover output` | Output picker (sheet on iPhone) |
| 21-dialog-handoff | `-screen household -dialog handoff:kitchen` | Hand-off confirmation |
| 22-sheet-new-playlist | `-screen playlists -sheet new-playlist` | New Playlist sheet |
| 23-dialog-remove-download | `-screen downloads -dialog remove-download:grace-hill-1` | Destructive confirmation |
| 24-search-suggestions | `-screen search:sl -searchActive YES` | Active search: scopes, suggestions |
| 30-menu-sort | `-screen songs` + `tap:menu.sort` | Sort pop-up menu |
| 31-context-song | `-screen album:harbor-lights` + `context:song.harbor-lights-3` | Song context menu with preview |
| 32-context-song-submenu | … + `hover:Add_to_Playlist` | Add to Playlist ▸ submenu |
| 33-context-album | `-screen albums` + `context:album.paper-satellites` | Album context menu |
| 34-context-episode | `-screen show:workbench` + `context:episode.workbench-2` | Episode context menu |
| 35-menu-speed | `-screen home -player expanded -playing episode:grace-hill-1` + `tap:menu.speed` | Playback speed menu |

### iPad Pro 13-inch, portrait
01-home, 02-songs, 03-album-clair-de-lune, 04-show-grace-hill, 05-player,
06-household, 07-search-bach, 08-settings (downloads), 20-popover-output
(toolbar), 21-popover-upnext, 22-dialog-handoff, 23-sheet-new-playlist,
30-menu-sort, 32-context-song-submenu, 33-context-album, 35-menu-speed.

### iPad Pro 13-inch, landscape
01-songs-inspector-lyrics, 02-album-inspector-upnext
(`-playing track:clair-de-lune-4`), 03-show-transcript-inspector,
04-player, 05-player-transcript, 06-household, 07-home,
08-settings-household, 20-popover-output, 30-menu-sort,
32-context-song-submenu, 34-context-episode.

### Mac, 1440×900 window
01-songs, 02-album-harbor-lights, 03-songs-inspector-lyrics,
04-episode-transcript-inspector, 05-home, 06-player, 07-household,
08-search-bach, 09-settings-playback (Settings window: `-screen.png`
holds the whole screen), 20-popover-output, 21-popover-upnext,
22-dialog-handoff, 23-sheet-new-playlist, 24-search-suggestions; driven,
whole screen: 30-menu-sort, 31-context-song-submenu, 33-context-album,
34-context-episode, 35-menu-speed, 36-menu-controls (the Controls menu).
