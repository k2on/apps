//! harken's routes as the phones read them: the twin's methods, each the
//! route's own (`harken_app::routes`).

use ark_ffi::Twin;
use harken_app::Route;

use crate::RouteFfi;

fn route(r: &RouteFfi) -> Route {
    Route::from_ffi(r.clone())
}

/// The route a URL names, or none: `Route::from_url`, for the phones'
/// deep links and launch arguments (UniFFI writes no constructor on an
/// enum): `/album/water-music/`, `harken://album/water-music/`.
#[uniffi::export]
pub fn route_from_url(url: String) -> Option<RouteFfi> {
    Route::from_url(&url).map(Route::ffi)
}

#[uniffi::export(name = "Route")]
impl RouteFfi {
    /// The URL (`Route::url`): `/album/Water%20Music/`.
    #[uniffi::method(name = "url")]
    pub fn ffi_url(&self) -> String {
        route(self).url()
    }

    #[uniffi::method(name = "title")]
    pub fn ffi_title(&self) -> String {
        route(self).title()
    }

    #[uniffi::method(name = "kind")]
    pub fn ffi_kind(&self) -> String {
        route(self).kind().into()
    }

    #[uniffi::method(name = "heading")]
    pub fn ffi_heading(&self) -> Option<String> {
        route(self).heading().map(String::from)
    }

    #[uniffi::method(name = "is_root")]
    pub fn ffi_is_root(&self) -> bool {
        route(self).is_root()
    }

    #[uniffi::method(name = "fallback")]
    pub fn ffi_fallback(&self) -> RouteFfi {
        route(self).fallback().ffi()
    }

    #[uniffi::method(name = "work")]
    pub fn ffi_work(&self) -> Option<RouteFfi> {
        route(self).work().map(Route::ffi)
    }

    /// Whether the route is this playlist's (`Route::names`).
    #[uniffi::method(name = "names")]
    pub fn ffi_names(&self, id: String, name: String) -> bool {
        route(self).names(&id, &name)
    }
}
