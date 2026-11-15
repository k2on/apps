//! The status screen laid out, once: drawn natively from this recording
//! (ui/docs/app.md, "Layout: #[view]").

use ark_app::view;
use ark_app::view::*;

use super::{StatusMsg, StatusPage};

/// What this phone is held to and what it last found: the policy, whether
/// the app is the device owner, the VPN, what is kept, and Apply Now.
#[view]
pub fn status(p: &StatusPage) -> View<StatusMsg> {
    detail_page((
        card_section((
            list_row("Policy").value(p.policy).glyph("settings"),
            list_row("Device Owner").value(p.owner).glyph("users"),
        )),
        card_section((
            list_row("Always-On VPN").value(p.vpn).glyph("cloud"),
            list_row("Lockdown").value(p.lockdown).glyph("eye"),
            list_row("Restrictions").value(p.restrictions).glyph("settings"),
            list_row("Kept Installed").value(p.kept).glyph("landmark"),
            list_row("Security Patch").value(p.patch).glyph("mail"),
        )),
        card_section(list_row("Last Applied").value(p.last).glyph("cloud")),
        status_line(p.doing, p.busy, p.problem),
        wide_button("Apply Now", true, p.idle, p.busy, StatusMsg::Refresh),
    ))
}
