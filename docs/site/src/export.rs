//! What the site's export works out natively before its pages are written
//! (app.toml's `[web] prepare`; ui/docs/app.md, "Export"): the desktop
//! pictures, drawn by arkui itself into the export's `pictures/`, and
//! catalogue.json beside the pages; and, for every page to be opened with
//! (`Session::prepared`), what the browser cannot work out — each
//! picture's sizes, the tables of what arkui's functions answer, the notes.
use std::fs;
use std::io;
use std::path::Path;

use iced::Theme;

use crate::catalogue;
use crate::components::COMPONENTS;
use crate::examples::{self, Canvas};
use crate::pages::{Answer, Built, BuiltComponent, Pic, VARIANTS};
use crate::render::Painter;

/// The two widths a picture is drawn at, in logical pixels: the page's
/// measure, and a phone's less its gutters.
pub const WIDE: f32 = 640.0;
pub const NARROW: f32 = 320.0;

/// What the native build works out for the browser: each component's
/// pictures (drawn into `out/pictures` when `painter` is given), the tables
/// of what its functions answer, and its notes.
fn built(out: &Path, mut painter: Option<&mut Painter>) -> io::Result<Built> {
    let mut built = Built::default();
    for c in COMPONENTS {
        let mut pictures = vec![];
        for e in examples::pictures(c.id) {
            let mut sizes = vec![];
            for v in VARIANTS {
                let theme = if v.starts_with("light") { Theme::Light } else { Theme::Dark };
                let width = if v.ends_with("wide") { WIDE } else { NARROW };
                let size = match painter.as_deref_mut() {
                    Some(p) => {
                        let picture = (e.draw)(Canvas::new(p, &theme, width));
                        fs::write(out.join(format!("pictures/{}-{v}.webp", e.name)), picture.webp())?;
                        picture.logical()
                    }
                    None => (width as u32, 0),
                };
                sizes.push(size);
            }
            pictures.push(Pic {
                name: e.name.into(),
                caption: e.caption.into(),
                sizes,
            });
        }
        built.components.push(BuiltComponent {
            id: c.id.into(),
            pictures,
            answers: examples::answers(c.id)
                .into_iter()
                .map(|a| Answer {
                    caption: a.caption.into(),
                    rows: a.rows,
                })
                .collect(),
            notes: examples::notes(c.id).iter().map(|n| n.to_string()).collect(),
        });
    }
    Ok(built)
}

/// The pictures drawn into `out/pictures` and catalogue.json written
/// beside the pages; what every page is opened with, as JSON.
pub fn prepare(out: &Path) -> io::Result<String> {
    let mut painter = Painter::new();
    written(out, Some(&mut painter))
}

/// [`prepare`] with no picture drawn, their sizes unknown: the tests'.
pub fn unpainted(out: &Path) -> io::Result<String> {
    written(out, None)
}

fn written(out: &Path, painter: Option<&mut Painter>) -> io::Result<String> {
    fs::create_dir_all(out.join("pictures"))?;
    fs::write(out.join("catalogue.json"), catalogue::json())?;
    let built = built(out, painter)?;
    Ok(serde_json::to_string(&built).expect("plain data"))
}
