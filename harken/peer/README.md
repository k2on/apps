# harken-peer

harken's peer with no screen, and what it hosts. Built as the binary
`harken-peer` beside the server (`harken/server/src/bin/harken-peer.rs`
calls `harken_peer::headless::main`), so the fleet that drives it and the
release that ships it are the server's package's.

- **`headless.rs`** — `harken-peer --dir DIR (--server URL | --alone)`: an
  `ark_peer::Peer` and its link, minus the window, driven by JSON lines on
  stdin (`harken/README.md` § harken-peer, and the fleet).
- **`house.rs`** — `harken-peer --house --server URL`: the house. One
  thread; the house's own connection to the server's socket (a volatile
  machine that says no device and authors nothing, fed every session) is
  read for where to stand, and each speaker standing in an account's
  session is a volatile machine on a socket of its own, signed in through
  the server's `/house/sign-in` with the key in `HARKEN_HOUSE_KEY`'s file.
  A session is acted on once every speaker in it has heard what the
  house's connection has and has nothing unanswered.
- **`house.rs`** — Home Assistant's media players as devices of a
  server's listening sessions. A pure `Speakers`, handed the session and what the house said:
  who holds a speaker, what a change of the session becomes (a hand-off of
  the whole queue from where it was, a pause, a play, a seek), what the
  speaker did that the session does not know said as its own transition
  (`started`, `advance`, `jump`, `stalled`, `play`, `seek`,
  `report_position`) — where a playing speaker is being its
  `media_position` counted on from `media_position_updated_at`, which is
  when the house last refreshed it, read against the house's own clock as
  its answers' `Date`s narrow it down (`when::Skew`); an undated one is no
  position at all, and one dated before the bridge last told the speaker
  something is not believed for `STALE` polls; a speaker more than
  `SYNC_MS` (600 ms, every output's bound) from where the session counts it
  is said where it is (`report_position`, the clock re-anchored and no
  player told anything), more than `DRIFT_MS` its own `seek`, and a
  session that moved by more than the bound is told to it; a stall is two
  silent polls in a row (`STALL`), at a point the house dated since it last
  played; which track it is on read from the URL it reports
  compared as an address, never as a string (`address`: decoded, entities
  and all, the scheme and host in one case, a default port dropped), or
  from its `queue_position` where the URL names its queue; a hand-off from
  half way through a track that the speaker starts from the beginning (the
  seek after the enqueue dropped before it loaded) is sought again once it
  has the track, `SEEKS` times at most, and `started` is said only once it
  is there and the house has dated where — at that measured point, counted
  on to when it is said (`as_of`), on the track handed or any after it
  (`advance` or `jump` first), never at the point handed; `WAIT` polls of
  fetching before it stops waiting — `SETTLE` polls
  of grace after a hand-off, `LAPSE` polls before letting a playing speaker
  go (`release`, said as it), `QUIET` polls after telling it anything, and
  the second taker releasing the first. Its volume and its group are
  reconciled the same way (`tune`): what the house says it supports is said
  once as the speaker's `equip`; a volume or a group that moved in the
  session is told to the house (`volume_set`, `volume_mute`, `join`,
  `unjoin`), and one that moved in the house while the session stood still
  is said as the speaker (`report_volume`, `report_group`), outside `QUIET`;
  a member is never handed a queue. Tracks put into the session's queue
  after the one playing or at its end reach the speaker's own queue in
  place (`play_media` with `enqueue: next`, last first, and `enqueue:
  add`), so nothing stops; a track taken out or moved is a fresh hand-off
  from where the track playing is. And `ha`, the thread: each speaker a
  device — a socket of its own to the server — in each session a client
  is in;
  where no client is, the speakers making the sound — the output and
  whoever follows it — go on standing, reconciled and reporting, while it
  sounds and for `IDLE` (ten minutes) after it stops, and then go, pausing
  what they held, the session kept paused as any is; a speaker that is not
  making the sound never keeps a session's presence open — the
  service calls, polling every speaker standing anywhere (its volume and
  group; what it plays only where it is held), the integration it groups
  in read once through the template API, and every hand-off, release,
  pause, join and unjoin said out loud. Everything it tells the house goes
  before the next poll, so a reading is always of what was told; a handed
  queue's first track and its seek go at once and the rest `ha::PUSH` at a
  time between polls; and with `HARKEN_BRIDGE_TRACE=1` each poll of each
  speaker is a line (below).

## Running

```
harken-peer --house --server http://127.0.0.1:8787
```

| variable | |
|---|---|
| `HARKEN_HA_URL`, `HARKEN_HA_TOKEN_FILE`, `HARKEN_HA_PLAYERS` | all three; players are `media_player.x[=Name]`, comma-separated |
| `HARKEN_HA_MEDIA` | where a *speaker* fetches from (default: `HARKEN_PUBLIC_URL`) |
| `HARKEN_HOUSE_KEY` | the file the server keeps the house's key in (the server's `HARKEN_HOUSE_KEY`) |
| `HARKEN_BRIDGE_TRACE=1` | a line per poll per speaker: what the house said of it and what the bridge did (below) |

The names are the ones the server read before the house left it; what is
new is `HARKEN_HOUSE_KEY`, on both. It keeps nothing on disk: its
speakers' machines are volatile, and their logins are the server's to
hand again.

### When a speaker is not where the bars say

The bridge is tested against a stand-in Home Assistant, and a real house
with a real Sonos has differed from it each time something went wrong. The
trace is what shows the difference: one line per poll of each speaker
standing, saying exactly what the house said and what the bridge made of
it. Turn it on with `services.harken.homeAssistant.trace = true;` (which
sets `HARKEN_BRIDGE_TRACE=1`) and rebuild, or set that variable by hand
for a house run any other way. Then:

1. Play something on a browser or a phone, half a minute into a track.
2. Note the time, and pick the speaker.
3. Wait half a minute with the speaker playing; if the bars are behind or
   jumped back, say by how much and when.
4. Send back the journal from a few seconds before the time noted to the
   end — `journalctl -u harken-house --since "HH:MM:SS" | grep 'house:'` — and
   what you saw and when.

A line reads:

```
harken-peer: house: trace: media_player.kitchen: state "playing", media_content_id "http://10.0.0.2:8787/media/music/a.mp3", media_position 41.0, media_position_updated_at "2026-10-18T21:07:23.112000+00:00", Date Sun, 18 Oct 2026 21:07:24 GMT; speaker 41934 ms; session 41890 ms; track 0 of 12 (handed 0); started; quiet 0, waited 3, sought 1, silent 0; nothing done
```

— the house's answer as it gave it, then where the bridge counts the
speaker (`(dated before the last command)` when the house has not yet
dated anything since it was told something; `undated` when it gave no
position), where it has the session (`standing` while the hand-off is in
flight, at the point handed), where in the queue, and what it did:
`started`, `report_position`, `seek`, `stalled` with the point each was
said at, `media_seek` and the rest told to the house. Turn it off again
afterwards: it is a line a second for every speaker standing.
