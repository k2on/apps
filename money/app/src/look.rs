//! What money's screens look like that is money's, not ArkUI's, for
//! every client that draws them: a category's glyph by the words in its
//! name, the tile it sits on, and the emoji the phones draw it with. (The desktop's `view.rs` and the
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

/// The emoji a category is drawn with before its name on the phones (the
/// reference's household: 🏠 Mortgage, ⚡️ Electricity), by the words in
/// its name as [`category_glyph`] reads them; none when no word says.
pub fn category_emoji(name: &str) -> &'static str {
    let name = name.to_lowercase();
    let says = |words: &[&str]| words.iter().any(|w| name.contains(w));
    match () {
        _ if says(&["household"]) => "\u{1F9FA}",
        _ if says(&["rent", "mortgage", "home"]) => "\u{1F3E0}",
        _ if says(&["electric", "power", "utilit"]) => "\u{26A1}\u{FE0F}",
        _ if says(&["water"]) => "\u{1F4A7}",
        _ if says(&["internet", "wifi"]) => "\u{1F310}",
        _ if says(&["phone", "mobile"]) => "\u{1F4F1}",
        _ if says(&["insurance"]) => "\u{2602}\u{FE0F}",
        _ if says(&["emergency"]) => "\u{1F6DF}",
        _ if says(&["grocer", "food", "supermarket"]) => "\u{1F6D2}",
        _ if says(&["dining", "restaurant", "coffee", "eat"]) => "\u{1F35C}",
        _ if says(&["gas", "transit", "transport", "fuel", "commute"]) => "\u{1F686}",
        _ if says(&["car", "repair", "maintenance"]) => "\u{1F527}",
        _ if says(&["entertain", "movie", "game", "fun"]) => "\u{1F39F}\u{FE0F}",
        _ if says(&["subscription", "streaming"]) => "\u{1F4FA}",
        _ if says(&["cloth", "shoe"]) => "\u{1F455}",
        _ if says(&["health", "medical", "doctor", "pharmacy"]) => "\u{1FA7A}",
        _ if says(&["vacation", "trip", "travel", "flight"]) => "\u{1F3D6}\u{FE0F}",
        _ if says(&["gift", "holiday", "birthday"]) => "\u{1F381}",
        _ if says(&["laptop", "computer", "tech"]) => "\u{1F4BB}",
        _ if says(&["saving", "invest", "retire"]) => "\u{1F4B0}",
        _ => "",
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

    /// The reference's emoji, by the same words; none for a name no word
    /// says. Falsified by Household drawn as a home, or a stray emoji.
    #[test]
    fn a_category_wears_its_emoji() {
        assert_eq!(category_emoji("Mortgage"), "\u{1F3E0}");
        assert_eq!(category_emoji("Household"), "\u{1F9FA}");
        assert_eq!(category_emoji("Dining Out"), "\u{1F35C}");
        assert_eq!(category_emoji("Something Else"), "");
    }
}

/// How a category or an account is offered where one is chosen by typing
/// (ArkUI's search picker): the group it is listed under, and the figure at
/// its end in its tint.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Offered {
    pub group: String,
    pub detail: String,
    pub tint: &'static str,
}

/// A category: under its group, what it has available in its tone's tint
/// (overspent negative, short or on a card caution).
pub fn category_offered(l: &money_core::BudgetLine, style: &ark_format::NumberStyle) -> Offered {
    Offered {
        group: l.group.clone(),
        detail: ark_format::amount(l.available, style),
        tint: crate::shell::inspect::tint_of(l.tone()),
    }
}

/// An account: under Budget Accounts or Tracking Accounts, its balance,
/// negative below nothing.
pub fn account_offered(a: &money_core::AccountLine, style: &ark_format::NumberStyle) -> Offered {
    Offered {
        group: if a.on_budget { "Budget Accounts" } else { "Tracking Accounts" }.into(),
        detail: ark_format::amount(a.balance, style),
        tint: if a.balance < 0 { "negative" } else { "neutral" },
    }
}

/// Ready to Assign, Inflow's one choice.
pub const INFLOW: &str = "Inflow";
/// Where a transfer's other accounts are listed among the categories.
pub const TRANSFERS: &str = "Transfers";
