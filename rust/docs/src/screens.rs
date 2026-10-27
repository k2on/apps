//! The phones' captures, as the `screens` workflow last published them, and
//! the phone they are drawn in.
//!
//! The workflow (`.github/workflows/screens.yaml`) runs on its own time after
//! a push: it boots a real Android emulator and a real iOS simulator, opens
//! each app's showcase at every shot in [`crate::components`], and commits
//! what it took to the `screens` branch with a `manifest.json` saying what is
//! there and on which device. The page reads that branch through
//! raw.githubusercontent.com, which any origin may fetch, so a capture shows
//! up on the next load with nothing redeployed — and a page whose capture has
//! not been taken yet says so in the phone's place.
//!
//! **The frame is drawn here, not in the picture**: a bezel round the screen
//! at the device's own aspect and corner, with its camera (a punch hole on
//! Android, the island on iOS), so it stays sharp at any size and a capture
//! is the device's pixels and nothing else.
use std::collections::HashMap;

use arkui::images::{Images, Loaded};
use iced::widget::{column, container, image, stack, text, Space};
use iced::{Background, Border, Color, Element, Length, Task};
use serde::Deserialize;

use crate::components::Kind;

/// Where the `screens` branch is read from: the branch on GitHub, or
/// wherever `DOCS_SCREENS` said when this was built (a local directory
/// served for trying the page, say).
pub fn base() -> &'static str {
    option_env!("DOCS_SCREENS").unwrap_or("https://raw.githubusercontent.com/k2on/apps/screens/")
}

/// How wide a phone is drawn, in logical pixels.
pub const PHONE: f32 = 270.0;
/// The bezel, either side of the screen.
const BEZEL: f32 = 10.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phone {
    Android,
    Ios,
}

impl Phone {
    pub fn key(self) -> &'static str {
        match self {
            Phone::Android => "android",
            Phone::Ios => "ios",
        }
    }
}

/// What `manifest.json` says.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Changes with every publish, so a picture's URL does too and no cache
    /// (the Cache API keeps pictures by URL) shows an old one.
    pub version: String,
    /// The commit of main the captures were taken from.
    pub commit: String,
    /// When, as the workflow wrote it.
    pub made: String,
    pub platforms: HashMap<String, Platform>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Platform {
    pub device: Device,
    pub shots: Vec<Captured>,
}

/// The device the captures were taken on, in its own pixels.
#[derive(Debug, Clone, Deserialize)]
pub struct Device {
    pub name: String,
    pub width: f32,
    pub height: f32,
    /// The screen's corner radius.
    pub corner: f32,
    /// `hole` (a punch-hole camera) or `island`.
    pub cutout: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Captured {
    pub id: String,
    /// `screenshot` or `recording`.
    pub kind: String,
    /// The themes it was taken in: `light`, `dark`.
    pub themes: Vec<String>,
    /// A recording's frame count and rate; 1 and 0 for a screenshot.
    #[serde(default = "one")]
    pub frames: u32,
    #[serde(default)]
    pub fps: u32,
}

fn one() -> u32 {
    1
}

/// Where the manifest has got to.
#[derive(Debug, Clone, Default)]
pub enum Fetched {
    #[default]
    Asking,
    Got(Manifest),
    /// No branch yet, or no network: every phone says nothing is captured.
    Missing(String),
}

impl Fetched {
    pub fn manifest(&self) -> Option<&Manifest> {
        match self {
            Fetched::Got(m) => Some(m),
            _ => None,
        }
    }

    /// What one phone has for one shot, with the device it was taken on.
    pub fn find(&self, phone: Phone, id: &str) -> Option<(&Platform, &Captured)> {
        let p = self.manifest()?.platforms.get(phone.key())?;
        Some((p, p.shots.iter().find(|s| s.id == id)?))
    }
}

/// A picture's URL: a screenshot, or a recording's `frame`th (from 0).
pub fn url(m: &Manifest, phone: Phone, shot: &Captured, dark: bool, frame: u32) -> String {
    let theme = match dark && shot.themes.iter().any(|t| t == "dark") {
        true => "dark",
        false => "light",
    };
    let file = match shot.kind.as_str() {
        "recording" => format!("{}/{:03}.jpg", shot.id, frame + 1),
        _ => format!("{}.png", shot.id),
    };
    format!("{}{}/{theme}/{file}?v={}", base(), phone.key(), m.version)
}

/// Every picture this capture needs, in the order it is shown.
pub fn urls(m: &Manifest, phone: Phone, shot: &Captured, dark: bool) -> Vec<String> {
    (0..shot.frames.max(1)).map(|f| url(m, phone, shot, dark, f)).collect()
}

/// The pictures: screenshots kept sharp, a recording's frames smaller, as
/// there are dozens of them.
pub struct Pictures {
    pub stills: Images,
    pub frames: Images,
}

impl Pictures {
    pub fn new() -> Self {
        Pictures {
            stills: Images::bounded("docs/screens", 1200),
            frames: Images::bounded("docs/frames", 640),
        }
    }

    pub fn of(&self, kind: &str) -> &Images {
        match kind {
            "recording" => &self.frames,
            _ => &self.stills,
        }
    }

    /// Ask for every picture `shot` needs; what is already known is not asked
    /// again.
    pub fn want(&mut self, kind: &str, urls: Vec<String>) -> Task<Loaded> {
        let cache = match kind {
            "recording" => &mut self.frames,
            _ => &mut self.stills,
        };
        Task::batch(urls.into_iter().filter_map(|u| cache.want(u)))
    }

    pub fn loaded(&mut self, done: Loaded) {
        match done.url.contains(".jpg") {
            true => self.frames.loaded(done),
            false => self.stills.loaded(done),
        }
    }
}

/// The device a phone is drawn as before anything is captured: a Pixel's and
/// an iPhone's proportions, so the empty frame is the right shape.
pub fn default_device(phone: Phone) -> Device {
    match phone {
        Phone::Android => Device {
            name: "Pixel 7".into(),
            width: 1080.0,
            height: 2400.0,
            corner: 120.0,
            cutout: "hole".into(),
        },
        Phone::Ios => Device {
            name: "iPhone 16 Pro".into(),
            width: 1206.0,
            height: 2622.0,
            corner: 165.0,
            cutout: "island".into(),
        },
    }
}

/// A phone `PHONE` wide with `screen` in it, at `device`'s aspect.
pub fn frame<'a, M: 'a>(device: &Device, screen: Element<'a, M>) -> Element<'a, M> {
    let scale = PHONE / device.width;
    let (w, h) = (PHONE, device.height * scale);
    let corner = device.corner * scale;
    let camera: Element<'a, M> = match device.cutout.as_str() {
        // The island: 125 × 37 points on a 402-point screen, 11 down.
        "island" => container(Space::new())
            .width(w * 0.31)
            .height(w * 0.092)
            .style(move |_| black(w * 0.046))
            .into(),
        _ => container(Space::new())
            .width(w * 0.035)
            .height(w * 0.035)
            .style(move |_| black(w * 0.0175))
            .into(),
    };
    let top = match device.cutout.as_str() {
        "island" => w * 0.027,
        _ => w * 0.028,
    };
    let over = column![Space::new().height(top), container(camera).center_x(Length::Fill)];
    let screen = container(stack![container(screen).width(w).height(h), over])
        .width(w)
        .height(h)
        .clip(true)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color::BLACK)),
            border: Border {
                radius: corner.into(),
                ..Border::default()
            },
            ..container::Style::default()
        });
    container(screen)
        .padding(BEZEL)
        .style(move |_| container::Style {
            background: Some(Background::Color(Color::from_rgb8(0x16, 0x16, 0x18))),
            border: Border {
                color: Color::from_rgb8(0x3A, 0x3A, 0x3E),
                width: 1.5,
                radius: (corner + BEZEL).into(),
            },
            ..container::Style::default()
        })
        .into()
}

fn black(radius: f32) -> container::Style {
    container::Style {
        background: Some(Background::Color(Color::BLACK)),
        border: Border {
            radius: radius.into(),
            ..Border::default()
        },
        ..container::Style::default()
    }
}

/// The screen inside the phone: the picture, rounded to the screen's corner,
/// or a line saying why there is none.
pub fn screen<'a, M: 'a>(device: &Device, picture: Option<&image::Handle>, missing: String) -> Element<'a, M> {
    let scale = PHONE / device.width;
    let (w, h) = (PHONE, device.height * scale);
    match picture {
        Some(p) => image(p.clone()).width(w).height(h).border_radius(device.corner * scale).into(),
        None => container(text(missing).size(12).color(Color::from_rgb8(0x9A, 0x9A, 0xA0)).center())
            .padding(28)
            .center_x(w)
            .center_y(h)
            .into(),
    }
}

pub fn kind_name(kind: Kind) -> &'static str {
    match kind {
        Kind::Screenshot => "Screenshot",
        Kind::Recording => "Recording",
    }
}

/// Fetch the manifest: `fetch` in a browser, a thread and ureq natively.
pub fn fetch() -> Task<Fetched> {
    let url = format!("{}manifest.json", base());
    #[cfg(target_arch = "wasm32")]
    {
        Task::perform(
            async move {
                use wasm_bindgen::JsCast;
                let window = web_sys::window().ok_or("no window")?;
                // Asked fresh: the branch moves under the same URL.
                let init = web_sys::RequestInit::new();
                init.set_cache(web_sys::RequestCache::NoStore);
                let promise = window.fetch_with_str_and_init(&url, &init);
                let response: web_sys::Response = wasm_bindgen_futures::JsFuture::from(promise)
                    .await
                    .map_err(|_| "no answer")?
                    .dyn_into()
                    .map_err(|_| "not a response")?;
                if !response.ok() {
                    return Err(format!("{} {}", response.status(), url));
                }
                let body = wasm_bindgen_futures::JsFuture::from(response.text().map_err(|_| "no body")?)
                    .await
                    .map_err(|_| "no body")?;
                Ok::<String, String>(body.as_string().unwrap_or_default())
            },
            read,
        )
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let (tx, rx) = iced::futures::channel::oneshot::channel();
        std::thread::spawn(move || {
            let got = ureq::get(&url)
                .call()
                .map_err(|e| e.to_string())
                .and_then(|r| r.into_string().map_err(|e| e.to_string()));
            let _ = tx.send(got);
        });
        Task::perform(async move { rx.await.unwrap_or_else(|_| Err("dropped".into())) }, read)
    }
}

fn read(body: Result<String, String>) -> Fetched {
    match body.and_then(|b| serde_json::from_str::<Manifest>(&b).map_err(|e| e.to_string())) {
        Ok(m) => Fetched::Got(m),
        Err(e) => Fetched::Missing(e),
    }
}
