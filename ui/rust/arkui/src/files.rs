//! Files a person gives a window: dropped on it ([`dropped`]), or chosen
//! from a file chooser ([`choose`]), each [`Picked`] its name, its media
//! type when known, and its bytes.
//!
//! - **Dropped**: the window's own drop (iced's `FileDropped`), read from
//!   where it is.
//! - **Chosen**: on Linux, the desktop's own chooser through its portal
//!   (`org.freedesktop.portal.FileChooser` over D-Bus, the bus the theme
//!   is already read from), on a thread of its own.

use iced::{Subscription, Task};

/// A file given to the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub name: String,
    /// What the system says it is (`image/jpeg`), or blank: then its
    /// name's extension says ([`media_of`]).
    pub media: String,
    pub bytes: Vec<u8>,
}

impl Picked {
    /// Its media type: the system's, else its name's.
    pub fn media(&self) -> Option<String> {
        match self.media.trim() {
            "" => media_of(&self.name).map(String::from),
            m => Some(m.to_string()),
        }
    }
}

/// The media type a file's name says, for the kinds a window takes.
pub fn media_of(name: &str) -> Option<&'static str> {
    let ext = name.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match ext.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "heic" => "image/heic",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "pdf" => "application/pdf",
        _ => return None,
    })
}

/// A file dropped on the window, read: the first of a drop that reads (a
/// drop is every file let go at once, as the fork's winit says it); one
/// that does not read is passed over.
pub fn dropped() -> Subscription<Picked> {
    iced::event::listen_with(|e, _, _| match e {
        iced::Event::Window(iced::window::Event::FileDropped(paths)) => paths.iter().find_map(|path| {
            let bytes = std::fs::read(path).ok()?;
            let name = path.file_name()?.to_string_lossy().into_owned();
            Some(Picked {
                name,
                media: String::new(),
                bytes,
            })
        }),
        _ => None,
    })
}

/// A file chosen, of these media types (`image/*`), or none when the
/// person closed the chooser.
pub fn choose(title: &str, accept: &[&str]) -> Task<Option<Picked>> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    {
        let (title, accept) = (title.to_string(), accept.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        std::thread::spawn(move || {
            let _ = tx.send(native::choose(&title, &accept));
        });
    }
    Task::perform(async move { rx.await.ok().flatten() }, |p| p)
}

/// `file:///a%20b.jpg` as the path it names.
pub fn path_of_uri(uri: &str) -> Option<String> {
    let rest = uri.strip_prefix("file://")?;
    let (bytes, mut out, mut i) = (rest.as_bytes(), Vec::new(), 0);
    while i < bytes.len() {
        match (bytes[i], bytes.get(i + 1..i + 3)) {
            (b'%', Some(h)) => {
                out.push(u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok()?);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

mod native {
    use super::Picked;

    /// The desktop's chooser, waited for.
    pub fn choose(title: &str, accept: &[String]) -> Option<Picked> {
        #[cfg(target_os = "linux")]
        return portal(title, accept).ok().flatten();
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (title, accept);
            None
        }
    }

    /// `FileChooser.OpenFile`, and the `Response` of the request it makes:
    /// the request's path is known before the call (its `handle_token`),
    /// so the answer is listened for before it can come.
    #[cfg(target_os = "linux")]
    fn portal(title: &str, accept: &[String]) -> zbus::Result<Option<Picked>> {
        use std::collections::HashMap;
        use zbus::blocking::{Connection, Proxy};
        use zbus::zvariant::{OwnedObjectPath, OwnedValue, Value};

        const DESKTOP: &str = "org.freedesktop.portal.Desktop";
        let conn = Connection::session()?;
        let token = format!("arkui{}", std::process::id() as u64 * 1000 + crate::files::tick());
        let me = conn
            .unique_name()
            .map(|n| n.trim_start_matches(':').replace('.', "_"))
            .unwrap_or_default();
        let path = format!("/org/freedesktop/portal/desktop/request/{me}/{token}");
        let request = Proxy::new(&conn, DESKTOP, path.as_str(), "org.freedesktop.portal.Request")?;
        let mut answers = request.receive_signal("Response")?;
        let chooser = Proxy::new(&conn, DESKTOP, "/org/freedesktop/portal/desktop", "org.freedesktop.portal.FileChooser")?;
        // One filter of the media types asked for: `(1, type)` a media
        // type, `(0, glob)` a pattern.
        let types: Vec<(u32, String)> = accept.iter().map(|a| (1, a.clone())).collect();
        let filters = vec![(title.to_string(), types)];
        let mut options: HashMap<&str, Value> = HashMap::new();
        options.insert("handle_token", Value::from(token.as_str()));
        if !accept.is_empty() {
            options.insert("filters", Value::from(filters.clone()));
            options.insert("current_filter", Value::from(filters[0].clone()));
        }
        let _: OwnedObjectPath = chooser.call("OpenFile", &("", title, options))?;
        let Some(answer) = answers.next() else { return Ok(None) };
        let (code, results): (u32, HashMap<String, OwnedValue>) = answer.body().deserialize()?;
        if code != 0 {
            return Ok(None);
        }
        let uris: Vec<String> = match results.get("uris") {
            Some(v) => v.try_clone()?.try_into()?,
            None => vec![],
        };
        let Some(path) = uris.first().and_then(|u| super::path_of_uri(u)) else {
            return Ok(None);
        };
        let path = std::path::PathBuf::from(path);
        let Ok(bytes) = std::fs::read(&path) else { return Ok(None) };
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Ok(Some(Picked {
            name,
            media: String::new(),
            bytes,
        }))
    }
}

/// A number no two choosers of this process share.
fn tick() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_uri_is_its_path() {
        assert_eq!(path_of_uri("file:///home/a/My%20Receipt.JPG").as_deref(), Some("/home/a/My Receipt.JPG"));
        assert_eq!(path_of_uri("https://x/y"), None);
        assert_eq!(media_of("My Receipt.JPG"), Some("image/jpeg"));
        assert_eq!(media_of("notes.txt"), None);
        let p = Picked {
            name: "scan".into(),
            media: "image/png".into(),
            bytes: vec![],
        };
        assert_eq!(p.media().as_deref(), Some("image/png"), "the browser's word first");
    }
}
