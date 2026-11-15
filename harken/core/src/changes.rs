//! Every durable change harken's screens send, as one type: what a client
//! sends rather than a function's name and its input's JSON (money's
//! `MoneyChange`'s twin). A [`HarkenChange`] names its function
//! ([`HarkenChange::name`]) and carries its input, which crosses to the
//! peer as its bytes ([`HarkenChange::input`]); its refinements are asked
//! of it before it is sent ([`HarkenChange::problem`]), in the words a form
//! says under its field. The listening session's transitions are not here:
//! they are the live space's, never the log's.

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
}
