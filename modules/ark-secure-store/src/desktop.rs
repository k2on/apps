//! The Linux desktop's secure store: the Secret Service
//! (`org.freedesktop.secrets`: GNOME Keyring, KWallet, KeePassXC) on the
//! session bus, over zbus as MPRIS is. Each secret is an item of the
//! default collection with the attributes `application` (the app's name)
//! and `key`, its label `<app>: <key>`.
//!
//! No session bus, or no Secret Service on it, answers
//! [`ModuleError::Unavailable`]; a collection that stays locked, or a call
//! the service turns down, `Failed`. The secret goes over the session's
//! `plain` algorithm: the bus is the user's own, as every libsecret client
//! that does not ask for `dh-ietf1024-sha256-aes128-cbc-pkcs7` does.

use std::collections::HashMap;
use std::sync::Mutex;

use ark_module::{Answer, ModuleError};
use zbus::blocking::{Connection, Proxy};
use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

const DEST: &str = "org.freedesktop.secrets";
const SERVICE: &str = "/org/freedesktop/secrets";
const SERVICE_IFACE: &str = "org.freedesktop.Secret.Service";
const COLLECTION_IFACE: &str = "org.freedesktop.Secret.Collection";
const ITEM_IFACE: &str = "org.freedesktop.Secret.Item";
const PROMPT_IFACE: &str = "org.freedesktop.Secret.Prompt";
/// What a call answers in place of a prompt when it needs none.
const NO_PROMPT: &str = "/";

/// A secret as the Secret Service carries it: its session, the algorithm's
/// parameters (none for `plain`), the value and its content type.
type Secret = (OwnedObjectPath, Vec<u8>, Vec<u8>, String);

/// The Secret Service, for `app`'s secrets.
pub struct SecretService {
    app: String,
    address: Option<String>,
    /// The bus and the session opened on it, once a call has needed them;
    /// dropped when a call fails, so the next opens them again.
    open: Mutex<Option<(Connection, OwnedObjectPath)>>,
}

impl SecretService {
    /// The session bus's Secret Service, keeping `app`'s secrets (`money`).
    pub fn new(app: &str) -> SecretService {
        SecretService {
            app: app.into(),
            address: None,
            open: Mutex::new(None),
        }
    }

    /// The Secret Service on the bus at `address`, for a test.
    pub fn at(app: &str, address: &str) -> SecretService {
        SecretService {
            address: Some(address.into()),
            ..SecretService::new(app)
        }
    }

    /// The bus and a session on it: Unavailable when there is no bus or no
    /// service on it.
    fn session(&self) -> Answer<(Connection, OwnedObjectPath)> {
        let mut open = self.open.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(o) = &*open {
            return Ok(o.clone());
        }
        let bus = match &self.address {
            Some(a) => zbus::blocking::connection::Builder::address(a.as_str()).and_then(|b| b.build()),
            None => Connection::session(),
        }
        .map_err(|_| ModuleError::Unavailable)?;
        let service = Proxy::new(&bus, DEST, SERVICE, SERVICE_IFACE).map_err(|_| ModuleError::Unavailable)?;
        // A bus with no Secret Service on it, and none it can start, answers
        // ServiceUnknown here.
        let (_, session): (OwnedValue, OwnedObjectPath) = service
            .call("OpenSession", &("plain", Value::from("")))
            .map_err(|_| ModuleError::Unavailable)?;
        *open = Some((bus.clone(), session.clone()));
        Ok((bus, session))
    }

    /// `f` over the session, which is dropped when it fails.
    fn with<T>(&self, f: impl FnOnce(&Connection, &OwnedObjectPath) -> zbus::Result<T>) -> Answer<T> {
        let (bus, session) = self.session()?;
        f(&bus, &session).map_err(|e| {
            *self.open.lock().unwrap_or_else(|e| e.into_inner()) = None;
            ModuleError::Failed {
                why: format!("the Secret Service: {e}"),
            }
        })
    }

    fn attributes<'a>(&'a self, key: &'a str) -> HashMap<&'a str, &'a str> {
        HashMap::from([("application", self.app.as_str()), ("key", key)])
    }
}

/// `paths` unlocked, asking the person when the service has to.
fn unlock(bus: &Connection, paths: Vec<OwnedObjectPath>) -> zbus::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    let service = Proxy::new(bus, DEST, SERVICE, SERVICE_IFACE)?;
    let (_, prompt): (Vec<OwnedObjectPath>, OwnedObjectPath) = service.call("Unlock", &(paths,))?;
    prompted(bus, prompt).map(|_| ())
}

/// A prompt the service handed back, shown and waited on; none is `/`.
/// What it came to: the prompt's result (a collection it made), none when
/// there was no prompt.
fn prompted(bus: &Connection, prompt: OwnedObjectPath) -> zbus::Result<Option<OwnedValue>> {
    if prompt.as_str() == NO_PROMPT {
        return Ok(None);
    }
    let p = Proxy::new(bus, DEST, prompt.as_str(), PROMPT_IFACE)?;
    let mut completed = p.receive_signal("Completed")?;
    p.call_method("Prompt", &("",))?;
    let Some(m) = completed.next() else {
        return Err(zbus::Error::Failure("the prompt went away".into()));
    };
    let (dismissed, result): (bool, OwnedValue) = m.body().deserialize()?;
    match dismissed {
        true => Err(zbus::Error::Failure("the keyring's prompt was dismissed".into())),
        false => Ok(Some(result)),
    }
}

/// The default collection, made first when there is none (a keyring nobody
/// has logged in to), as libsecret makes it: the keyring asks for its
/// password then.
fn default_collection(bus: &Connection) -> zbus::Result<OwnedObjectPath> {
    let service = Proxy::new(bus, DEST, SERVICE, SERVICE_IFACE)?;
    let found: OwnedObjectPath = service.call("ReadAlias", &("default",))?;
    if found.as_str() != NO_PROMPT {
        return Ok(found);
    }
    let properties = HashMap::from([("org.freedesktop.Secret.Collection.Label", Value::from("Login"))]);
    let (made, prompt): (OwnedObjectPath, OwnedObjectPath) = service.call("CreateCollection", &(properties, "default"))?;
    if made.as_str() != NO_PROMPT {
        return Ok(made);
    }
    match prompted(bus, prompt)? {
        Some(v) => OwnedObjectPath::try_from(v).map_err(zbus::Error::Variant),
        None => Err(zbus::Error::Failure("there is no default keyring".into())),
    }
}

impl SecretService {
    /// The items kept under `key`, unlocked.
    fn items(&self, bus: &Connection, key: &str) -> zbus::Result<Vec<OwnedObjectPath>> {
        let service = Proxy::new(bus, DEST, SERVICE, SERVICE_IFACE)?;
        let (mut unlocked, locked): (Vec<OwnedObjectPath>, Vec<OwnedObjectPath>) = service.call("SearchItems", &(self.attributes(key),))?;
        unlock(bus, locked.clone())?;
        unlocked.extend(locked);
        Ok(unlocked)
    }
}

impl crate::SecureStore for SecretService {
    fn get(&self, key: String) -> Answer<Option<String>> {
        self.with(|bus, session| {
            let Some(item) = self.items(bus, &key)?.into_iter().next() else {
                return Ok(None);
            };
            let item = Proxy::new(bus, DEST, item.as_str(), ITEM_IFACE)?;
            let (_, _, value, _): Secret = item.call("GetSecret", &(session,))?;
            Ok(String::from_utf8(value).ok())
        })
    }

    fn put(&self, key: String, value: String) -> Answer<()> {
        self.with(|bus, session| {
            let collection = default_collection(bus)?;
            unlock(bus, vec![collection.clone()])?;
            let c = Proxy::new(bus, DEST, collection.as_str(), COLLECTION_IFACE)?;
            let attributes: HashMap<String, String> = self.attributes(&key).into_iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
            let properties = HashMap::from([
                ("org.freedesktop.Secret.Item.Label", Value::from(format!("{}: {key}", self.app))),
                ("org.freedesktop.Secret.Item.Attributes", Value::from(attributes)),
            ]);
            let secret: Secret = (session.clone(), vec![], value.into_bytes(), "text/plain; charset=utf8".into());
            let (_, prompt): (OwnedObjectPath, OwnedObjectPath) = c.call("CreateItem", &(properties, secret, true))?;
            prompted(bus, prompt).map(|_| ())
        })
    }

    fn remove(&self, key: String) -> Answer<()> {
        self.with(|bus, _| {
            for item in self.items(bus, &key)? {
                let item = Proxy::new(bus, DEST, item.as_str(), ITEM_IFACE)?;
                let prompt: OwnedObjectPath = item.call("Delete", &())?;
                prompted(bus, prompt)?;
            }
            Ok(())
        })
    }
}
