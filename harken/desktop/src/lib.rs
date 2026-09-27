//! harken's terminal peer, as a library so that the peer can be run
//! headless: [`peer`] is the engine and its files, [`domain`] the generated
//! code and the names the screens use, [`net`] the one socket, [`ui`] the
//! screens, [`storage`] the files.

pub mod domain;
pub mod net;
pub mod peer;
pub mod storage;
pub mod ui;
