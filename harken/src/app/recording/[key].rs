/// One performance, by its key (the work's, `@`, who played it), and who
/// played it, the only thing that tells two of them apart.
#[derive(ark_router::Route)]
pub struct Recording {
    pub key: String,
    pub title: String,
}
