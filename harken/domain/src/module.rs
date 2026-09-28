//! The module: every router, in order. `module().emit()` is `harken.ark`.
use ark::authoring::*;

use crate::library::library;
use crate::playlists::playlists;

pub fn module() -> Module {
    Module::new((library(), playlists()))
}
