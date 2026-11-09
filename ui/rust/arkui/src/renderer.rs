//! Which renderer draws the window, said once, out loud.
//!
//! iced's renderer is wgpu with tiny-skia behind it: if wgpu opens no
//! device iced draws on the CPU instead, **silently**. The window still
//! works, just slower — and an image's `border_radius` is wgpu's alone, so
//! a round face comes out square. Nobody could tell which one they had, so:
//!
//! - **[`choose`]**, first thing in `main`, before iced starts. It reads
//!   `ARK_RENDERER` (`gpu` or `cpu`) and otherwise asks wgpu itself, the
//!   way iced is about to: every adapter on every backend this build has,
//!   hardware before software and Vulkan before GL, each asked for a
//!   device with iced's own limits. The first that opens is the one iced
//!   is pinned to (`WGPU_BACKEND`), so an adapter that refuses iced's device
//!   on Vulkan — Asahi's Honeykrisp on a Mesa without what iced asks for —
//!   falls back to GL on the same GPU rather than past it to the CPU, which
//!   iced alone would do. When none opens, iced is told tiny-skia
//!   (`ICED_BACKEND`) and the line says why, down to the driver library
//!   that would not load ([`native::why_none`]).
//! - **One line on stderr**, whatever `RUST_LOG`
//!   says — the apps install no logger: `renderer: wgpu (Vulkan, AMD Radeon
//!   …, discrete GPU)` or `renderer: tiny-skia (CPU) — no GPU adapter: …`.
//! - **What was actually drawn with.** The window's root
//!   ([`crate::window::view`]) tells [`drawn_with`] which renderer drew its
//!   frame. If that is not what [`choose`] said, a second line says so.
//! - **[`about`]**, the same sentence for an app's Settings.
//!
//! `ARK_RENDERER=cpu` is iced's `ICED_BACKEND=tiny-skia`; `ARK_RENDERER=gpu`
//! is `ICED_BACKEND=wgpu`, where a window that cannot open a device fails
//! to open rather than fall back.
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Mutex, OnceLock};

use iced::widget::text;
use iced::Element;

/// What [`choose`] found.
#[derive(Debug, Clone, PartialEq)]
pub enum Found {
    /// wgpu opened a device on this adapter.
    Gpu(Adapter),
    /// No adapter would open a device: why.
    None(String),
    /// `ARK_RENDERER=cpu`: not asked.
    Cpu,
    /// Not asked:
    /// or a window [`choose`] was not called for.
    Unasked,
}

/// One adapter, as wgpu describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct Adapter {
    /// `Vulkan`, `Gl`, `Metal`.
    pub backend: String,
    /// `AMD Radeon RX 7800 XT (RADV NAVI32)`, `Apple M2 (G14G)`, `llvmpipe`.
    pub name: String,
    /// `discrete GPU`, `integrated GPU`, `software: drawn on the CPU`.
    pub kind: String,
}

impl Adapter {
    fn says(&self) -> String {
        format!("wgpu ({}, {}, {})", self.backend, self.name, self.kind)
    }
}

static FOUND: OnceLock<Found> = OnceLock::new();
/// What `ARK_RENDERER` asked for, if it asked.
static FORCED: OnceLock<Option<&'static str>> = OnceLock::new();
/// 0 not drawn yet, 1 wgpu, 2 tiny-skia.
static DRAWN: AtomicU8 = AtomicU8::new(0);
/// Whether this process's window was [`choose`]n for, so a test drawing a
/// widget says nothing.
static ASKED: AtomicBool = AtomicBool::new(false);
/// The line last said, so a frame only speaks when it differs.
static SAID: Mutex<String> = Mutex::new(String::new());

/// Decide, and say, which renderer the window will draw with: first thing
/// in `main`, before `iced::application`.
pub fn choose() {
    ASKED.store(true, Ordering::Relaxed);
    {
        let asked = std::env::var("ARK_RENDERER").ok().map(|a| a.trim().to_ascii_lowercase());
        let forced = match asked.as_deref() {
            None | Some("") => None,
            Some("cpu") => Some("cpu"),
            Some("gpu") => Some("gpu"),
            Some(other) => {
                eprintln!("ARK_RENDERER={other}: not gpu or cpu, so chosen as if unset");
                None
            }
        };
        let _ = FORCED.set(forced);
        let found = match forced {
            Some("cpu") => {
                set("ICED_BACKEND", "tiny-skia");
                Found::Cpu
            }
            _ => {
                if forced == Some("gpu") {
                    set("ICED_BACKEND", "wgpu");
                }
                native::find()
            }
        };
        match &found {
            // Pinned to the backend whose device opened, unless the person
            // already pinned one: iced asks only one adapter, and one that
            // turns its device down sends it to the CPU.
            Found::Gpu(a) if std::env::var_os("WGPU_BACKEND").is_none() => set("WGPU_BACKEND", &a.backend.to_ascii_lowercase()),
            // Nothing will open: iced need not try it all again before the
            // CPU, and the line is then exactly what happens.
            Found::None(_) if forced.is_none() && std::env::var_os("ICED_BACKEND").is_none() => set("ICED_BACKEND", "tiny-skia"),
            _ => {}
        }
        let _ = FOUND.set(found);
    }
    say(&line());
}

fn set(key: &str, value: &str) {
    // Before iced starts and before any thread of this program's: `main`'s
    // first lines are the one place this is sound.
    std::env::set_var(key, value);
}

/// What [`choose`] found.
pub fn found() -> &'static Found {
    FOUND.get().unwrap_or(&Found::Unasked)
}

/// The renderer that drew the window's last frame: told by the root each
/// frame, from the renderer iced handed it.
pub fn drawn_with(renderer: &iced::Renderer) {
    let now = match renderer {
        iced::Renderer::Primary(_) => 1,
        iced::Renderer::Secondary(_) => 2,
    };
    if DRAWN.swap(now, Ordering::Relaxed) != now && ASKED.load(Ordering::Relaxed) {
        let line = line();
        if *SAID.lock().unwrap_or_else(|e| e.into_inner()) != line {
            say(&line);
        }
    }
}

/// The sentence: which renderer, on what, or why not the GPU.
pub fn line() -> String {
    let forced = FORCED.get().copied().flatten();
    let drawn = DRAWN.load(Ordering::Relaxed);
    match (found(), drawn) {
        (Found::Cpu, _) => "tiny-skia (CPU) \u{2014} ARK_RENDERER=cpu".into(),
        (Found::Gpu(a), 0 | 1) => a.says(),
        (Found::Gpu(a), _) => format!("tiny-skia (CPU) \u{2014} wgpu opened {} on its own but not for this window", a.says()),
        (Found::None(why), 1) => format!("wgpu (adapter unknown) \u{2014} though none opened when asked: {why}"),
        (Found::None(why), _) if forced == Some("gpu") => {
            format!("wgpu (ARK_RENDERER=gpu) \u{2014} no GPU adapter: {why}; the window cannot open")
        }
        (Found::None(why), _) => format!("tiny-skia (CPU) \u{2014} no GPU adapter: {why}"),
        (Found::Unasked, 0) => "not drawn yet".into(),
        (Found::Unasked, 1) => "wgpu".into(),
        (Found::Unasked, _) => "tiny-skia (CPU)".into(),
    }
}

fn say(line: &str) {
    *SAID.lock().unwrap_or_else(|e| e.into_inner()) = line.to_string();
    eprintln!("renderer: {line}");
}

/// The line, for an app's Settings: quiet, small, selectable by eye.
pub fn about<'a, M: 'a>() -> Element<'a, M> {
    text(format!("Drawn with {}", line()))
        .size(12)
        .style(crate::style::faint(0.55))
        .wrapping(text::Wrapping::Word)
        .into()
}

pub mod native {
    //! Asking wgpu, before iced does.
    use super::{Adapter, Found};
    use iced::wgpu;

    /// Every adapter wgpu has, best first, each asked for iced's device; the
    /// first that opens one. Backends as iced takes them (`WGPU_BACKEND`, or
    /// all of them).
    pub fn find() -> Found {
        let backends = wgpu::Backends::from_env().unwrap_or(wgpu::Backends::all());
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends,
            ..Default::default()
        });
        let mut adapters = instance.enumerate_adapters(backends);
        adapters.sort_by_key(|a| rank(&a.get_info()));
        let mut refused = Vec::new();
        for adapter in adapters {
            let info = adapter.get_info();
            match opens(&adapter) {
                Ok(()) => {
                    let found = Adapter {
                        backend: format!("{:?}", info.backend),
                        name: info.name.clone(),
                        kind: kind(info.device_type).into(),
                    };
                    // A software adapter is what is left when the driver is
                    // missing: say why there is no other, beside it.
                    if info.device_type == wgpu::DeviceType::Cpu {
                        return Found::Gpu(Adapter {
                            kind: format!("{}; no hardware adapter: {}", found.kind, why_none(&refused)),
                            ..found
                        });
                    }
                    return Found::Gpu(found);
                }
                Err(e) => refused.push(format!("{} ({:?}) refused iced's device: {e}", info.name, info.backend)),
            }
        }
        Found::None(why_none(&refused))
    }

    /// Hardware first, then Vulkan before GL, then a discrete GPU before
    /// an integrated one: what iced's high-performance preference would
    /// pick, with software last whatever its backend.
    fn rank(info: &wgpu::AdapterInfo) -> (u8, u8, u8) {
        let software = u8::from(info.device_type == wgpu::DeviceType::Cpu);
        let backend = match info.backend {
            wgpu::Backend::Vulkan | wgpu::Backend::Metal | wgpu::Backend::Dx12 => 0,
            _ => 1,
        };
        let device = match info.device_type {
            wgpu::DeviceType::DiscreteGpu => 0,
            wgpu::DeviceType::IntegratedGpu => 1,
            wgpu::DeviceType::VirtualGpu => 2,
            wgpu::DeviceType::Other => 3,
            wgpu::DeviceType::Cpu => 4,
        };
        (software, backend, device)
    }

    fn kind(t: wgpu::DeviceType) -> &'static str {
        match t {
            wgpu::DeviceType::DiscreteGpu => "discrete GPU",
            wgpu::DeviceType::IntegratedGpu => "integrated GPU",
            wgpu::DeviceType::VirtualGpu => "virtual GPU",
            wgpu::DeviceType::Other => "GPU",
            wgpu::DeviceType::Cpu => "software: drawn on the CPU",
        }
    }

    /// The device iced's compositor asks for (iced_wgpu 0.14's
    /// `window::compositor::new`): wgpu's default limits, else the
    /// downlevel ones, either with two bind groups and 2048 bindings.
    fn opens(adapter: &wgpu::Adapter) -> Result<(), String> {
        let mut last = String::new();
        for limits in [wgpu::Limits::default(), wgpu::Limits::downlevel_defaults()] {
            let asked = wgpu::DeviceDescriptor {
                label: Some("arkui::renderer probe"),
                required_limits: wgpu::Limits {
                    max_bind_groups: 2,
                    max_non_sampler_bindings: 2048,
                    ..limits
                },
                ..Default::default()
            };
            match block_on(adapter.request_device(&asked)) {
                Ok(_) => return Ok(()),
                Err(e) => last = e.to_string(),
            }
        }
        Err(last)
    }

    /// A future run to its end on this thread: wgpu's native requests are
    /// ready on the first poll, and nothing else runs yet.
    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::task::{Context, Poll, Wake};
        struct Unpark(std::thread::Thread);
        impl Wake for Unpark {
            fn wake(self: std::sync::Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = std::sync::Arc::new(Unpark(std::thread::current())).into();
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(out) => return out,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    /// Why no hardware adapter: what refused iced's device, else which
    /// driver library would not load and what the dynamic loader said —
    /// the line that tells a missing driver from one built against a newer
    /// C library than this program's (a NixOS whose system is newer than
    /// the flake's nixpkgs).
    pub fn why_none(refused: &[String]) -> String {
        if !refused.is_empty() {
            return refused.join("; ");
        }
        #[cfg(target_os = "linux")]
        {
            let failed = linux::unloadable();
            if !failed.is_empty() {
                return failed.join("; ");
            }
            if !std::path::Path::new("/run/opengl-driver").exists() && std::path::Path::new("/etc/NIXOS").exists() {
                return "no driver: /run/opengl-driver is missing (NixOS: hardware.graphics.enable)".into();
            }
        }
        "no Vulkan or GL driver answered".into()
    }

    #[cfg(target_os = "linux")]
    mod linux {
        use std::ffi::{CStr, CString};
        use std::os::raw::{c_char, c_int, c_void};
        use std::path::{Path, PathBuf};

        extern "C" {
            fn dlopen(name: *const c_char, flags: c_int) -> *mut c_void;
            fn dlclose(handle: *mut c_void) -> c_int;
            fn dlerror() -> *mut c_char;
        }
        const RTLD_NOW: c_int = 2;

        /// What the dynamic loader says loading `name`, if it fails.
        fn load(name: &str) -> Option<String> {
            let c = CString::new(name).ok()?;
            // SAFETY: a library opened and closed again on the main thread
            // before anything else runs; dlerror's string is copied before
            // the next call.
            unsafe {
                let handle = dlopen(c.as_ptr(), RTLD_NOW);
                if handle.is_null() {
                    let e = dlerror();
                    return Some(match e.is_null() {
                        true => "failed".into(),
                        false => CStr::from_ptr(e).to_string_lossy().into_owned(),
                    });
                }
                dlclose(handle);
            }
            None
        }

        fn split(var: &str) -> Vec<PathBuf> {
            std::env::var(var)
                .map(|v| v.split(':').filter(|p| !p.is_empty()).map(PathBuf::from).collect())
                .unwrap_or_default()
        }

        /// The manifests a loader reads: the variable's files and
        /// directories if it is set, else these directories.
        fn manifests(vars: &[&str], add: &str, dirs: &[PathBuf]) -> Vec<PathBuf> {
            let named: Vec<PathBuf> = vars.iter().flat_map(|v| split(v)).collect();
            let roots = if named.is_empty() { dirs.to_vec() } else { named };
            let mut out = Vec::new();
            for root in roots.into_iter().chain(split(add)) {
                match std::fs::read_dir(&root) {
                    Ok(entries) => {
                        let mut files: Vec<PathBuf> = entries
                            .flatten()
                            .map(|e| e.path())
                            .filter(|p| p.extension().is_some_and(|e| e == "json"))
                            .collect();
                        files.sort();
                        files.dedup();
                        out.extend(files);
                    }
                    Err(_) if root.is_file() => out.push(root),
                    Err(_) => {}
                }
            }
            out
        }

        /// A manifest's `library_path`, read by hand: the one string after
        /// that key.
        pub(super) fn library(json: &str) -> Option<String> {
            let after = &json[json.find("\"library_path\"")? + "\"library_path\"".len()..];
            let open = after.find('"')? + 1;
            let len = after[open..].find('"')?;
            Some(after[open..open + len].to_string())
        }

        /// Every loader and driver library the window would need that will
        /// not load here, with the loader's reason. Lavapipe and llvmpipe
        /// are left out: they are not a GPU.
        pub(super) fn unloadable() -> Vec<String> {
            let mut failed = Vec::new();
            for lib in ["libvulkan.so.1", "libEGL.so.1"] {
                if let Some(e) = load(lib) {
                    // The loader's message names the library itself.
                    failed.push(match e.starts_with(lib) {
                        true => e,
                        false => format!("{lib}: {e}"),
                    });
                }
            }
            let xdg: Vec<PathBuf> = split("XDG_DATA_DIRS");
            let share = |sub: &str| -> Vec<PathBuf> {
                let mut d = vec![Path::new("/run/opengl-driver/share").join(sub)];
                d.extend(xdg.iter().map(|x| x.join(sub)));
                d.extend([Path::new("/etc").join(sub), Path::new("/usr/share").join(sub)]);
                d
            };
            let vulkan = manifests(&["VK_DRIVER_FILES", "VK_ICD_FILENAMES"], "VK_ADD_DRIVER_FILES", &share("vulkan/icd.d"));
            let egl = manifests(
                &["__EGL_VENDOR_LIBRARY_FILENAMES", "__EGL_VENDOR_LIBRARY_DIRS"],
                "",
                &share("glvnd/egl_vendor.d"),
            );
            let mut drivers = 0;
            for manifest in vulkan.iter().chain(&egl) {
                let name = manifest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                if name.starts_with("lvp_") {
                    continue;
                }
                let Some(lib) = std::fs::read_to_string(manifest).ok().as_deref().and_then(library) else {
                    continue;
                };
                drivers += 1;
                if let Some(e) = load(&lib) {
                    failed.push(format!("{name} \u{2192} {e}"));
                }
            }
            if drivers == 0 && failed.is_empty() {
                failed.push("no Vulkan or EGL driver manifest found (looked in /run/opengl-driver/share, XDG_DATA_DIRS, /etc, /usr/share)".into());
            }
            failed.truncate(4);
            failed
        }
    }

    #[cfg(all(test, target_os = "linux"))]
    mod tests {
        /// The manifest's library is the string after its key, whatever the
        /// spacing. Falsified by taking the first quoted string.
        #[test]
        fn a_manifest_names_its_library() {
            let json = r#"{ "file_format_version" : "1.0.0", "ICD": { "api_version": "1.3", "library_path" : "/run/opengl-driver/lib/libvulkan_asahi.so" } }"#;
            assert_eq!(super::linux::library(json).as_deref(), Some("/run/opengl-driver/lib/libvulkan_asahi.so"));
            assert_eq!(super::linux::library("{}"), None);
        }
    }
}
