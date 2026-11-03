//! Files a person gives a window: dropped on it ([`dropped`]), or chosen
//! from a file chooser ([`choose`]) — on the desktop and in a browser
//! alike, each [`Picked`] its name, its media type when known, and its
//! bytes.
//!
//! - **Dropped**: on the desktop, the window's own drop (iced's
//!   `FileDropped`), read from where it is; in a browser, the page's
//!   `drop`, each file's bytes read from the browser.
//! - **Chosen**: on Linux, the desktop's own chooser through its portal
//!   (`org.freedesktop.portal.FileChooser` over D-Bus, the bus the theme
//!   is already read from), on a thread of its own; in a browser, a file
//!   input, clicked while the press that asked is still the person's.

use iced::{Subscription, Task};

/// A file given to the window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Picked {
    pub name: String,
    /// What the browser says it is (`image/jpeg`), or blank: then its
    /// name's extension says ([`media_of`]).
    pub media: String,
    pub bytes: Vec<u8>,
}

impl Picked {
    /// Its media type: the browser's, else its name's.
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

/// Every file dropped on the window, read; one that does not read is
/// passed over.
pub fn dropped() -> Subscription<Picked> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        iced::event::listen_with(|e, _, _| match e {
            iced::Event::Window(iced::window::Event::FileDropped(path)) => {
                let bytes = std::fs::read(&path).ok()?;
                let name = path.file_name()?.to_string_lossy().into_owned();
                Some(Picked {
                    name,
                    media: String::new(),
                    bytes,
                })
            }
            _ => None,
        })
    }
    #[cfg(target_arch = "wasm32")]
    {
        Subscription::run(web::drops)
    }
}

/// A file chosen, of these media types (`image/*`), or none when the
/// person closed the chooser.
pub fn choose(title: &str, accept: &[&str]) -> Task<Option<Picked>> {
    let (tx, rx) = iced::futures::channel::oneshot::channel();
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (title, accept) = (title.to_string(), accept.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        std::thread::spawn(move || {
            let _ = tx.send(native::choose(&title, &accept));
        });
    }
    #[cfg(target_arch = "wasm32")]
    {
        let _ = title;
        web::choose(&accept.join(","), tx);
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

#[cfg(not(target_arch = "wasm32"))]
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
#[cfg(not(target_arch = "wasm32"))]
fn tick() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

#[cfg(target_arch = "wasm32")]
mod web {
    use std::cell::RefCell;

    use iced::futures::channel::{mpsc, oneshot};
    use wasm_bindgen::closure::Closure;
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{DragEvent, Event, File, HtmlInputElement};

    use super::Picked;

    thread_local! {
        static DROPS: RefCell<Option<mpsc::UnboundedSender<Picked>>> = const { RefCell::new(None) };
    }

    /// A file's bytes, read from the browser.
    async fn read(f: File) -> Option<Picked> {
        let buf = JsFuture::from(f.array_buffer()).await.ok()?;
        Some(Picked {
            name: f.name(),
            media: f.type_(),
            bytes: js_sys::Uint8Array::new(&buf).to_vec(),
        })
    }

    /// The page's drops, heard once: a drag over the page is let drop
    /// (rather than the browser opening the file), and each file dropped
    /// read and sent.
    pub fn drops() -> mpsc::UnboundedReceiver<Picked> {
        let (tx, rx) = mpsc::unbounded();
        let first = DROPS.with(|d| d.borrow_mut().replace(tx).is_none());
        if first {
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let over = Closure::<dyn FnMut(Event)>::new(|e: Event| e.prevent_default());
                let drop = Closure::<dyn FnMut(DragEvent)>::new(|e: DragEvent| {
                    e.prevent_default();
                    let Some(files) = e.data_transfer().and_then(|d| d.files()) else {
                        return;
                    };
                    for f in (0..files.length()).filter_map(|i| files.get(i)) {
                        wasm_bindgen_futures::spawn_local(async move {
                            if let Some(p) = read(f).await {
                                DROPS.with(|d| d.borrow().as_ref().map(|tx| tx.unbounded_send(p)));
                            }
                        });
                    }
                });
                let _ = doc.add_event_listener_with_callback("dragover", over.as_ref().unchecked_ref());
                let _ = doc.add_event_listener_with_callback("drop", drop.as_ref().unchecked_ref());
                over.forget();
                drop.forget();
            }
        }
        rx
    }

    /// A file input of these types, clicked; what is chosen read and sent,
    /// or none for a chooser closed.
    pub fn choose(accept: &str, tx: oneshot::Sender<Option<Picked>>) {
        let Some(input) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.create_element("input").ok())
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        else {
            let _ = tx.send(None);
            return;
        };
        input.set_type("file");
        input.set_accept(accept);
        let tx = std::rc::Rc::new(RefCell::new(Some(tx)));
        let (sent, cancelled) = (tx.clone(), tx);
        let chosen = input.clone();
        let change = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            let Some(tx) = sent.borrow_mut().take() else { return };
            let file = chosen.files().and_then(|f| f.get(0));
            wasm_bindgen_futures::spawn_local(async move {
                let picked = match file {
                    Some(f) => read(f).await,
                    None => None,
                };
                let _ = tx.send(picked);
            });
        });
        let cancel = Closure::<dyn FnMut(Event)>::new(move |_: Event| {
            if let Some(tx) = cancelled.borrow_mut().take() {
                let _ = tx.send(None);
            }
        });
        let _ = input.add_event_listener_with_callback("change", change.as_ref().unchecked_ref());
        let _ = input.add_event_listener_with_callback("cancel", cancel.as_ref().unchecked_ref());
        change.forget();
        cancel.forget();
        input.click();
    }
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
