//! A page's route by its tag, and back: the routes themselves are files, in
//! `src/app` (`crate::Route`, `ark_router::routes!`).
use crate::pages::OVERVIEW;
use crate::Route;

impl Route {
    /// The page tagged `tag`.
    pub fn of(tag: &str) -> Route {
        match tag {
            OVERVIEW | "" => Route::Overview,
            t => Route::Page { tag: t.into() },
        }
    }

    /// The tag of the page it names.
    pub fn tag(&self) -> &str {
        match self {
            Route::Overview => OVERVIEW,
            Route::Page { tag } => tag,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every page's route survives its URL, and its URL is its tag's.
    /// Falsified by spelling the overview `/overview/`.
    #[test]
    fn every_page_survives_its_url() {
        let pages = crate::pages::pages("/");
        let routes: Vec<Route> = pages.iter().map(|p| Route::of(&p.tag)).collect();
        ark_router::assert_round_trips(&routes);
        assert_eq!(Route::of(OVERVIEW).url(), "/");
        assert_eq!(Route::of("menu").url(), "/menu/");
        assert_eq!(Route::from_url("/menu/?q=spring"), Some(Route::of("menu")), "the search is the reader's");
        for p in &pages {
            assert_eq!(crate::pages::href("/", &p.tag), Route::of(&p.tag).url());
        }
        // A static export writes every page, in reading order, and no other.
        assert_eq!(Route::statics(), routes);
    }
}
