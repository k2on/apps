//! A static site (arkui-web's `ssg` mode, `prerender`): a fixture app's
//! declared paths written at every band, and every link on them resolving.

use ark_app::host::{Cx, HostedApp, Link, Nothing, Place, Running, Session};
use ark_app::view::{form_button, View};
use ark_app::window::Windowed;
use arkui_web::host::Here;
use arkui_web::page;
use arkui_web::{document, prerender, Address, WebHost, Written};

/// A site of three paths: a link to each declared one, and the controls
/// a static page cannot run. Its one change is counted, as a data change.
#[derive(Default)]
struct Site {
    at: Vec<String>,
    inspector: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Go {
    To(&'static str),
    Inspector,
    Change,
}

impl HostedApp for Site {
    type Key = Nothing;
    type Change = Delete;
    type Route = ();
    type Request = ();

    fn encode(_change: &Delete) -> (&'static str, Vec<u8>) {
        ("site.delete", vec![])
    }
    fn open(_session: &Session, _cx: &mut Cx<'_, Site>) -> Site {
        Site::default()
    }
    fn screens(&mut self) -> Vec<&mut dyn Running<Site>> {
        vec![]
    }
    fn screens_ref(&self) -> Vec<&dyn Running<Site>> {
        vec![]
    }
    fn route(&mut self, _route: (), _cx: &mut Cx<'_, Site>) {}
    fn request(&mut self, _request: (), _cx: &mut Cx<'_, Site>) {}
    fn link(&mut self, link: Link, _cx: &mut Cx<'_, Site>) {
        if let Link::Address(url) = link {
            let r = Address::of_url(&url, "/");
            self.at = r.path.clone();
            self.inspector = r.get("inspector").is_some();
        }
    }
    fn launched(&mut self, _args: Vec<String>, _cx: &mut Cx<'_, Site>) {}
}

type Seen<'a> = &'a dyn Place<Nothing, Delete>;

impl Windowed for Site {
    type Msg = Go;
    fn take(&mut self, m: Go, cx: &mut Cx<'_, Site>) {
        match m {
            Go::To(p) => self.at = (!p.is_empty()).then(|| p.to_string()).into_iter().collect(),
            Go::Inspector => self.inspector = true,
            // A change of the data's: counted by the page, which presses
            // every control without sending one.
            Go::Change => {
                let _ = cx.place.send(Delete);
            }
        }
    }
    fn window(&self, _place: Seen<'_>, _now_ms: i64) -> View<Go> {
        View::List(vec![
            form_button("Home", true, Go::To("")),
            form_button("Notes", true, Go::To("notes")),
            form_button("Elsewhere", true, Go::To("elsewhere")),
            form_button("Inspector", true, Go::Inspector),
            form_button("Delete", true, Go::Change),
        ])
    }
    fn hosted(&self, _name: &str, _place: Seen<'_>, _now_ms: i64) -> View<Go> {
        View::Empty
    }
    fn address(&self, _place: Seen<'_>, _now_ms: i64) -> String {
        let r = Address::at(self.at.clone());
        match self.inspector {
            true => r.with("inspector", "1").url(),
            false => r.url(),
        }
    }
    fn paths(&self, _place: Seen<'_>) -> Vec<String> {
        vec!["/".into(), "/notes/".into()]
    }
}

/// The site's one change of its data.
#[derive(Clone, Debug)]
struct Delete;

/// The fixture as its generated entry point would say it.
struct Web;

impl arkui_web::WebHosted for Web {
    type App = Site;
    const ID: &'static str = "site";
    const NAME: &'static str = "Site";
    const SCHEME: &'static str = "site";
    const VERSION: &'static str = "0.1";
    const ACCENT: &'static str = "blue";
    const MARK: &'static str = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>";
    const STATIC: bool = true;
}

#[test]
fn a_static_site_is_its_declared_paths_and_every_link_resolves() {
    let write = |r: &Address, _app: &WebHost<Web>, view: &str| {
        document(
            &Written {
                mode: page::Mode::Static,
                route: r.clone(),
                base: "/".into(),
                ..Written::default()
            },
            view,
        )
    };
    let open = || WebHost::<Web>::over(None, Here::default(), None);
    let pages = prerender(&open, 10, None, &[], &write).unwrap();
    let urls: Vec<String> = pages.iter().map(|(r, _)| r.url()).collect();
    assert_eq!(urls, ["/", "/notes/"]);
    let written: Vec<(String, String)> = pages.iter().map(|(r, h)| (r.url(), h.clone())).collect();
    assert_eq!(page::dangling(&written, "/"), Vec::<String>::new());
    let home = &pages[0].1;
    assert!(home.contains(r#"formaction="/notes/""#), "a declared path is a link");
    for off in ["Elsewhere", "Inspector", "Delete"] {
        let at = home.find(&format!(">{off}<")).unwrap();
        let tag = &home[home[..at].rfind("<button").unwrap()..at];
        assert!(tag.contains("disabled") && !tag.contains("formaction"), "{off}: {tag}");
    }
    assert_eq!(home.matches("class=\"ark-at ark-at-").count(), page::Band::ALL.len(), "a band each");
    let e = prerender(&open, 1, None, &[], &write).unwrap_err();
    assert!(e.contains("past the bound"), "{e}");
}
