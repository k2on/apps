//! The stores, beside the banks: the importer's other half. Each run it
//! gathers receipts from where the server was told they are, has the
//! domain's adapters read them (`money_domain::stores`), and authors each
//! store's as `receipts.add_receipts` — which changes nothing for a receipt
//! it already has, so gathering the same ones again is harmless.
//!
//! - **Amazon, through `amazon-orders`** (`MONEY_AMAZON_ORDERS`, the
//!   program): run as `amazon-orders orders --last-30-days --full-details
//!   -o json` (`MONEY_AMAZON_ARGS` to say otherwise, `--year 2026` for a
//!   first pull), signed in by `AMAZON_USERNAME`, `AMAZON_PASSWORD` and,
//!   for a one-time code, `AMAZON_OTP_SECRET_KEY`, each read at the run and
//!   given to that program alone: from the files the server was told
//!   (`MONEY_AMAZON_USERNAME_FILE`, …), else from the login given on the
//!   stores page ([`crate::vault`]). Its home is
//!   `<data>/amazon-orders`, so the session it keeps there spares Amazon a
//!   login each run.
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
    /// The folder of exports.
    pub dir: Option<PathBuf>,
}

impl Stores {
    pub fn any(&self) -> bool {
        self.amazon.is_some() || self.dir.is_some()
    }

    /// Read the environment (blank is unset).
    pub fn from_env(env: &impl Fn(&str) -> Option<String>) -> Result<Stores> {
        let amazon = match env("MONEY_AMAZON_ORDERS") {
            None => None,
            Some(program) => {
                let (u, p) = (env("MONEY_AMAZON_USERNAME_FILE"), env("MONEY_AMAZON_PASSWORD_FILE"));
                if u.is_some() != p.is_some() {
                    bail!(
                        "MONEY_AMAZON_USERNAME_FILE and MONEY_AMAZON_PASSWORD_FILE go together; set both, or neither and sign in on the stores page"
                    );
                }
                Some(AmazonOrders {
                    program: program.into(),
                    args: match env("MONEY_AMAZON_ARGS") {
                        Some(a) => a.split_whitespace().map(String::from).collect(),
                        None => DEFAULT_ARGS.iter().map(|s| s.to_string()).collect(),
                    },
                    username_file: u.map(PathBuf::from),
                    password_file: p.map(PathBuf::from),
                    otp_secret_file: env("MONEY_AMAZON_OTP_SECRET_FILE").map(PathBuf::from),
                })
            }
        };
        Ok(Stores {
            amazon,
            dir: env("MONEY_RECEIPTS_DIR").map(PathBuf::from),
        })
    }
}

/// What `amazon-orders` is run with unless told.
pub const DEFAULT_ARGS: [&str; 5] = ["orders", "--last-30-days", "--full-details", "-o", "json"];

/// The `amazon-orders` program, and the files of its sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AmazonOrders {
    pub program: PathBuf,
    pub args: Vec<String>,
    /// The sign-in's files; none, and it is the stores page's
    /// ([`crate::vault`]).
    pub username_file: Option<PathBuf>,
    pub password_file: Option<PathBuf>,
    pub otp_secret_file: Option<PathBuf>,
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
            .ok_or_else(|| anyhow!("no Amazon login: give one on the stores page (/stores)"))
    }

    /// What the program prints, run once with its home in `data`.
    pub fn run(&self, data: &Path) -> Result<String> {
        let login = self.login(data)?;
        let home = data.join("amazon-orders");
        std::fs::create_dir_all(&home).with_context(|| format!("creating {}", home.display()))?;
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
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

    fn vars(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: std::collections::BTreeMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    #[test]
    fn the_stores_are_told_by_the_environment() {
        assert!(!Stores::from_env(&vars(&[])).unwrap().any());
        let page = Stores::from_env(&vars(&[("MONEY_AMAZON_ORDERS", "/bin/amazon-orders")])).unwrap();
        assert_eq!(page.amazon.unwrap().username_file, None, "the stores page's login");
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
        // No files: the stores page's login, or none yet.
        let page = AmazonOrders {
            username_file: None,
            password_file: None,
            ..a
        };
        assert!(page.run(dir.path()).unwrap_err().to_string().contains("stores page"));
        let v = crate::vault::Vault {
            amazon: Some(crate::vault::AmazonLogin {
                username: "me@example.com".into(),
                password: "hunter2".into(),
                otp_secret: None,
            }),
            ..Default::default()
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
