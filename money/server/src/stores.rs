//! The stores, beside the banks: the importer's other half. Each run it
//! gathers receipts from where the server was told they are, has the
//! domain's adapters read them (`money_domain::stores`), and authors each
//! store's as `receipts.add_receipts` — which changes nothing for a receipt
//! it already has, so gathering the same ones again is harmless.
//!
//! - **Amazon, through `amazon-orders`** (`MONEY_AMAZON_ORDERS`, the
//!   program): run as `amazon-orders orders --last-30-days --full-details
//!   -o json`, then for how each order was charged — shipment by
//!   shipment, refunds — as `amazon-orders transactions --days 60 -o json`
//!   (`MONEY_AMAZON_TRANSACTIONS_ARGS`); (`MONEY_AMAZON_ARGS` to say
//!   otherwise of the orders, `--year 2026` for a
//!   first pull), signed in by `AMAZON_USERNAME`, `AMAZON_PASSWORD` and,
//!   for a one-time code, `AMAZON_OTP_SECRET_KEY`, each read at the run and
//!   given to that program alone: from the files the server was told
//!   (`MONEY_AMAZON_USERNAME_FILE`, …), else from the login given on the
//!   settings page ([`crate::vault`]). Its home is
//!   `<data>/amazon-orders`, so the session it keeps there spares Amazon a
//!   login each run.
//! - **Costco, through its web API** (`MONEY_COSTCO_REFRESH_TOKEN_FILE`):
//!   the refresh token in that file signs in to Costco's sign-in (Azure AD
//!   B2C), which answers with a new one, written back over it; the id token
//!   then asks the Orders & Purchases page's GraphQL for the warehouse and
//!   gas receipts of the last `MONEY_COSTCO_DAYS` (90), item by item.
//! - **A folder of exports** (`MONEY_RECEIPTS_DIR`): a file in
//!   `<dir>/<store>/` (`target/`, `costco/`, `amazon/`) is read by that
//!   store's adapter, then moved to `imported/` beside it once the import
//!   is in — or to `failed/`, said why, when it does not read.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};

/// How long `amazon-orders` may take: a year's history with every order's
/// details is a page per order.
const AMAZON_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// What the server is told of the stores.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stores {
    pub amazon: Option<AmazonOrders>,
    /// Costco, signed in by the refresh token in its file.
    pub costco: Option<Costco>,
    /// The folder of exports.
    pub dir: Option<PathBuf>,
}

impl Stores {
    pub fn any(&self) -> bool {
        self.amazon.is_some() || self.costco.is_some() || self.dir.is_some()
    }

    /// Read the environment (blank is unset).
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Stores> {
        let amazon = match env("MONEY_AMAZON_ORDERS") {
            None => None,
            Some(program) => {
                let (u, p) = (env("MONEY_AMAZON_USERNAME_FILE"), env("MONEY_AMAZON_PASSWORD_FILE"));
                if u.is_some() != p.is_some() {
                    bail!(
                        "MONEY_AMAZON_USERNAME_FILE and MONEY_AMAZON_PASSWORD_FILE go together; set both, or neither and sign in on the settings page"
                    );
                }
                Some(AmazonOrders {
                    program: program.into(),
                    args: match env("MONEY_AMAZON_ARGS") {
                        Some(a) => a.split_whitespace().map(String::from).collect(),
                        None => DEFAULT_ARGS.iter().map(|s| s.to_string()).collect(),
                    },
                    charges_args: match env("MONEY_AMAZON_TRANSACTIONS_ARGS") {
                        Some(a) => a.split_whitespace().map(String::from).collect(),
                        None => CHARGES_ARGS.iter().map(|s| s.to_string()).collect(),
                    },
                    username_file: u.map(PathBuf::from),
                    password_file: p.map(PathBuf::from),
                    otp_secret_file: env("MONEY_AMAZON_OTP_SECRET_FILE").map(PathBuf::from),
                })
            }
        };
        let costco_days = env("MONEY_COSTCO_DAYS")
            .map(|days| days.parse::<u32>())
            .transpose()
            .map_err(|e| anyhow!("MONEY_COSTCO_DAYS is not a number: {e}"))?
            .unwrap_or(90);
        Ok(Stores {
            amazon,
            costco: env("MONEY_COSTCO_REFRESH_TOKEN_FILE").map(|file| Costco {
                refresh_token_file: PathBuf::from(file),
                days: costco_days,
                ..Costco::default()
            }),
            dir: env("MONEY_RECEIPTS_DIR").map(PathBuf::from),
        })
    }
}

/// What `amazon-orders` is run with unless told.
pub const DEFAULT_ARGS: [&str; 5] = ["orders", "--last-30-days", "--full-details", "-o", "json"];

/// What it is run with for the charges unless told: two months, so a
/// charge or a refund a while after its order is still seen.
pub const CHARGES_ARGS: [&str; 5] = ["transactions", "--days", "60", "-o", "json"];

/// The `amazon-orders` program, and the files of its sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmazonOrders {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// What it is run with for the charges.
    pub charges_args: Vec<String>,
    /// The sign-in's files; none, and it is the settings page's
    /// ([`crate::vault`]).
    pub username_file: Option<PathBuf>,
    pub password_file: Option<PathBuf>,
    pub otp_secret_file: Option<PathBuf>,
}

/// Costco's own receipts, from its web API: a refresh token signs in, then
/// GraphQL asks for the warehouse and gas receipts since [`Costco::days`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Costco {
    pub refresh_token_file: PathBuf,
    /// How far back receipts are asked for.
    pub days: u32,
    /// The token endpoint; defaults to Costco's.
    pub token_url: String,
    /// The receipts endpoint; defaults to Costco's.
    pub receipts_url: String,
}

impl Default for Costco {
    fn default() -> Self {
        Costco {
            refresh_token_file: PathBuf::new(),
            days: 90,
            token_url: "https://signin.costco.com/e0714dd4-784d-46d6-a278-3e29553483eb/B2C_1A_SSO_WCS_signup_signin_219/oauth2/v2.0/token".into(),
            receipts_url: "https://ecom-api.costco.com/ebusiness/order/v1/orders/graphql".into(),
        }
    }
}

/// The GraphQL query Costco's web app lists receipts with, but asking for
/// each item's lines as its single receipt's page does: the list's own
/// asks for item numbers alone, and the adapter needs every line's amount.
const COSTCO_QUERY: &str = r#"query receiptsWithCounts($startDate: String!, $endDate: String!,$documentType:String!,$documentSubType:String!) {
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
    /// The receipts since `days`, as the adapter expects them.
    pub fn receipts(&self) -> Result<String> {
        let refresh_token = secret(&self.refresh_token_file)?;
        let token: serde_json::Value = ureq::post(&self.token_url)
            .set("Content-Type", "application/x-www-form-urlencoded")
            .send_string(&format!(
                "grant_type=refresh_token&refresh_token={}&client_id={}&scope=openid%20offline_access",
                urlencode(&refresh_token),
                "a3a5186b-7c89-4b4c-93a8-dd604e930757"
            ))?
            .into_json()
            .map_err(|e| anyhow!("Costco sign-in failed: {e}"))?;
        let id_token = token["id_token"].as_str().ok_or_else(|| anyhow!("Costco sign-in gave no token"))?;
        let new_refresh_token = token["refresh_token"]
            .as_str()
            .ok_or_else(|| anyhow!("Costco sign-in gave no refresh token"))?;
        let tmp = self.refresh_token_file.with_extension("tmp");
        std::fs::write(&tmp, new_refresh_token).with_context(|| format!("writing {}", tmp.display()))?;
        std::fs::rename(&tmp, &self.refresh_token_file).with_context(|| format!("replacing {}", self.refresh_token_file.display()))?;

        let start = chrono::Utc::now() - chrono::Duration::days(self.days as i64);
        let request = serde_json::json!({
            "query": COSTCO_QUERY,
            "variables": {
                "startDate": start.format("%-m/%-d/%Y").to_string(),
                "endDate": chrono::Utc::now().format("%-m/%-d/%Y").to_string(),
                "documentType": "all",
                "documentSubType": "all"
            }
        });
        let response = ureq::post(&self.receipts_url)
            .set("Content-Type", "application/json-patch+json")
            .set("costco.env", "PROD")
            .set("costco-x-authorization", &format!("Bearer {id_token}"))
            .set("costco-x-wcs-clientId", "4900eb1f-0c10-4bd9-99c3-c59e6c1ecebf")
            .set("client-identifier", "481b1aec-aa3b-454b-b81b-48187e28f205")
            .set("costco.service", "restOrders")
            .send_string(&request.to_string())?;
        let body: serde_json::Value = response.into_json().map_err(|e| anyhow!("reading Costco's receipts: {e}"))?;
        if !body["data"]["receiptsWithCounts"].is_object() {
            bail!("Costco did not return receipts");
        }
        serde_json::to_string(&body["data"]).map_err(|e| anyhow!("serializing Costco's receipts: {e}"))
    }
}

/// A body field in a form, escaped the way a browser does.
fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            b' ' => "%20".into(),
            b => format!("%{b:02X}"),
        })
        .collect()
}

/// A secret's file, trimmed; its contents never in an error.
fn secret(path: &Path) -> Result<String> {
    let text = std::fs::read_to_string(path).map_err(|e| anyhow!("cannot read {}: {e}", path.display()))?;
    match text.trim() {
        "" => bail!("{} is empty", path.display()),
        s => Ok(s.to_string()),
    }
}

impl AmazonOrders {
    /// The login it signs in with: the server's files, else the stores
    /// page's.
    pub fn login(&self, data: &Path) -> Result<crate::vault::AmazonLogin> {
        if let (Some(u), Some(p)) = (&self.username_file, &self.password_file) {
            return Ok(crate::vault::AmazonLogin {
                username: secret(u)?,
                password: secret(p)?,
                otp_secret: self.otp_secret_file.as_deref().map(secret).transpose()?,
            });
        }
        crate::vault::Vault::load(data)?
            .amazon
            .ok_or_else(|| anyhow!("no Amazon login: give one on the settings page (/settings)"))
    }

    /// What the program prints of the orders, run once with its home in
    /// `data`.
    pub fn run(&self, data: &Path) -> Result<String> {
        self.run_with(data, &self.args)
    }

    /// What it prints of the charges.
    pub fn charges(&self, data: &Path) -> Result<String> {
        self.run_with(data, &self.charges_args)
    }

    fn run_with(&self, data: &Path, args: &[String]) -> Result<String> {
        let login = self.login(data)?;
        let home = data.join("amazon-orders");
        std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
        let mut cmd = Command::new(&self.program);
        cmd.args(args)
            .env("HOME", &home)
            .env("AMAZON_USERNAME", &login.username)
            .env("AMAZON_PASSWORD", &login.password)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(otp) = &login.otp_secret {
            cmd.env("AMAZON_OTP_SECRET_KEY", otp);
        }
        let mut child = cmd.spawn().with_context(|| format!("running {}", self.program.display()))?;
        // Read as it is written, so a long history does not fill the pipe
        // and stop the program.
        let (mut out, mut err) = (child.stdout.take().unwrap(), child.stderr.take().unwrap());
        let reader = std::thread::spawn(move || {
            let mut s = String::new();
            out.read_to_string(&mut s).map(|_| s)
        });
        let errs = std::thread::spawn(move || {
            let mut s = String::new();
            let _ = err.read_to_string(&mut s);
            s
        });
        let deadline = Instant::now() + AMAZON_TIMEOUT;
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if Instant::now() > deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("amazon-orders took more than {} minutes", AMAZON_TIMEOUT.as_secs() / 60);
            }
            std::thread::sleep(Duration::from_millis(200));
        };
        let out = reader.join().map_err(|_| anyhow!("reading amazon-orders"))??;
        let err = errs.join().unwrap_or_default();
        if !status.success() {
            let last = err.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim();
            let last: String = last.chars().take(300).collect();
            bail!("amazon-orders failed ({status}): {last}");
        }
        Ok(out)
    }
}

/// One export waiting in the folder: its store, and the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Waiting {
    pub store: String,
    pub file: PathBuf,
}

/// Every export waiting in `dir`, store by store, by name.
pub fn waiting(dir: &Path) -> Vec<Waiting> {
    let mut out = vec![];
    for store in money_domain::stores::STORES {
        let Ok(rd) = std::fs::read_dir(dir.join(store)) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
        files.sort();
        out.extend(files.into_iter().map(|file| Waiting {
            store: store.to_string(),
            file,
        }));
    }
    out
}

/// An export put away: under `imported/` or `failed/` beside it.
pub fn put_away(w: &Waiting, under: &str) -> Result<()> {
    let parent = w.file.parent().ok_or_else(|| anyhow!("{} has no folder", w.file.display()))?;
    let to = parent.join(under);
    std::fs::create_dir_all(&to)?;
    let name = w.file.file_name().ok_or_else(|| anyhow!("{} has no name", w.file.display()))?;
    std::fs::rename(&w.file, to.join(name)).with_context(|| format!("moving {}", w.file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: std::collections::BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn the_stores_are_told_by_the_environment() {
        assert!(!Stores::from_env(&vars(&[])).unwrap().any());
        let page = Stores::from_env(&vars(&[("MONEY_AMAZON_ORDERS", "/bin/amazon-orders")])).unwrap();
        assert_eq!(page.amazon.unwrap().username_file, None, "the settings page's login");
        let e = Stores::from_env(&vars(&[
            ("MONEY_AMAZON_ORDERS", "/bin/amazon-orders"),
            ("MONEY_AMAZON_USERNAME_FILE", "/c/u"),
        ]))
        .unwrap_err();
        assert!(e.to_string().contains("go together"), "{e}");
        let s = Stores::from_env(&vars(&[
            ("MONEY_AMAZON_ORDERS", "/bin/amazon-orders"),
            ("MONEY_AMAZON_USERNAME_FILE", "/c/u"),
            ("MONEY_AMAZON_PASSWORD_FILE", "/c/p"),
            ("MONEY_AMAZON_ARGS", "orders --year 2026 --full-details -o json"),
            ("MONEY_RECEIPTS_DIR", "/var/lib/money/receipts"),
        ]))
        .unwrap();
        let a = s.amazon.unwrap();
        assert_eq!(a.args, ["orders", "--year", "2026", "--full-details", "-o", "json"]);
        assert_eq!(a.otp_secret_file, None);
        assert_eq!(s.dir, Some(PathBuf::from("/var/lib/money/receipts")));
        let s = Stores::from_env(&vars(&[
            ("MONEY_COSTCO_REFRESH_TOKEN_FILE", "/var/lib/money/costco-refresh-token"),
            ("MONEY_COSTCO_DAYS", "30"),
        ]))
        .unwrap();
        let c = s.costco.unwrap();
        assert_eq!(c.refresh_token_file, PathBuf::from("/var/lib/money/costco-refresh-token"));
        assert_eq!(c.days, 30);
        assert_eq!(c.token_url, Costco::default().token_url);
        assert_eq!(c.receipts_url, Costco::default().receipts_url);
    }

    #[test]
    fn costco_signs_in_rotates_its_refresh_token_and_reads_receipts() {
        let dir = tempfile::tempdir().unwrap();
        let token_file = dir.path().join("refresh-token");
        std::fs::write(&token_file, "old refresh token").unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let costco = Costco {
            refresh_token_file: token_file.clone(),
            days: 0,
            token_url: format!("http://127.0.0.1:{port}/token"),
            receipts_url: format!("http://127.0.0.1:{port}/graphql"),
        };
        let (sent, got) = std::sync::mpsc::channel();
        let serve = std::thread::spawn(move || {
            for n in 0..2 {
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
                let body = if n == 0 {
                    r#"{"id_token":"the id token","refresh_token":"the new refresh token"}"#
                } else {
                    r#"{"data":{"receiptsWithCounts":{"inWarehouse":1,"gasStation":0,"carWash":0,"gasAndCarWash":0,"receipts":[
                        {"warehouseName":"SEATTLE","receiptType":"In-Warehouse","documentType":"WarehouseReceiptDetail",
                         "transactionDateTime":"2026-10-25T14:33:00","transactionBarcode":"21134300501862310251433","total":10.83,
                         "itemArray":[{"itemNumber":"1234","itemDescription01":"KS ORGANIC EGGS","itemDescription02":"24 CT","unit":1,"amount":10.83}]}
                    ]}}}"#
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).unwrap();
                sent.send(request).unwrap();
            }
        });
        let out = costco.receipts().unwrap();
        let token_request = got.recv().unwrap();
        let receipts_request = got.recv().unwrap();
        serve.join().unwrap();

        assert!(token_request.contains("grant_type=refresh_token"), "{token_request}");
        assert!(token_request.contains("refresh_token=old%20refresh%20token"), "{token_request}");
        assert!(
            token_request.contains("client_id=a3a5186b-7c89-4b4c-93a8-dd604e930757"),
            "{token_request}"
        );
        assert!(
            receipts_request.contains("costco-x-authorization: Bearer the id token"),
            "{receipts_request}"
        );
        assert!(receipts_request.contains("costco.service: restOrders"), "{receipts_request}");
        assert!(receipts_request.contains("amount"), "the items' lines are asked for: {receipts_request}");
        // What it gives back is what the domain's adapter reads.
        let import = money_domain::stores::costco(&out).unwrap();
        assert_eq!(import.receipts.len(), 1);
        let r = &import.receipts[0];
        assert_eq!((r.date.as_str(), r.place.as_str(), r.total), ("2026-10-25", "Costco SEATTLE", -1083));
        assert_eq!(r.items[0].name, "KS ORGANIC EGGS 24 CT");
        assert_eq!(r.items[0].amount, -1083);
        assert_eq!(std::fs::read_to_string(&token_file).unwrap(), "the new refresh token");
    }

    /// The program's output is what it printed; its sign-in reaches it in
    /// its environment and nowhere else; a failure says its last line.
    #[test]
    fn amazon_orders_is_run_signed_in() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, text: &str| {
            let p = dir.path().join(name);
            std::fs::write(&p, text).unwrap();
            p
        };
        let program = write("fake-amazon-orders", "#!/bin/sh\n[ \"$AMAZON_PASSWORD\" = hunter2 ] || { echo 'bad password' >&2; exit 2; }\necho \"[{\\\"order_number\\\":\\\"$1\\\",\\\"home\\\":\\\"$HOME\\\"}]\"\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let a = AmazonOrders {
            program,
            args: vec!["orders".into()],
            charges_args: vec!["transactions".into()],
            username_file: Some(write("u", "me@example.com\n")),
            password_file: Some(write("p", "hunter2\n")),
            otp_secret_file: None,
        };
        let out = a.run(dir.path()).unwrap();
        assert!(out.contains("\"order_number\":\"orders\""), "{out}");
        assert!(out.contains("/amazon-orders\"") && !out.contains("hunter2"), "{out}");
        let wrong = AmazonOrders {
            password_file: Some(write("p2", "nope")),
            ..a.clone()
        };
        let e = wrong.run(dir.path()).unwrap_err().to_string();
        assert!(e.contains("bad password") && !e.contains("nope"), "{e}");
        // No files: the settings page's login, or none yet.
        let page = AmazonOrders {
            username_file: None,
            password_file: None,
            ..a
        };
        assert!(page.run(dir.path()).unwrap_err().to_string().contains("settings page"));
        let v = crate::vault::Vault {
            amazon: Some(crate::vault::AmazonLogin {
                username: "me@example.com".into(),
                password: "hunter2".into(),
                otp_secret: None,
            }),
        };
        v.save(dir.path()).unwrap();
        assert!(page.run(dir.path()).unwrap().contains("order_number"));
    }

    #[test]
    fn exports_wait_by_store_and_are_put_away() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("costco")).unwrap();
        std::fs::create_dir_all(dir.path().join("walmart")).unwrap();
        std::fs::write(dir.path().join("costco/b.json"), "{}").unwrap();
        std::fs::write(dir.path().join("costco/a.json"), "{}").unwrap();
        std::fs::write(dir.path().join("walmart/x.json"), "{}").unwrap();
        let w = waiting(dir.path());
        assert_eq!(
            w.iter().map(|w| w.file.file_name().unwrap().to_str().unwrap()).collect::<Vec<_>>(),
            ["a.json", "b.json"]
        );
        put_away(&w[0], "imported").unwrap();
        assert!(dir.path().join("costco/imported/a.json").exists());
        assert_eq!(waiting(dir.path()).len(), 1);
    }
}
