//! `devices-enroll`: a phone made devices' device owner and signed in, from
//! the laptop it is plugged into (adb), in one go:
//!
//! 1. devices installed, if an APK is given (`adb install`);
//! 2. made the device owner (`adb shell dpm set-device-owner
//!    us.koon.devices/dev.arkui.modules.ArkDpcAdmin`) — on a phone with no
//!    account added since its last reset — which wakes the app
//!    (`Woken::Enrolled`);
//! 3. the person whose phone it is signed in on the laptop, in its browser
//!    (`ark_auth::client::login`), and a one-time code made for their
//!    session (`/auth/device-code`, what "Add a Device" shows);
//! 4. that code's join link opened on the phone (`adb shell am start …
//!    devices://join?server=…&code=…`): the phone redeems it and is signed in
//!    as them, its first sync holding it to their policy.
//!
//! The phone's own screen asks only whether to join. Then turn USB debugging
//! off; a policy with `no_debugging_features` keeps it off.

use std::process::Command;

use anyhow::{bail, Context, Result};

/// devices' applicationId: the flake's `bundlePrefix` and app.toml's `id`.
pub const APP: &str = "us.koon.devices";
/// The admin the app is made the device owner as (ark-dpc's).
pub const ADMIN: &str = "dev.arkui.modules.ArkDpcAdmin";
/// devices' scheme (app.toml's `scheme`).
pub const SCHEME: &str = "devices";

/// What the laptop was asked to do.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Enroll {
    /// The server, as the phone reaches it (`https://devices.example.com`).
    pub server: String,
    /// devices' APK, installed first when given.
    pub apk: Option<String>,
    /// Whose phone it is: the login the browser is opened for.
    pub user: Option<String>,
    /// Which phone, when more than one is plugged in (`adb -s`).
    pub serial: Option<String>,
}

impl Enroll {
    /// The command line: `SERVER [--apk PATH] [--user NAME] [--serial SERIAL]`.
    pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Enroll> {
        let mut e = Enroll::default();
        let mut args = args.into_iter();
        while let Some(a) = args.next() {
            let mut value = |flag: &str| args.next().with_context(|| format!("{flag} needs a value"));
            match a.as_str() {
                "--apk" => e.apk = Some(value("--apk")?),
                "--user" => e.user = Some(value("--user")?),
                "--serial" => e.serial = Some(value("--serial")?),
                flag if flag.starts_with('-') => bail!("unknown option {flag}"),
                server if e.server.is_empty() => e.server = server.trim_end_matches('/').to_string(),
                extra => bail!("unexpected argument {extra}"),
            }
        }
        if e.server.is_empty() {
            bail!("which server? devices-enroll SERVER [--apk PATH] [--user NAME] [--serial SERIAL]");
        }
        if !e.server.starts_with("https://") && !e.server.starts_with("http://") {
            bail!("the server is a URL: https://devices.example.com, not {}", e.server);
        }
        Ok(e)
    }

    /// adb's arguments for `rest`, for this phone.
    fn adb(&self, rest: &[&str]) -> Vec<String> {
        let mut v = vec![];
        if let Some(s) = &self.serial {
            v.extend(["-s".to_string(), s.clone()]);
        }
        v.extend(rest.iter().map(|s| s.to_string()));
        v
    }

    /// The APK installed, over any devices already there.
    pub fn install(&self) -> Option<Vec<String>> {
        self.apk.as_ref().map(|apk| self.adb(&["install", "-r", apk]))
    }

    /// The app made the device owner.
    pub fn owner(&self) -> Vec<String> {
        self.adb(&["shell", "dpm", "set-device-owner", &format!("{APP}/{ADMIN}")])
    }

    /// `link` opened on the phone, in devices.
    pub fn open(&self, link: &str) -> Vec<String> {
        self.adb(&["shell", "am", "start", "-a", "android.intent.action.VIEW", "-d", link, APP])
    }
}

/// The join link that signs the phone in with `code`.
pub fn link(server: &str, code: &str) -> String {
    ark_auth::onboarding::join_link(SCHEME.into(), server.into(), Some(code.into()))
}

fn run(what: &str, args: &[String]) -> Result<()> {
    eprintln!("devices-enroll: {what}: adb {}", args.join(" "));
    let status = Command::new("adb")
        .args(args)
        .status()
        .context("running adb (is it installed and on the PATH?)")?;
    if !status.success() {
        bail!("{what}: adb said {status}");
    }
    Ok(())
}

/// Open `url` in the laptop's browser, or say it.
fn browse(url: &str) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    if Command::new(opener).arg(url).status().map(|s| s.success()).unwrap_or(false) {
        eprintln!("devices-enroll: signing in in your browser");
    } else {
        eprintln!("devices-enroll: open this to sign in: {url}");
    }
}

/// Every step, in order; the first that fails stops it, said.
pub fn enroll(e: &Enroll) -> Result<()> {
    if let Some(install) = e.install() {
        run("installing devices", &install)?;
    }
    run("making devices the device owner", &e.owner())?;
    let login = ark_auth::client::login(&e.server, e.user.as_deref(), browse).map_err(|why| anyhow::anyhow!("signing in at {}: {why}", e.server))?;
    let (code, _) =
        ark_auth::client::device_code(&e.server, &login.token).map_err(|why| anyhow::anyhow!("a code for the phone from {}: {why}", e.server))?;
    run("signing the phone in", &e.open(&link(&e.server, &code)))?;
    eprintln!("devices-enroll: done. On the phone, join; then turn USB debugging off.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split(' ').map(String::from).collect()
    }

    /// What the laptop runs, for the phone it names. Falsified by the admin's
    /// component without the app's package: dpm refuses "Unknown admin".
    #[test]
    fn the_steps_name_the_app_and_the_phone() {
        let e = Enroll::parse(args("https://devices.example.com/ --apk devices.apk --serial 3A1 --user ann")).unwrap();
        assert_eq!(e.server, "https://devices.example.com");
        assert_eq!(e.install().unwrap(), args("-s 3A1 install -r devices.apk"));
        assert_eq!(
            e.owner(),
            args("-s 3A1 shell dpm set-device-owner us.koon.devices/dev.arkui.modules.ArkDpcAdmin")
        );
        let l = link(&e.server, "AB12");
        assert_eq!(l, "devices://join?server=https%3A%2F%2Fdevices.example.com&code=AB12");
        let open = e.open(&l);
        assert_eq!(&open[..7], &args("-s 3A1 shell am start -a android.intent.action.VIEW")[..]);
        assert_eq!(open.last().map(String::as_str), Some(APP));
        // The link is one that phones read back as the server and the code.
        let j = ark_auth::onboarding::join_of(l).unwrap();
        assert_eq!((j.server.as_str(), j.code.as_deref()), ("https://devices.example.com", Some("AB12")));
    }

    #[test]
    fn a_server_is_a_url() {
        assert!(Enroll::parse(args("devices.example.com")).is_err());
        assert!(Enroll::parse(vec![]).is_err());
        assert!(Enroll::parse(args("https://a --bogus x")).is_err());
        assert!(Enroll::parse(args("https://a")).unwrap().install().is_none());
    }
}
