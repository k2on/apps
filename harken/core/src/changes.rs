//! Every durable change harken's screens send, as one type: what a client
//! sends rather than a function's name and its input's JSON (money's
//! `MoneyChange`'s twin). A [`HarkenChange`] names its function
//! ([`HarkenChange::name`]) and carries its input, which crosses to the
//! peer as its bytes ([`HarkenChange::input`]); its refinements are asked
//! of it before it is sent ([`HarkenChange::problem`]), in the words a form
//! says under its field. The listening session's transitions are here too:
//! a peer sends a function of the live space's as a call on it, never as an
//! intent on the log (`ark_peer::Peer::change`); a play the reconciler
//! counts, and where an episode has got to, are the log's.

use ark::format::Encode;
use ark::row::Input;

use crate::types::*;

macro_rules! changes {
    ($( $(#[$doc:meta])* $v:ident($t:ty) = $name:literal, )*) => {
        /// A durable change of harken's, by the function that makes it.
        #[derive(Clone, Debug, PartialEq, Eq)]
        pub enum HarkenChange {
            $( $(#[$doc])* $v($t), )*
        }

        impl HarkenChange {
            /// The function it is, by its name (`playlists.create_playlist`).
            pub fn name(&self) -> &'static str {
                match self {
                    $( HarkenChange::$v(_) => $name, )*
                }
            }

            /// Its input, as the peer takes it: the value format's bytes.
            pub fn input(&self) -> Vec<u8> {
                match self {
                    $( HarkenChange::$v(i) => i.to_bytes(), )*
                }
            }

            /// Why it would be refused before it is sent: the first of its
            /// input's refinements it fails, in the domain's words ("a
            /// playlist needs a name"); none when it passes them.
            pub fn problem(&self) -> Option<String> {
                match self {
                    $( HarkenChange::$v(i) => first(<$t as Input>::check(i.clone())), )*
                }
            }

            /// The change `name` (a function's full name) of `input`'s
            /// bytes; none for a function that is not one, or bytes that
            /// do not read as its input.
            pub fn of(name: &str, input: &[u8]) -> Option<HarkenChange> {
                match name {
                    $( $name => <$t>::from_bytes(input).ok().map(HarkenChange::$v), )*
                    _ => None,
                }
            }

            /// One of each, every input its default.
            pub fn every() -> Vec<HarkenChange> {
                vec![$( HarkenChange::$v(<$t>::default()), )*]
            }
        }
    };
}

fn first<T>(r: Result<T, Vec<(String, String)>>) -> Option<String> {
    r.err().and_then(|mut e| (!e.is_empty()).then(|| e.remove(0).1))
}

changes! {
    /// A playlist made, by its name (a taken name is numbered).
    CreatePlaylist(CreatePlaylist) = "playlists.create_playlist",
    /// Something put on a playlist.
    AddToPlaylist(AddToPlaylist) = "playlists.add_to_playlist",
    /// Everything in the library put on a playlist.
    AddAllToPlaylist(AddAllToPlaylist) = "playlists.add_all_to_playlist",
    /// Something taken off a playlist.
    RemoveFromPlaylist(RemoveFromPlaylist) = "playlists.remove_from_playlist",
    /// A podcast's or a series of sermons' feed followed.
    Subscribe(Subscribe) = "library.subscribe",
    /// A feed followed no more.
    Unsubscribe(Unsubscribe) = "library.unsubscribe",
    /// A play the reconciler counted.
    RecordPlay(RecordPlay) = "listening.record_play",
    /// Where an episode has got to, for its listener.
    ReportProgress(ReportProgress) = "library.report_progress",
    // The listening session's presses, live calls.
    /// A queue played, from one of its tracks.
    Start(Start) = "listening.start",
    Play(()) = "listening.play",
    Pause(()) = "listening.pause",
    Stop(()) = "listening.stop",
    Seek(Seek) = "listening.seek",
    Skip(Skip) = "listening.skip",
    Jump(Jump) = "listening.jump",
    Enqueue(Enqueue) = "listening.enqueue",
    Remove(Remove) = "listening.remove",
    Move(Move) = "listening.move",
    ClearUpcoming(()) = "listening.clear_upcoming",
    /// Where this device's scrubber is being dragged to (ephemeral).
    Scrub(Scrub) = "listening.scrub",
    /// The sound handed to another device.
    Transfer(Transfer) = "listening.transfer",
    Join(Member) = "listening.join",
    Unjoin(Member) = "listening.unjoin",
    SetVolume(SetVolume) = "listening.set_volume",
    NudgeVolume(NudgeVolume) = "listening.nudge_volume",
    SetMute(SetMute) = "listening.set_mute",
    SetGroupVolume(GroupVolume) = "listening.set_group_volume",
    NudgeGroupVolume(GroupNudge) = "listening.nudge_group_volume",
    SetGroupMute(GroupMute) = "listening.set_group_mute",
    // What the output's reconciler says of its player, live calls.
    Equip(Equip) = "listening.equip",
    ReportVolume(ReportVolume) = "listening.report_volume",
    ReportGroup(ReportGroup) = "listening.report_group",
    Started(At) = "listening.started",
    Advance(Advance) = "listening.advance",
    Stalled(At) = "listening.stalled",
    ReportPosition(Report) = "listening.report_position",
    Release(()) = "listening.release",
}
