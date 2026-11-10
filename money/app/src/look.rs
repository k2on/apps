//! What money's screens look like that is money's, not ArkUI's, for
//! every client that draws them: a category's glyph by the words in its
//! name, and the tile it sits on. (The desktop's `view.rs` and the
//! browser's `screens.rs` read these.)

/// What a category is for, by the words in its name, as one of ArkUI's
/// glyph names (`arkui_glyphs::named`); a tag when none says.
pub fn category_glyph(name: &str) -> &'static str {
    let name = name.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| name.contains(w));
    match () {
        _ if says(&["rent", "mortgage", "home", "household"]) => "house",
        _ if says(&["electric", "power", "utilit"]) => "zap",
        _ if says(&["water"]) => "droplet",
        _ if says(&["internet", "wifi"]) => "wifi",
        _ if says(&["phone", "mobile"]) => "smartphone",
        _ if says(&["insurance", "emergency"]) => "shield-check",
        _ if says(&["grocer", "food", "supermarket"]) => "shopping-cart",
        _ if says(&["dining", "restaurant", "coffee", "eat"]) => "utensils",
        _ if says(&["gas", "transit", "fuel", "commute"]) => "fuel",
        _ if says(&["car", "repair", "maintenance"]) => "wrench",
        _ if says(&["entertain", "movie", "game", "fun"]) => "play",
        _ if says(&["subscription", "streaming"]) => "repeat",
        _ if says(&["cloth", "shoe"]) => "shirt",
        _ if says(&["health", "medical", "doctor", "pharmacy"]) => "heart",
        _ if says(&["vacation", "trip", "travel", "flight"]) => "send",
        _ if says(&["gift", "holiday", "birthday"]) => "gift",
        _ if says(&["laptop", "computer", "tech"]) => "laptop",
        _ if says(&["saving", "invest", "retire"]) => "trending-up",
        _ => "tag",
    }
}

/// The tiles a category's glyph sits on, one picked by its name
/// ([`tile_of`]) so it is the same tile everywhere: saturated enough to
/// read on the light page and the dark one alike, a white glyph on each.
/// The desktop's `palette::TILES` are these.
pub const TILES: [&str; 8] = ["#2AA7C0", "#F0567A", "#A8865A", "#34A853", "#F08A24", "#8E6CEF", "#3B82F6", "#D4A21A"];

/// A card's payment category's tile: every card's the same slate.
pub const CARD_TILE: &str = "#475569";

/// The tile of the category called `name`: FNV-1a over its UTF-16, as
/// ArkUI's derived squares pick theirs (`arkui::art::hash`).
pub fn tile_of(name: &str) -> &'static str {
    let mut h: u32 = 0x811c_9dc5;
    for unit in name.encode_utf16() {
        h ^= u32::from(unit);
        h = h.wrapping_mul(0x0100_0193);
    }
    TILES[h as usize % TILES.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A name's words pick its glyph. Falsified by a word moved to another
    /// glyph.
    #[test]
    fn a_category_is_drawn_by_its_words() {
        assert_eq!(category_glyph("Groceries"), "shopping-cart");
        assert_eq!(category_glyph("Dining Out"), "utensils");
        assert_eq!(category_glyph("Something Else"), "tag");
        assert_eq!(tile_of("Groceries"), tile_of("Groceries"));
    }
}
