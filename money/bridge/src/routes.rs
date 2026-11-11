//! money's routes as the phones read them: the twin's methods, each the
//! route's own (`money_app::routes`).

use ark_ffi::Twin;
use money_app::Route;

use crate::RouteFfi;

fn route(r: &RouteFfi) -> Route {
    Route::from_ffi(r.clone())
}

/// The route a URL names, or none: `Route::from_url`, for the phones'
/// deep links and launch arguments (UniFFI writes no constructor on an
/// enum): `/budget/`, `money://account/<id>/checking/`.
#[uniffi::export]
pub fn route_from_url(url: String) -> Option<RouteFfi> {
    Route::from_url(&url).map(Route::ffi)
}

#[uniffi::export]
impl RouteFfi {
    /// The URL (`Route::url`): `/account/<id>/checking/`.
    #[uniffi::method(name = "url")]
    pub fn ffi_url(&self) -> String {
        route(self).url()
    }

    #[uniffi::method(name = "title")]
    pub fn ffi_title(&self) -> String {
        route(self).title()
    }

    #[uniffi::method(name = "is_root")]
    pub fn ffi_is_root(&self) -> bool {
        route(self).is_root()
    }

    #[uniffi::method(name = "fallback")]
    pub fn ffi_fallback(&self) -> RouteFfi {
        route(self).fallback().ffi()
    }

    #[uniffi::method(name = "is_register")]
    pub fn ffi_is_register(&self) -> bool {
        route(self).is_register()
    }

    /// The row's id, or nil for a route that names no row (`Route::row_id`).
    #[uniffi::method(name = "row_id")]
    pub fn ffi_row_id(&self) -> Option<String> {
        route(self).row_id()
    }

    /// Whether the route names this row (`Route::names`).
    #[uniffi::method(name = "names")]
    pub fn ffi_names(&self, id: String, name: String) -> bool {
        route(self).names(&id, &name)
    }
}
