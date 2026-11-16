/// One show and its episodes: its key (`podcast/full-time-nix`) and its
/// title.
#[derive(ark_router::Route)]
pub struct Show {
    pub key: String,
    pub title: String,
}
