//! Costco, signed in to as its Android app is: Costco's sign-in (Azure AD
//! B2C) with the app's client, an authorization code with PKCE, and a
//! refresh token that is kept and replaced at every use.
//!
//! - **Signing in** ([`Costco::sign_in`], from the app's Settings): Settings
//!   opens Costco's sign-in in a tab, where the password, a passkey or a
//!   one-time code are Costco's own business. Costco then sends the browser
//!   to the app's address, `msauth://com.costco.app.android/…?code=…`,
//!   which a browser cannot open; what it tried is pasted back on the page,
//!   and [`Costco::finish`] trades its code for the refresh token. The code
//!   is no good without the verifier the server kept, so a pasted address
//!   is nobody else's to use.
//! - **Each sync** ([`Costco::receipts`]): the refresh token signs in, the
//!   new one Costco answers with is kept in its place (the vault's, or
//!   `MONEY_COSTCO_REFRESH_TOKEN_FILE`), and the id token asks the Orders &
//!   Purchases page's GraphQL for the receipts of the last
//!   `MONEY_COSTCO_DAYS`, item by item, for `money_domain::stores::costco`.
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
    pub fn sign_in(&self) -> Result<SignIn> {
        let verifier = random(32)?;
        let state = random(16)?;
        let challenge = b64(&Sha256::digest(verifier.as_bytes()));
        let url = format!(
            "{}?client_id={CLIENT_ID}&redirect_uri={}&scope={}&response_type=code&code_challenge={challenge}&code_challenge_method=S256&state={state}",
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
        })
    }

    /// The receipts since `days`, as the adapter reads them; none when
    /// nobody has signed in.
    pub fn receipts(&self, data: &Path) -> Result<Option<String>> {
        let used = match &self.token_file {
            Some(file) => crate::stores::secret(file)?,
            None => match Vault::load(data)?.costco {
                Some(l) => l.refresh_token,
                None => return Ok(None),
            },
        };
        let t = self
            .token(&[
                ("grant_type", "refresh_token"),
                ("client_id", CLIENT_ID),
                ("refresh_token", &used),
                ("scope", SCOPE),
            ])
            .map_err(|e| anyhow!("{e:#}; sign in to Costco again in the app's Settings"))?;
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
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sent, got) = std::sync::mpsc::channel();
        let serve = std::thread::spawn(move || {
            for (status, body) in answers {
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
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                sent.send(String::from_utf8_lossy(&request).to_string()).unwrap();
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
}
