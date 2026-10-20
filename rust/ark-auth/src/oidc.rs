//! The relying party: one OpenID Connect client, which is the server —
//! the `openidconnect` crate over the `ureq` agent this crate already
//! speaks HTTP with.
//!
//! Authorization code flow with PKCE (S256), as a confidential client. The
//! provider is found from its issuer URL (discovery, and its key set), the
//! code is exchanged at its token endpoint with the client secret, and the
//! ID token that comes back says who signed in. The crate checks the ID
//! token whole: its signature against the provider's keys (fetched again
//! at every exchange, so a provider's key rotation is followed without a
//! restart), its issuer exactly, its audience (this client, and no other
//! audience it does not trust), its expiry, and that its nonce is the one
//! this login started with — which is what ties the token to the browser
//! that asked, and not to a code somebody replayed.

use openidconnect::core::{CoreAuthenticationFlow, CoreClient, CoreJsonWebKeySet, CoreProviderMetadata, CoreUserInfoClaims};
use openidconnect::{
    AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointMaybeSet, EndpointNotSet, EndpointSet, IssuerUrl, Nonce, OAuth2TokenResponse,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, Scope, TokenResponse,
};

use crate::Account;

/// A provider this server can send people to and get identities back from.
#[derive(Clone)]
pub struct Provider {
    /// Boxed: discovery and the key set are kilobytes, and a `Mode` holds this.
    meta: Box<CoreProviderMetadata>,
    client_id: String,
    client_secret: String,
    scopes: Vec<String>,
}

impl std::fmt::Debug for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Provider")
            .field("issuer", &self.issuer())
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

/// What a login started with, and what the callback has to match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Challenge {
    pub state: String,
    pub nonce: String,
    pub verifier: String,
}

impl Challenge {
    pub fn new() -> Self {
        use crate::session::random_token;
        Challenge {
            state: random_token(),
            nonce: random_token(),
            verifier: random_token(),
        }
    }
}

impl Default for Challenge {
    fn default() -> Self {
        Self::new()
    }
}

/// A client secret from a file — a systemd credential, a sops or agenix
/// secret — with the trailing newline an editor leaves taken off. The
/// secret never goes in a command line, an environment or the nix store.
pub fn read_secret(path: impl AsRef<std::path::Path>) -> Result<String, String> {
    let path = path.as_ref();
    let s = std::fs::read_to_string(path).map_err(|e| format!("the client secret {}: {e}", path.display()))?;
    let s = s.trim_end_matches(['\n', '\r']).to_string();
    match s.is_empty() {
        true => Err(format!("the client secret {} is empty", path.display())),
        false => Ok(s),
    }
}

/// The agent every request to the provider goes through: no redirects
/// followed (the crate's advice, against a server-side request forged by a
/// provider's answer), fifteen seconds at most.
fn http() -> ureq::Agent {
    ureq::AgentBuilder::new().redirects(0).timeout(std::time::Duration::from_secs(15)).build()
}

type Client = CoreClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointMaybeSet, EndpointMaybeSet>;

/// An error and every cause under it, on one line.
fn said(e: &dyn std::error::Error) -> String {
    let mut out = e.to_string();
    let mut at = e.source();
    while let Some(s) = at {
        out.push_str(&format!(": {s}"));
        at = s.source();
    }
    out
}

impl Provider {
    /// [`Provider::discover`] with the secret read from a file.
    pub fn discover_with_secret_file(
        issuer: &str,
        client_id: &str,
        secret_file: impl AsRef<std::path::Path>,
        scopes: &[&str],
    ) -> Result<Self, String> {
        Provider::discover(issuer, client_id, &read_secret(secret_file)?, scopes)
    }

    /// Read `{issuer}/.well-known/openid-configuration` and the key set it
    /// names; the issuer it says must be `issuer` exactly. Blocking: call it
    /// at startup, or from a blocking task.
    pub fn discover(issuer: &str, client_id: &str, client_secret: &str, scopes: &[&str]) -> Result<Self, String> {
        let url = IssuerUrl::new(issuer.to_string()).map_err(|e| format!("the issuer {issuer}: {e}"))?;
        let meta = CoreProviderMetadata::discover(&url, &http()).map_err(|e| format!("discovering {issuer}: {}", said(&e)))?;
        Ok(Provider {
            meta: Box::new(meta),
            client_id: client_id.into(),
            client_secret: client_secret.into(),
            scopes: scopes.iter().map(|s| s.to_string()).collect(),
        })
    }

    pub fn issuer(&self) -> &str {
        self.meta.issuer().as_str()
    }

    fn client(&self, meta: CoreProviderMetadata, callback: &str) -> Result<Client, String> {
        let secret = Some(ClientSecret::new(self.client_secret.clone()));
        let redirect = RedirectUrl::new(callback.into()).map_err(|e| format!("the callback {callback}: {e}"))?;
        Ok(CoreClient::from_provider_metadata(meta, ClientId::new(self.client_id.clone()), secret).set_redirect_uri(redirect))
    }

    /// Where to send the person, for `challenge`, with the provider coming
    /// back to `callback`.
    pub fn authorize_url(&self, challenge: &Challenge, callback: &str) -> Result<String, String> {
        let pkce = PkceCodeChallenge::from_code_verifier_sha256(&PkceCodeVerifier::new(challenge.verifier.clone()));
        let client = self.client((*self.meta).clone(), callback)?;
        let (state, nonce) = (challenge.state.clone(), challenge.nonce.clone());
        let mut req = client.authorize_url(
            CoreAuthenticationFlow::AuthorizationCode,
            move || CsrfToken::new(state),
            move || Nonce::new(nonce),
        );
        for s in self.scopes.iter().filter(|s| *s != "openid") {
            req = req.add_scope(Scope::new(s.clone()));
        }
        Ok(req.set_pkce_challenge(pkce).url().0.to_string())
    }

    /// Trade the code the provider sent back for who signed in. Blocking.
    pub fn exchange(&self, code: &str, challenge: &Challenge, callback: &str) -> Result<Account, String> {
        let http = http();
        let keys = CoreJsonWebKeySet::fetch(self.meta.jwks_uri(), &http).map_err(|e| format!("the provider's keys: {}", said(&e)))?;
        let client = self.client((*self.meta).clone().set_jwks(keys), callback)?;
        let tokens = (client.exchange_code(AuthorizationCode::new(code.into())))
            .map_err(|e| said(&e))?
            .set_pkce_verifier(PkceCodeVerifier::new(challenge.verifier.clone()))
            .request(&http)
            .map_err(|e| format!("the token endpoint: {}", said(&e)))?;
        let token = tokens.id_token().ok_or("the provider sent no ID token")?;
        let claims =
            (token.claims(&client.id_token_verifier(), &Nonce::new(challenge.nonce.clone()))).map_err(|e| format!("the ID token: {}", said(&e)))?;
        let name = |n: Option<&openidconnect::LocalizedClaim<openidconnect::EndUserName>>, u: Option<&openidconnect::EndUserUsername>| {
            n.and_then(|n| n.get(None))
                .map(|n| n.to_string())
                .or(u.map(|u| u.to_string()))
                .unwrap_or_default()
        };
        let mut account = Account {
            id: claims.subject().to_string(),
            name: name(claims.name(), claims.preferred_username()),
            email: claims.email().map_or(String::new(), |e| e.to_string()),
            roles: vec![],
        };
        // A provider that keeps the ID token small says the rest at
        // userinfo, which must name the same subject.
        if account.name.is_empty() || account.email.is_empty() {
            let asked = client.user_info(tokens.access_token().clone(), Some(claims.subject().clone()));
            let info: Option<CoreUserInfoClaims> = asked.ok().and_then(|r| r.request(&http).ok());
            if let Some(info) = info {
                if account.name.is_empty() {
                    account.name = name(info.name(), info.preferred_username());
                }
                if account.email.is_empty() {
                    account.email = info.email().map_or(String::new(), |e| e.to_string());
                }
            }
        }
        if account.id.is_empty() {
            return Err("the ID token names nobody".into());
        }
        Ok(account)
    }
}
