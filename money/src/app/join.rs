/// Sign in to a server from a link or a QR code: `/join/?server=…&code=…`.
/// Not a page: a shell hands it to the onboarding.
#[derive(ark_router::Route)]
pub struct Join {
    #[query]
    pub server: String,
    #[query]
    pub code: Option<String>,
}
