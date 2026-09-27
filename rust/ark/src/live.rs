//! §14 Live rooms, as `Ark.Live` defines them: what is true now.
//!
//! The second channel on the one socket. A room is one account: every
//! connection the authority verified as the same user. The engine's part is
//! which connections are in which room, what the app's [`Machine`] is told
//! when one arrives, speaks or leaves, and the one row a room may ask to
//! keep when it empties. The engine never looks inside a frame.

use std::collections::BTreeMap;

pub type Room = String;

/// A connection, as the transport numbers them; never reused within one
/// authority's life.
pub type ConnId = i64;

/// One connection's standing in a room: which connection, which room, and
/// who it is within the room.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Peer {
    pub conn: ConnId,
    pub room: Room,
    pub who: String,
}

/// What an app's machine hands back after hearing something: frames to
/// deliver, each to one connection, and whether the room's state is worth a
/// row when it empties.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Post {
    pub out: Vec<(ConnId, Vec<u8>)>,
    pub keep: bool,
}

/// The app's machine for one room, over opaque frames. Pure: it takes what
/// happened and says what follows (`Ark.Live.Machine`).
pub trait Machine {
    type State;
    /// A peer joined; `peers` already includes it.
    fn join(&self, peers: &[Peer], p: &Peer, s: Self::State) -> (Self::State, Post);
    fn say(&self, peers: &[Peer], p: &Peer, frame: &[u8], s: Self::State) -> (Self::State, Post);
    /// A peer left; `peers` no longer includes it.
    fn part(&self, peers: &[Peer], p: &Peer, s: Self::State) -> (Self::State, Post);
    /// What to write when the room empties; `None` for nothing.
    fn snapshot(&self, s: &Self::State) -> Option<Vec<u8>>;
    /// A fresh room, or one woken from what was kept.
    fn wake(&self, kept: Option<&[u8]>) -> Self::State;
}

/// A machine that does nothing: the simulation is about the log.
pub struct Silent;

impl Machine for Silent {
    type State = ();
    fn join(&self, _: &[Peer], _: &Peer, s: ()) -> ((), Post) {
        (s, Post::default())
    }
    fn say(&self, _: &[Peer], _: &Peer, _: &[u8], s: ()) -> ((), Post) {
        (s, Post::default())
    }
    fn part(&self, _: &[Peer], _: &Peer, s: ()) -> ((), Post) {
        (s, Post::default())
    }
    fn snapshot(&self, _: &()) -> Option<Vec<u8>> {
        None
    }
    fn wake(&self, _: Option<&[u8]>) {}
}

/// Every room open on an authority, and the kept snapshot of the ones that
/// are not.
pub struct Rooms<S> {
    pub open: BTreeMap<Room, (S, Vec<Peer>)>,
    pub kept: BTreeMap<Room, Vec<u8>>,
    pub by_conn: BTreeMap<ConnId, Peer>,
}

impl<S> Default for Rooms<S> {
    fn default() -> Self {
        Rooms {
            open: BTreeMap::new(),
            kept: BTreeMap::new(),
            by_conn: BTreeMap::new(),
        }
    }
}

impl<S> Rooms<S> {
    pub fn new() -> Self {
        Self::default()
    }

    /// The peer a connection stands as, if it is in a room.
    pub fn room_of(&self, c: ConnId) -> Option<&Peer> {
        self.by_conn.get(&c)
    }
}

/// §14.1 A connection joins a room, as this participant. A connection
/// already standing as exactly this peer is left where it is and the
/// machine hears nothing; one standing elsewhere departs there first.
pub fn arrive<M: Machine>(m: &M, rs: &mut Rooms<M::State>, p: Peer) -> Post {
    match rs.by_conn.get(&p.conn) {
        Some(here) if *here == p => return Post::default(),
        Some(_) => {
            depart(m, rs, p.conn);
        }
        None => {}
    }
    let (s0, mut peers) = match rs.open.remove(&p.room) {
        Some(open) => open,
        None => (m.wake(rs.kept.get(&p.room).map(|b| b.as_slice())), vec![]),
    };
    peers.push(p.clone());
    let (s1, post) = m.join(&peers, &p, s0);
    rs.open.insert(p.room.clone(), (s1, peers));
    rs.kept.remove(&p.room);
    rs.by_conn.insert(p.conn, p);
    post
}

/// §14.2 A connection speaks. A connection in no room is ignored: a frame
/// is dropped, never queued.
pub fn speak<M: Machine>(m: &M, rs: &mut Rooms<M::State>, c: ConnId, frame: &[u8]) -> Post {
    let Some(p) = rs.by_conn.get(&c).cloned() else {
        return Post::default();
    };
    let Some((s, peers)) = rs.open.remove(&p.room) else {
        return Post::default();
    };
    let (s2, post) = m.say(&peers, &p, frame, s);
    rs.open.insert(p.room.clone(), (s2, peers));
    post
}

/// §14.3 A connection is gone. The machine is told; if it was the last one
/// in the room, the room's snapshot is kept (if the machine offers one) and
/// the room is dropped from memory.
pub fn depart<M: Machine>(m: &M, rs: &mut Rooms<M::State>, c: ConnId) -> Post {
    let Some(p) = rs.by_conn.remove(&c) else {
        return Post::default();
    };
    let Some((s, peers)) = rs.open.remove(&p.room) else {
        return Post::default();
    };
    let peers: Vec<Peer> = peers.into_iter().filter(|q| q.conn != c).collect();
    let (s2, post) = m.part(&peers, &p, s);
    if peers.is_empty() {
        match m.snapshot(&s2) {
            Some(b) => {
                rs.kept.insert(p.room.clone(), b);
            }
            None => {
                rs.kept.remove(&p.room);
            }
        }
    } else {
        rs.open.insert(p.room.clone(), (s2, peers));
    }
    post
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A machine that counts arrivals, keeps the count, and echoes a frame
    /// to everyone else in the room.
    struct Counter;

    impl Machine for Counter {
        type State = u8;
        fn join(&self, _: &[Peer], _: &Peer, s: u8) -> (u8, Post) {
            (s + 1, Post::default())
        }
        fn say(&self, peers: &[Peer], p: &Peer, frame: &[u8], s: u8) -> (u8, Post) {
            (
                s,
                Post {
                    out: peers.iter().filter(|q| q.conn != p.conn).map(|q| (q.conn, frame.to_vec())).collect(),
                    keep: false,
                },
            )
        }
        fn part(&self, _: &[Peer], _: &Peer, s: u8) -> (u8, Post) {
            (s, Post::default())
        }
        fn snapshot(&self, s: &u8) -> Option<Vec<u8>> {
            Some(vec![*s])
        }
        fn wake(&self, kept: Option<&[u8]>) -> u8 {
            kept.map(|b| b[0]).unwrap_or(0)
        }
    }

    fn peer(c: ConnId, who: &str) -> Peer {
        Peer {
            conn: c,
            room: "alice".into(),
            who: who.into(),
        }
    }

    #[test]
    fn a_repeated_hello_is_paging_and_a_room_wakes_from_what_it_kept() {
        let m = Counter;
        let mut rs = Rooms::new();
        arrive(&m, &mut rs, peer(1, "phone"));
        arrive(&m, &mut rs, peer(1, "phone"));
        arrive(&m, &mut rs, peer(2, "laptop"));
        assert_eq!(rs.open["alice"].0, 2);
        assert_eq!(speak(&m, &mut rs, 1, b"hi").out, vec![(2, b"hi".to_vec())]);
        assert_eq!(speak(&m, &mut rs, 9, b"hi"), Post::default());
        depart(&m, &mut rs, 1);
        depart(&m, &mut rs, 2);
        assert!(rs.open.is_empty());
        assert_eq!(rs.kept["alice"], vec![2]);
        arrive(&m, &mut rs, peer(3, "tv"));
        assert_eq!(rs.open["alice"].0, 3);
        assert!(rs.kept.is_empty());
        // A hello as somebody else on the same connection is a change of rooms.
        arrive(
            &m,
            &mut rs,
            Peer {
                conn: 3,
                room: "bob".into(),
                who: "tv".into(),
            },
        );
        assert!(!rs.open.contains_key("alice"));
        assert_eq!(rs.room_of(3).map(|p| p.room.as_str()), Some("bob"));
    }
}
