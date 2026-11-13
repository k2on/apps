//! A static site (arkui-web's `ssg` mode, `prerender`): a fixture app's
//! declared paths written at every band, and every link on them resolving.

use arkui_web::page;
use arkui_web::Address;
use arkui_web::{document, prerender, AnyView, WebApp, Written};
use leptos::html;
use leptos::prelude::*;

/// A site of three paths: a link to each declared one, and the
/// controls a static page cannot run.
#[derive(Default)]
struct Site {
    at: Vec<String>,
    inspector: bool,
    changes: u64,
}

#[derive(Clone, Debug)]
enum Go {
    To(&'static str),
    Inspector,
    Change,
}

impl WebApp for Site {
    type Msg = Go;
    fn view(&self) -> AnyView {
        let b = |label: &'static str, m: Go| arkui_web::wired!(html::button(), arkui_web::tapped(label, &m)).child(label).into_any();
        vec![
            b("Home", Go::To("")),
            b("Notes", Go::To("notes")),
            b("Elsewhere", Go::To("elsewhere")),
            b("Inspector", Go::Inspector),
            b("Delete", Go::Change),
        ]
        .into_any()
    }
    fn update(&mut self, m: Go) {
        match m {
            Go::To(p) => self.at = (!p.is_empty()).then(|| p.to_string()).into_iter().collect(),
            Go::Inspector => self.inspector = true,
            Go::Change => self.changes += 1,
        }
    }
    fn route(&self) -> Address {
        let r = Address::at(self.at.clone());
        if self.inspector {
            return r.with("inspector", "1");
        }
        r
    }
    fn arrive(&mut self, r: &Address) -> Vec<Go> {
        self.at = r.path.clone();
        self.inspector = r.get("inspector").is_some();
        vec![]
    }
    fn static_paths(&self) -> Vec<Address> {
        vec![Address::default(), Address::at(["notes"])]
    }
    fn changes(&self) -> u64 {
        self.changes
    }
}

#[test]
fn a_static_site_is_its_declared_paths_and_every_link_resolves() {
    let write = |r: &Address, view: &str| {
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
    let pages = prerender(&Site::default, 10, None, &[], &write).unwrap();
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
    let e = prerender(&Site::default, 1, None, &[], &write).unwrap_err();
    assert!(e.contains("static-bound"), "{e}");
}
