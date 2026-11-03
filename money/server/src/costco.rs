//! Costco, signed in to as its Android app is: Costco's sign-in (Azure AD
//! B2C) with the app's client, an authorization code with PKCE, and a
//! refresh token that is kept and replaced at every use.
//!
//! - **Signing in, by the server** ([`Costco::sign_in_with_password`], from
//!   the app's Settings): the email and password sent through Costco's
//!   sign-in page as a browser sends them, and the redirect to the app's
//!   address, `msauth://com.costco.app.android/…?code=…`, read rather than
//!   followed; its code traded by [`Costco::finish`]. The password is kept
//!   with the refresh token, to sign in again with.
//! - **Signing in, on Costco's page** ([`Costco::sign_in`]), for when
//!   Costco asks for more than the password: Settings opens Costco's
//!   sign-in in a tab, where the password, a passkey or a one-time code are
//!   Costco's own business, and the `msauth://` address it ends at is pasted
//!   back. The code is no good without the verifier the server kept, so a
//!   pasted address is nobody else's to use.
//! - **Each sync** ([`Costco::receipts`]): the refresh token signs in, the
//!   new one Costco answers with is kept in its place (the vault's, or
//!   `MONEY_COSTCO_REFRESH_TOKEN_FILE`), and the id token asks the Orders &
//!   Purchases page's GraphQL for the receipts of the last
//!   `MONEY_COSTCO_DAYS`, item by item, for `money_domain::stores::costco`.
//!   A token Costco stops taking is replaced by signing in with the
//!   password again, once.
//!
//! None of it is a published API: the client, the address and the headers
//! are what Costco's app and web pages were seen to send.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use sha2::{Digest, Sha256};

use crate::vault::{CostcoLogin, Vault};

/// Costco's sign-in, the policy its Android app signs in with.
const B2C: &str = "https://signin.costco.com/e0714dd4-784d-46d6-a278-3e29553483eb/B2C_1A_SSO_WCS_signup_signin_89/oauth2/v2.0";

/// The client Costco's app and web pages sign in as; the receipts API is
/// told it too.
pub const CLIENT_ID: &str = "4900eb1f-0c10-4bd9-99c3-c59e6c1ecebf";

/// Where Costco sends the app back: the one address registered for the
/// client that a server can be handed.
pub const REDIRECT_URI: &str = "msauth://com.costco.app.android/G%2F8zOiF35CG594p1zkZ4A5w8KnQ%3D";

const SCOPE: &str = "openid offline_access";

/// Costco's sign-in and receipts, and how far back.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Costco {
    /// A file of the refresh token, kept in place of the vault's: for a
    /// token put there by hand.
    pub token_file: Option<PathBuf>,
    /// How far back receipts are asked for.
    pub days: u32,
    pub authorize_url: String,
    pub token_url: String,
    pub receipts_url: String,
}

impl Default for Costco {
    fn default() -> Self {
        Costco {
            token_file: None,
            days: 90,
            authorize_url: format!("{B2C}/authorize"),
            token_url: format!("{B2C}/token"),
            receipts_url: "https://ecom-api.costco.com/ebusiness/order/v1/orders/graphql".into(),
        }
    }
}

/// A sign-in under way: the address the page opens, and what its code is
/// traded with.
#[derive(Clone)]
pub struct SignIn {
    pub url: String,
    state: String,
    verifier: String,
}

/// What Costco's sign-in answers with.
struct Tokens {
    id_token: String,
    refresh_token: Option<String>,
}

/// The GraphQL query Costco's web app lists receipts with, but asking for
/// each item's lines as its single receipt's page does: the list's own
/// asks for item numbers alone, and the adapter needs every line's amount.
const QUERY: &str = r#"query receiptsWithCounts($startDate: String!, $endDate: String!,$documentType:String!,$documentSubType:String!) {
    receiptsWithCounts(startDate: $startDate, endDate: $endDate,documentType:$documentType,documentSubType:$documentSubType) {
    inWarehouse
    gasStation
    carWash
    gasAndCarWash
    receipts{
    warehouseName warehouseShortName receiptType documentType transactionDateTime transactionDate transactionBarcode transactionType total
    totalItemCount
    itemArray {
      itemNumber
      itemDescription01
      itemDescription02
      itemIdentifier
      unit
      amount
      fuelUnitQuantity
      itemUnitPriceAmount
    }
    tenderArray {
      tenderTypeCode
      tenderDescription
      amountTender
    }
    couponArray {
      upcnumberCoupon
    }
  }
}
  }"#;

impl Costco {
    /// A new sign-in: Costco's page for it, with a fresh verifier and state.
    /// Its form every time (`prompt=login`): a browser Costco remembers
    /// would otherwise be sent straight on to the app's address, which it
    /// cannot open, and be left on a blank page with nothing to do.
    pub fn sign_in(&self) -> Result<SignIn> {
        let verifier = random(32)?;
        let state = random(16)?;
        let challenge = b64(&Sha256::digest(verifier.as_bytes()));
        let url = format!(
            "{}?client_id={CLIENT_ID}&redirect_uri={}&scope={}&response_type=code&prompt=login&code_challenge={challenge}&code_challenge_method=S256&state={state}",
            self.authorize_url,
            urlencode(REDIRECT_URI),
            urlencode(SCOPE),
        );
        Ok(SignIn { url, state, verifier })
    }

    /// The sign-in finished: the address Costco sent the browser to (or
    /// its code alone) traded for the login kept.
    pub fn finish(&self, s: &SignIn, pasted: &str) -> Result<CostcoLogin> {
        let (code, state) = pasted_code(pasted)?;
        if state.is_some_and(|st| st != s.state) {
            bail!("that address is from another sign-in: sign in to Costco again");
        }
        let t = self.token(&[
            ("grant_type", "authorization_code"),
            ("client_id", CLIENT_ID),
            ("code", &code),
            ("redirect_uri", REDIRECT_URI),
            ("code_verifier", &s.verifier),
            ("scope", SCOPE),
        ])?;
        Ok(CostcoLogin {
            who: email(&t.id_token).unwrap_or_default(),
            refresh_token: t.refresh_token.ok_or_else(|| anyhow!("Costco's sign-in gave no refresh token"))?,
            ..CostcoLogin::default()
        })
    }

    /// Signed in by the server itself, as a browser on Costco's sign-in page
    /// is: the page fetched for its session, the email and password sent as
    /// its form sends them, and the sign-in confirmed, whose answer is the
    /// redirect to the app's address — read here, never followed — whose
    /// code [`Costco::finish`] trades. A step Costco asks for beyond the
    /// password (a code sent by email) is said, not taken: its own page is
    /// the way then.
    pub fn sign_in_with_password(&self, email: &str, password: &str) -> Result<CostcoLogin> {
        let s = self.sign_in()?;
        let mut b = Session::of(&self.authorize_url)?;
        let r = b.get(&s.url)?;
        if let Some(to) = location(&r) {
            return self.finished(&s, &to, email, password);
        }
        let html = r.into_string().map_err(|e| anyhow!("reading Costco's sign-in page: {e}"))?;
        let p = Page::read(&html)?;
        let ids: Vec<String> = fields(&html).into_iter().map(|(id, _)| id).collect();
        if !(ids.iter().any(|i| i == "signInName") && ids.iter().any(|i| i == "password")) {
            bail!("Costco's sign-in page asks for {}, not an email and password", ids.join(", "));
        }
        let tx = urlencode(&p.trans_id);
        let asserted = format!("{}{}/SelfAsserted?tx={tx}&p={}", b.origin, p.tenant, p.policy);
        let form = [("request_type", "RESPONSE"), ("signInName", email), ("password", password)];
        let said: serde_json::Value = b
            .post(&asserted, &p.csrf, &form)?
            .into_json()
            .map_err(|e| anyhow!("reading Costco's sign-in: {e}"))?;
        if said["status"].as_str() != Some("200") {
            bail!("Costco said: {}", said["message"].as_str().unwrap_or("the sign-in was refused"));
        }
        let confirmed = format!(
            "{}{}/api/{}/confirmed?rememberMe=false&csrf_token={}&tx={tx}&p={}",
            b.origin,
            p.tenant,
            p.api,
            urlencode(&p.csrf),
            p.policy
        );
        let r = b.get(&confirmed)?;
        match location(&r) {
            Some(to) => self.finished(&s, &to, email, password),
            None => {
                let html = r.into_string().unwrap_or_default();
                let asked: Vec<String> = fields(&html).into_iter().map(|(_, name)| name).collect();
                match asked.is_empty() {
                    true => bail!("Costco did not finish the sign-in: sign in on Costco's page instead"),
                    false => bail!(
                        "Costco asks for more than the password ({}): sign in on Costco's page instead",
                        asked.join(", ")
                    ),
                }
            }
        }
    }

    /// Where the confirmed sign-in went: the app's address, its code traded;
    /// anywhere else is Costco's error, or a sign-in this server cannot
    /// finish.
    fn finished(&self, s: &SignIn, to: &str, email: &str, password: &str) -> Result<CostcoLogin> {
        if !to.starts_with("msauth://") {
            pasted_code(to)?;
            bail!("Costco sent the sign-in elsewhere: sign in on Costco's page instead");
        }
        let mut login = self.finish(s, to)?;
        if login.who.is_empty() {
            login.who = email.into();
        }
        (login.email, login.password) = (email.into(), Some(password.into()));
        Ok(login)
    }

    /// The receipts since `days`, as the adapter reads them; none when
    /// nobody has signed in. A refresh token Costco no longer takes is
    /// replaced by signing in again with the password kept, once; when
    /// that is refused too, the password is forgotten, so the account is
    /// not tried with it again and again.
    pub fn receipts(&self, data: &Path) -> Result<Option<String>> {
        let (used, kept) = match &self.token_file {
            Some(file) => (crate::stores::secret(file)?, None),
            None => match Vault::load(data)?.costco {
                Some(l) => (l.refresh_token.clone(), Some(l)),
                None => return Ok(None),
            },
        };
        let refresh = |token: &str| {
            self.token(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", token),
                ("scope", SCOPE),
            ])
        };
        let (t, used) = match (refresh(&used), kept) {
            (Ok(t), _) => (t, used),
            (
                Err(e),
                Some(CostcoLogin {
                    email,
                    password: Some(password),
                    ..
                }),
            ) => {
                let again = self.sign_in_with_password(&email, &password).and_then(|l| {
                    let t = refresh(&l.refresh_token)?;
                    Ok((l, t))
                });
                match again {
                    Ok((l, t)) => {
                        let token = l.refresh_token.clone();
                        Vault::change(data, |v| {
                            v.costco = Some(l);
                            Ok(())
                        })?;
                        (t, token)
                    }
                    Err(again) => {
                        Vault::change(data, |v| {
                            if let Some(l) = v.costco.as_mut().filter(|l| l.refresh_token == used) {
                                l.password = None;
                            }
                            Ok(())
                        })?;
                        bail!("{e:#}, and signing in again: {again:#}; sign in to Costco again in the app's Settings");
                    }
                }
            }
            (Err(e), _) => bail!("{e:#}; sign in to Costco again in the app's Settings"),
        };
        if let Some(new) = &t.refresh_token {
            self.keep(data, &used, new)?;
        }

        let now = chrono::Utc::now();
        let start = now - chrono::Duration::days(self.days as i64);
        let request = serde_json::json!({
            "query": QUERY,
            "variables": {
                "startDate": start.format("%-m/%-d/%Y").to_string(),
                "endDate": now.format("%-m/%-d/%Y").to_string(),
                "documentType": "all",
                "documentSubType": "all"
            }
        });
        let response = agent()
            .post(&self.receipts_url)
            .set("Content-Type", "application/json-patch+json")
            .set("costco.env", "PROD")
            .set("costco-x-authorization", &format!("Bearer {}", t.id_token))
            .set("costco-x-wcs-clientId", CLIENT_ID)
            .set("client-identifier", "481b1aec-aa3b-454b-b81b-48187e28f205")
            .set("costco.service", "restOrders")
            .send_string(&request.to_string())
            .map_err(|e| anyhow!("Costco's receipts: {}", said(e)))?;
        let body: serde_json::Value = response.into_json().map_err(|e| anyhow!("reading Costco's receipts: {e}"))?;
        if !body["data"]["receiptsWithCounts"].is_object() {
            bail!("Costco did not return receipts");
        }
        Ok(Some(body["data"].to_string()))
    }

    /// The refresh token Costco replaced `used` with, kept where `used`
    /// was: unless a new sign-in has put another there meanwhile.
    fn keep(&self, data: &Path, used: &str, new: &str) -> Result<()> {
        match &self.token_file {
            Some(file) => {
                let tmp = file.with_extension("tmp");
                std::fs::write(&tmp, new).with_context(|| format!("writing {}", tmp.display()))?;
                std::fs::rename(&tmp, file).with_context(|| format!("replacing {}", file.display()))
            }
            None => Vault::change(data, |v| {
                if let Some(l) = v.costco.as_mut().filter(|l| l.refresh_token == used) {
                    l.refresh_token = new.to_string();
                }
                Ok(())
            }),
        }
    }

    fn token(&self, form: &[(&str, &str)]) -> Result<Tokens> {
        let body = form.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect::<Vec<_>>().join("&");
        let answer: serde_json::Value = agent()
            .post(&self.token_url)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&body)
            .map_err(|e| anyhow!("Costco's sign-in: {}", said(e)))?
            .into_json()
            .map_err(|e| anyhow!("Costco's sign-in: {e}"))?;
        Ok(Tokens {
            id_token: answer["id_token"]
                .as_str()
                .ok_or_else(|| anyhow!("Costco's sign-in gave no id token"))?
                .to_string(),
            refresh_token: answer["refresh_token"].as_str().map(str::to_string),
        })
    }
}

/// How Costco is asked: never waited on for long, since a sign-in holds a
/// call's thread and a sync the importer's.
fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).build()
}

/// What a browser on Costco's sign-in page would say it is.
const BROWSER: &str = "Mozilla/5.0 (Linux; Android 14) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Mobile Safari/537.36";

/// A browser's session with Costco's sign-in: its cookies, by name, and
/// redirects answered rather than followed, since the last of them is to
/// the app's address.
struct Session {
    agent: ureq::Agent,
    origin: String,
    cookies: std::collections::BTreeMap<String, String>,
}

impl Session {
    fn of(url: &str) -> Result<Session> {
        let rest = url.split_once("://").map(|(_, r)| r).ok_or_else(|| anyhow!("{url} is not an address"))?;
        let host = rest.split('/').next().unwrap_or(rest);
        let scheme = &url[..url.find("://").unwrap_or(0)];
        Ok(Session {
            agent: ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).redirects(0).build(),
            origin: format!("{scheme}://{host}"),
            cookies: Default::default(),
        })
    }

    fn cookie(&self) -> String {
        self.cookies.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("; ")
    }

    fn kept(&mut self, r: ureq::Response) -> ureq::Response {
        for set in r.all("set-cookie") {
            let pair = set.split(';').next().unwrap_or("");
            if let Some((name, value)) = pair.split_once('=') {
                self.cookies.insert(name.trim().into(), value.trim().into());
            }
        }
        r
    }

    fn get(&mut self, url: &str) -> Result<ureq::Response> {
        let r = self
            .agent
            .get(url)
            .set("User-Agent", BROWSER)
            .set("Cookie", &self.cookie())
            .call()
            .map_err(|e| anyhow!("Costco's sign-in: {}", said(e)))?;
        Ok(self.kept(r))
    }

    fn post(&mut self, url: &str, csrf: &str, form: &[(&str, &str)]) -> Result<ureq::Response> {
        let body = form.iter().map(|(k, v)| format!("{k}={}", urlencode(v))).collect::<Vec<_>>().join("&");
        let r = self
            .agent
            .post(url)
            .set("User-Agent", BROWSER)
            .set("Cookie", &self.cookie())
            .set("Origin", &self.origin)
            .set("X-CSRF-TOKEN", csrf)
            .set("X-Requested-With", "XMLHttpRequest")
            .set("Content-Type", "application/x-www-form-urlencoded; charset=UTF-8")
            .send_string(&body)
            .map_err(|e| anyhow!("Costco's sign-in: {}", said(e)))?;
        Ok(self.kept(r))
    }
}

/// Where a redirect goes.
fn location(r: &ureq::Response) -> Option<String> {
    (300..400)
        .contains(&r.status())
        .then(|| r.header("location").map(str::to_string))
        .flatten()
}

/// What a B2C page says of itself (`var SETTINGS = {…}`): the step it is,
/// and what its form is sent with.
struct Page {
    api: String,
    csrf: String,
    trans_id: String,
    tenant: String,
    policy: String,
}

impl Page {
    fn read(html: &str) -> Result<Page> {
        let s = object_after(html, "var SETTINGS = ").ok_or_else(|| anyhow!("Costco's sign-in page is not one this server reads"))?;
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_string();
        let page = Page {
            api: text(&s["api"]),
            csrf: text(&s["csrf"]),
            trans_id: text(&s["transId"]),
            tenant: text(&s["hosts"]["tenant"]),
            policy: text(&s["hosts"]["policy"]),
        };
        if page.csrf.is_empty() || page.trans_id.is_empty() || page.tenant.is_empty() {
            bail!("Costco's sign-in page is not one this server reads");
        }
        Ok(page)
    }
}

/// A page's form fields (`var SA_FIELDS = {…}`): each one's id and name.
fn fields(html: &str) -> Vec<(String, String)> {
    let Some(f) = object_after(html, "var SA_FIELDS = ") else {
        return vec![];
    };
    let all = f["AttributeFields"].as_array().cloned().unwrap_or_default();
    all.iter()
        .map(|a| (a["ID"].as_str().unwrap_or("").to_string(), a["DN"].as_str().unwrap_or("").to_string()))
        .filter(|(id, _)| !id.is_empty())
        .collect()
}

/// The JSON object a page's script assigns after `marker`.
fn object_after(html: &str, marker: &str) -> Option<serde_json::Value> {
    let at = html.find(marker)? + marker.len();
    serde_json::Deserializer::from_str(&html[at..])
        .into_iter::<serde_json::Value>()
        .next()?
        .ok()
}

/// What a failed request said: B2C's `error_description` (its first line;
/// the rest is a correlation id), else the status.
fn said(e: ureq::Error) -> String {
    match e {
        ureq::Error::Status(code, r) => {
            let text = r.into_string().unwrap_or_default();
            let why = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error_description"].as_str().map(|d| d.lines().next().unwrap_or("").trim().to_string()));
            match why {
                Some(w) if !w.is_empty() => format!("{w} ({code})"),
                _ => format!("{code}"),
            }
        }
        e => e.to_string(),
    }
}

/// The code (and state) in what was pasted: the whole address, its query,
/// or the code alone.
fn pasted_code(pasted: &str) -> Result<(String, Option<String>)> {
    let text = pasted.trim();
    let query = text.split_once(['?', '#']).map_or(text, |(_, q)| q);
    let fields: Vec<(String, String)> = query
        .split(['&', ' ', '\n'])
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), percent_decode(v)))
        .collect();
    let get = |key: &str| fields.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    if let Some(why) = get("error_description").or_else(|| get("error")) {
        bail!("Costco's sign-in said: {}", why.replace('+', " ").lines().next().unwrap_or(""));
    }
    match get("code") {
        Some(code) if !code.is_empty() => Ok((code, get("state"))),
        _ if !text.is_empty() && !text.contains(['=', '?', ' ', '/']) => Ok((text.to_string(), None)),
        _ => bail!("there is no code in that: paste the whole address beginning msauth://"),
    }
}

fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (
            b[i],
            b.get(i + 1..i + 3)
                .and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()),
        ) {
            (b'%', Some(byte)) => {
                out.push(byte);
                i += 3;
            }
            (c, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A form's or a query's value, escaped the way a browser does.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// Whose an id token is: its email, read without checking its signature,
/// for the page to show and nothing else.
fn email(id_token: &str) -> Option<String> {
    let payload = id_token.split('.').nth(1)?;
    let claims: serde_json::Value = serde_json::from_slice(&base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(payload).ok()?).ok()?;
    claims["email"]
        .as_str()
        .or_else(|| claims["emails"][0].as_str())
        .or_else(|| claims["signInNames.emailAddress"].as_str())
        .map(str::to_string)
}

fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(bytes)
}

fn random(n: usize) -> Result<String> {
    let mut bytes = vec![0; n];
    getrandom::getrandom(&mut bytes).map_err(|e| anyhow!("no randomness: {e}"))?;
    Ok(b64(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    /// A fake Costco on a port: each request answered in turn, and handed
    /// back whole.
    fn serve(answers: Vec<(u16, String)>) -> (u16, std::sync::mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let answers = answers
            .into_iter()
            .map(|(status, body)| -> Answer { Box::new(move |_: &str| (status, vec![], body)) })
            .collect();
        serve_with(answers)
    }

    /// What a fake Costco answers one request with, told the request: a
    /// status, headers beside the content type, and a body.
    type Answer = Box<dyn FnOnce(&str) -> (u16, Vec<(&'static str, String)>, String) + Send>;

    fn serve_with(answers: Vec<Answer>) -> (u16, std::sync::mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sent, got) = std::sync::mpsc::channel();
        let serve = std::thread::spawn(move || {
            for answer in answers {
                let (mut stream, _) = listener.accept().unwrap();
                // The headers, then as much body as they say.
                let mut request = vec![];
                let mut buf = [0; 8192];
                while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                    let read = stream.read(&mut buf).unwrap();
                    assert!(read > 0, "the request ended early");
                    request.extend_from_slice(&buf[..read]);
                }
                let head = request.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
                let length = String::from_utf8_lossy(&request[..head])
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                while request.len() < head + length {
                    let read = stream.read(&mut buf).unwrap();
                    assert!(read > 0, "the body ended early");
                    request.extend_from_slice(&buf[..read]);
                }
                let request = String::from_utf8_lossy(&request).to_string();
                let (status, headers, body) = answer(&request);
                let headers: String = headers.iter().map(|(k, v)| format!("{k}: {v}\r\n")).collect();
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                sent.send(request).unwrap();
            }
        });
        (port, got, serve)
    }

    fn at(port: u16) -> Costco {
        Costco {
            token_file: None,
            days: 30,
            authorize_url: format!("http://127.0.0.1:{port}/authorize"),
            token_url: format!("http://127.0.0.1:{port}/token"),
            receipts_url: format!("http://127.0.0.1:{port}/graphql"),
        }
    }

    /// An id token as B2C writes one, unsigned here: only its email is read.
    fn id_token(email: &str) -> String {
        format!("x.{}.y", b64(serde_json::json!({ "emails": [email] }).to_string().as_bytes()))
    }

    const RECEIPTS: &str = r#"{"data":{"receiptsWithCounts":{"inWarehouse":1,"gasStation":0,"carWash":0,"gasAndCarWash":0,"receipts":[
        {"warehouseName":"SEATTLE","receiptType":"In-Warehouse","documentType":"WarehouseReceiptDetail",
         "transactionDateTime":"2026-10-25T14:33:00","transactionBarcode":"21134300501862310251433","total":10.83,
         "itemArray":[{"itemNumber":"1234","itemDescription01":"KS ORGANIC EGGS","itemDescription02":"24 CT","unit":1,"amount":10.83}]}
    ]}}}"#;

    #[test]
    fn the_sign_in_is_the_apps_with_pkce() {
        let s = Costco::default().sign_in().unwrap();
        assert!(s.url.starts_with("https://signin.costco.com/"), "{}", s.url);
        assert!(s.url.contains(&format!("client_id={CLIENT_ID}")));
        assert!(s
            .url
            .contains("redirect_uri=msauth%3A%2F%2Fcom.costco.app.android%2FG%252F8zOiF35CG594p1zkZ4A5w8KnQ%253D"));
        assert!(s.url.contains("scope=openid%20offline_access"));
        let challenge = b64(&Sha256::digest(s.verifier.as_bytes()));
        assert!(s.url.contains(&format!("code_challenge={challenge}&code_challenge_method=S256")));
        assert!(s.url.contains(&format!("state={}", s.state)));
        assert!(s.url.contains("&prompt=login&"), "the form every time: {}", s.url);
        assert_ne!(s.state, Costco::default().sign_in().unwrap().state, "each sign-in its own");
    }

    #[test]
    fn a_pasted_address_gives_its_code() {
        let addr = "msauth://com.costco.app.android/G%2F8zOiF35CG594p1zkZ4A5w8KnQ%3D?state=abc&code=eyJr%2Bcode";
        assert_eq!(pasted_code(addr).unwrap(), ("eyJr+code".into(), Some("abc".into())));
        assert_eq!(pasted_code("  eyJraWQiOiJjcGlt  ").unwrap(), ("eyJraWQiOiJjcGlt".into(), None));
        let err = pasted_code("msauth://x?error=access_denied&error_description=AADB2C90091%3a+The+user+has+cancelled.%0d%0aCorrelation")
            .unwrap_err()
            .to_string();
        assert!(err.contains("AADB2C90091: The user has cancelled."), "{err}");
        assert!(pasted_code("https://www.costco.com/").is_err());
    }

    #[test]
    fn the_code_is_traded_with_the_verifier_for_a_login() {
        let (port, got, serve) = serve(vec![(
            200,
            format!(r#"{{"id_token":"{}","refresh_token":"the refresh token"}}"#, id_token("me@example.com")),
        )]);
        let costco = at(port);
        let s = costco.sign_in().unwrap();
        let pasted = format!(
            "msauth://com.costco.app.android/G%2F8zOiF35CG594p1zkZ4A5w8KnQ%3D?state={}&code=the-code",
            s.state
        );
        let login = costco.finish(&s, &pasted).unwrap();
        let request = got.recv().unwrap();
        serve.join().unwrap();

        assert_eq!(login.who, "me@example.com");
        assert_eq!(login.refresh_token, "the refresh token");
        assert!(request.starts_with("POST /token "), "{request}");
        assert!(request.contains("grant_type=authorization_code"), "{request}");
        assert!(request.contains("code=the-code"), "{request}");
        assert!(request.contains(&format!("code_verifier={}", s.verifier)), "{request}");
        assert!(request.contains("redirect_uri=msauth%3A%2F%2F"), "{request}");
    }

    #[test]
    fn another_sign_ins_address_is_refused() {
        let costco = at(1);
        let s = costco.sign_in().unwrap();
        let err = costco.finish(&s, "msauth://x?state=not-this-one&code=c").unwrap_err().to_string();
        assert!(err.contains("another sign-in"), "{err}");
    }

    #[test]
    fn costcos_refusal_is_said() {
        let (port, _got, serve) = serve(vec![(
            400,
            r#"{"error":"invalid_grant","error_description":"AADB2C90080: The provided grant has expired. Please re-authenticate and try again.\r\nCorrelation ID: 1"}"#.into(),
        )]);
        let costco = at(port);
        let dir = tempfile::tempdir().unwrap();
        Vault::change(dir.path(), |v| {
            v.costco = Some(CostcoLogin {
                who: "me@example.com".into(),
                refresh_token: "old".into(),
                ..CostcoLogin::default()
            });
            Ok(())
        })
        .unwrap();
        let err = format!("{:#}", costco.receipts(dir.path()).unwrap_err());
        serve.join().unwrap();
        assert!(err.contains("AADB2C90080: The provided grant has expired"), "{err}");
        assert!(err.contains("sign in to Costco again in the app's Settings"), "{err}");
        assert!(!err.contains("Correlation"), "{err}");
    }

    #[test]
    fn nobody_signed_in_reads_nothing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(at(1).receipts(dir.path()).unwrap(), None);
    }

    #[test]
    fn each_sync_signs_in_keeps_the_new_token_and_reads_receipts() {
        let (port, got, serve) = serve(vec![
            (200, r#"{"id_token":"the id token","refresh_token":"the new refresh token"}"#.to_string()),
            (200, RECEIPTS.into()),
        ]);
        let costco = at(port);
        let dir = tempfile::tempdir().unwrap();
        Vault::change(dir.path(), |v| {
            v.costco = Some(CostcoLogin {
                who: "me@example.com".into(),
                refresh_token: "old refresh token".into(),
                ..CostcoLogin::default()
            });
            Ok(())
        })
        .unwrap();
        let out = costco.receipts(dir.path()).unwrap().unwrap();
        let token_request = got.recv().unwrap();
        let receipts_request = got.recv().unwrap();
        serve.join().unwrap();

        assert!(token_request.contains("grant_type=refresh_token"), "{token_request}");
        assert!(token_request.contains("refresh_token=old%20refresh%20token"), "{token_request}");
        assert!(token_request.contains(&format!("client_id={CLIENT_ID}")), "{token_request}");
        assert!(
            receipts_request.contains("costco-x-authorization: Bearer the id token"),
            "{receipts_request}"
        );
        assert!(receipts_request.contains("costco.service: restOrders"), "{receipts_request}");
        assert!(receipts_request.contains("amount"), "the items' lines are asked for: {receipts_request}");
        let kept = Vault::load(dir.path()).unwrap().costco.unwrap();
        assert_eq!(
            (kept.who.as_str(), kept.refresh_token.as_str()),
            ("me@example.com", "the new refresh token")
        );

        // What it gives back is what the domain's adapter reads.
        let import = money_domain::stores::costco(&out).unwrap();
        assert_eq!(import.receipts.len(), 1);
        let r = &import.receipts[0];
        assert_eq!((r.date.as_str(), r.place.as_str(), r.total), ("2026-10-25", "Costco SEATTLE", -1083));
        assert_eq!(r.items[0].name, "KS ORGANIC EGGS 24 CT");
        assert_eq!(r.items[0].amount, -1083);
    }

    #[test]
    fn a_token_file_is_kept_in_place_of_the_vault() {
        let (port, _got, serve) = serve(vec![
            (200, r#"{"id_token":"t","refresh_token":"the new one"}"#.into()),
            (200, RECEIPTS.into()),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("costco-refresh-token");
        std::fs::write(&file, "by hand\n").unwrap();
        let costco = Costco {
            token_file: Some(file.clone()),
            ..at(port)
        };
        assert!(costco.receipts(dir.path()).unwrap().is_some());
        serve.join().unwrap();
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "the new one");
        assert_eq!(Vault::load(dir.path()).unwrap().costco, None);
    }

    /// A B2C page of `fields` (each an id and a name), as Costco's sign-in
    /// serves one.
    fn b2c_page(fields: &[(&str, &str)]) -> String {
        let fields: Vec<_> = fields.iter().map(|(id, dn)| serde_json::json!({ "ID": id, "DN": dn })).collect();
        format!(
            "<html><script>var SETTINGS = {};\nvar SA_FIELDS = {};</script></html>",
            serde_json::json!({
                "api": "CombinedSigninAndSignup",
                "csrf": "the-csrf",
                "transId": "StateProperties=abc",
                "hosts": { "tenant": "/tenant/policy", "policy": "policy" },
            }),
            serde_json::json!({ "AttributeFields": fields }),
        )
    }

    const SIGN_IN: [(&str, &str); 2] = [("signInName", "Sign in name"), ("password", "Password")];

    /// The page, its cookies, and the state its sign-in names, for the
    /// redirect to give back.
    fn the_page(state: std::sync::Arc<std::sync::Mutex<String>>) -> Answer {
        Box::new(move |request: &str| {
            let line = request.lines().next().unwrap_or("");
            let st = line.split("state=").nth(1).unwrap_or("").split([' ', '&']).next().unwrap_or("");
            *state.lock().unwrap() = st.to_string();
            let cookies = vec![
                ("Set-Cookie", "x-ms-cpim-csrf=c1; path=/; secure".to_string()),
                ("Set-Cookie", "x-ms-cpim-trans=t1; path=/".to_string()),
            ];
            (200, cookies, b2c_page(&SIGN_IN))
        })
    }

    fn json(status: u16, body: impl Into<String>) -> Answer {
        let body = body.into();
        Box::new(move |_: &str| (status, vec![], body))
    }

    #[test]
    fn the_server_signs_in_as_the_page_does() {
        let state = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let st = state.clone();
        let (port, got, serve) = serve_with(vec![
            the_page(state.clone()),
            json(200, r#"{"status":"200"}"#),
            Box::new(move |_: &str| {
                let to = format!("{REDIRECT_URI}?state={}&code=the-code", st.lock().unwrap());
                (302, vec![("Location", to)], String::new())
            }),
            json(
                200,
                format!(r#"{{"id_token":"{}","refresh_token":"the refresh token"}}"#, id_token("me@example.com")),
            ),
        ]);
        let login = at(port).sign_in_with_password("me@example.com", "hunter 2").unwrap();
        let (page, asserted, confirmed, token) = (got.recv().unwrap(), got.recv().unwrap(), got.recv().unwrap(), got.recv().unwrap());
        serve.join().unwrap();

        assert!(page.starts_with("GET /authorize?client_id="), "{page}");
        assert!(
            asserted.starts_with("POST /tenant/policy/SelfAsserted?tx=StateProperties%3Dabc&p=policy "),
            "{asserted}"
        );
        assert!(
            asserted.contains("x-ms-cpim-csrf=c1") && asserted.contains("x-ms-cpim-trans=t1"),
            "{asserted}"
        );
        assert!(asserted.to_ascii_lowercase().contains("x-csrf-token: the-csrf"), "{asserted}");
        assert!(
            asserted.ends_with("request_type=RESPONSE&signInName=me%40example.com&password=hunter%202"),
            "{asserted}"
        );
        assert!(
            confirmed.starts_with("GET /tenant/policy/api/CombinedSigninAndSignup/confirmed?rememberMe=false&csrf_token=the-csrf&tx="),
            "{confirmed}"
        );
        assert!(token.contains("code=the-code"), "{token}");
        assert_eq!(
            (
                login.who.as_str(),
                login.refresh_token.as_str(),
                login.email.as_str(),
                login.password.as_deref()
            ),
            ("me@example.com", "the refresh token", "me@example.com", Some("hunter 2"))
        );
    }

    #[test]
    fn a_wrong_password_is_costcos_to_say() {
        let state = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (port, _got, serve) = serve_with(vec![
            the_page(state),
            json(200, r#"{"status":"400","message":"Your password is incorrect."}"#),
        ]);
        let err = at(port).sign_in_with_password("me@example.com", "nope").unwrap_err().to_string();
        serve.join().unwrap();
        assert_eq!(err, "Costco said: Your password is incorrect.");
    }

    #[test]
    fn a_step_beyond_the_password_is_said() {
        let state = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (port, _got, serve) = serve_with(vec![
            the_page(state),
            json(200, r#"{"status":"200"}"#),
            json(200, b2c_page(&[("verificationCode", "Verification code")])),
        ]);
        let err = at(port).sign_in_with_password("me@example.com", "hunter2").unwrap_err().to_string();
        serve.join().unwrap();
        assert!(err.contains("(Verification code)") && err.contains("Costco's page"), "{err}");
    }

    /// A token Costco stopped taking is replaced by signing in again with
    /// the password kept; when that is refused, the password is forgotten.
    #[test]
    fn a_sync_signs_in_again_with_the_password_kept_once() {
        let expired = r#"{"error":"invalid_grant","error_description":"AADB2C90080: The provided grant has expired."}"#;
        let state = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let st = state.clone();
        let (port, _got, serve) = serve_with(vec![
            json(400, expired),
            the_page(state.clone()),
            json(200, r#"{"status":"200"}"#),
            Box::new(move |_: &str| {
                let to = format!("{REDIRECT_URI}?state={}&code=c", st.lock().unwrap());
                (302, vec![("Location", to)], String::new())
            }),
            json(200, r#"{"id_token":"t","refresh_token":"signed in again"}"#),
            json(200, r#"{"id_token":"t","refresh_token":"refreshed"}"#),
            json(200, RECEIPTS),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let kept = CostcoLogin {
            who: "me@example.com".into(),
            refresh_token: "old".into(),
            email: "me@example.com".into(),
            password: Some("hunter2".into()),
        };
        Vault::change(dir.path(), |v| {
            v.costco = Some(kept.clone());
            Ok(())
        })
        .unwrap();
        assert!(at(port).receipts(dir.path()).unwrap().is_some());
        serve.join().unwrap();
        let now = Vault::load(dir.path()).unwrap().costco.unwrap();
        assert_eq!((now.refresh_token.as_str(), now.password.as_deref()), ("refreshed", Some("hunter2")));

        let state = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let (port, _got, serve) = serve_with(vec![
            json(400, expired),
            the_page(state),
            json(200, r#"{"status":"400","message":"Your password is incorrect."}"#),
        ]);
        Vault::change(dir.path(), |v| {
            v.costco = Some(kept);
            Ok(())
        })
        .unwrap();
        let err = format!("{:#}", at(port).receipts(dir.path()).unwrap_err());
        serve.join().unwrap();
        assert!(err.contains("Your password is incorrect") && err.contains("Settings"), "{err}");
        assert_eq!(Vault::load(dir.path()).unwrap().costco.unwrap().password, None, "not tried again");
    }
}
