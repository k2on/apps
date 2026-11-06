// Who is signed in, at the top of a tab: their face at the end of the large
// title's line, as the iPhone's own apps have it (Apple's Games, on the
// right) — not in a bar of its own above the title, and with no button's
// chrome: the round picture, or the initials on the accent, alone. Tapped, it
// raises the account sheet rather than opening Settings: who they are, the
// server and whether it answers, what the app adds (money's banks that want a
// look), Settings, and Sign Out. Only a tab's root has it; a page pushed over
// a tab does not. The iPhone's twin is ArkUI's `AccountFace` and
// `AccountSheet` (swift/ArkUI, Account.swift), with the same words.
package dev.arkui

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.launch

/** The account's sizes: its face's target and badge, and the sheet's face and groups. */
public object Account {
    /** What a finger has of the face: Material's minimum target, the face centred in it. */
    public val Target: Dp = 48.dp

    /** The dot on the face while something wants a look, and how far it sits out over the face's edge. */
    public val Badge: Dp = 9.dp
    public val BadgeOut: Dp = 2.dp

    /** The face at the top of the sheet. */
    public val SheetFace: Dp = 72.dp

    /** A group of the sheet's rows: its corners, and a row's least height. */
    public val Corner: Dp = 12.dp
    public val RowHeight: Dp = 52.dp

    /** What the face's tap does, as a screen reader says it: the iPhone's hint. */
    public const val Hint: String = "Account and settings"

    /** What a screen reader hears of the face. */
    public fun label(name: String): String = "Signed in as ${name.ifEmpty { "?" }}"
}

/**
 * Who is signed in, for the end of a tab's large title ([TabTitle]'s `account`, [LargeTitleLine]):
 * their [Face] at [FaceSide], badged in the error colour while [attention] says what wants a look
 * (money's banks), raising the account sheet on a tap ([onClick]). A screen reader hears "Signed in
 * as …", then [attention], and that the tap is for the account and settings.
 */
@Composable
public fun AccountFace(
    name: String,
    picture: String,
    onClick: () -> Unit,
    modifier: Modifier = Modifier,
    attention: String? = null,
) {
    val said = Account.label(name) + (attention?.let { ", $it" } ?: "")
    Box(
        modifier.size(Account.Target).pressable(onClick = onClick, onClickLabel = Account.Hint, role = Role.Button),
        contentAlignment = Alignment.Center,
    ) {
        Box {
            Face(name.ifEmpty { "?" }, picture, FaceSide, description = said)
            if (attention != null) {
                Box(
                    Modifier.align(Alignment.TopEnd)
                        .offset(x = Account.BadgeOut, y = -Account.BadgeOut)
                        .size(Account.Badge)
                        .background(MaterialTheme.colorScheme.error, CircleShape)
                )
            }
        }
    }
}

/**
 * The account sheet the face raises: their face large, their name and [detail] (an email, a user
 * name, or what the demo is), the [server] (its host; none for the demo) and whether it is [linked],
 * what the app adds ([rows], [AccountRow]s in an [AccountGroup]), Settings and [signOut], and Done.
 * A row that leaves the sheet puts it away first, then does what it said: [onSettings] opens the
 * Settings page, [onSignOut] signs out. [onDismiss] is told the sheet has gone, however it went.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
public fun AccountSheet(
    name: String,
    picture: String,
    detail: String,
    server: String?,
    linked: Boolean,
    onSettings: () -> Unit,
    onSignOut: () -> Unit,
    onDismiss: () -> Unit,
    signOut: String = "Sign Out",
    rows: @Composable ColumnScope.(leave: (() -> Unit) -> Unit) -> Unit = {},
) {
    val sheet = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    val scope = rememberCoroutineScope()
    // Down first, then what the row said: a page opening under a sheet still on its way down.
    val leave: (() -> Unit) -> Unit = { then ->
        scope.launch { sheet.hide() }.invokeOnCompletion {
            onDismiss()
            then()
        }
    }
    ModalBottomSheet(onDismissRequest = onDismiss, sheetState = sheet) {
        Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(bottom = Space.xl)) {
            Row(Modifier.fillMaxWidth().padding(horizontal = Space.s), horizontalArrangement = Arrangement.End) {
                TextButton(onClick = { leave {} }) { Text("Done", fontWeight = FontWeight.SemiBold) }
            }
            Column(
                Modifier.fillMaxWidth().padding(horizontal = Space.page).semantics(mergeDescendants = true) { heading() },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Face(name.ifEmpty { "?" }, picture, Account.SheetFace, description = "")
                Spacer(Modifier.height(Space.m))
                Text(name.ifEmpty { "Signed In" }, style = MaterialTheme.typography.titleLarge, fontWeight = FontWeight.Bold, textAlign = TextAlign.Center)
                if (detail.isNotEmpty()) Text(detail, style = quiet(MaterialTheme.typography.bodyMedium), textAlign = TextAlign.Center)
            }
            Spacer(Modifier.height(Space.xl))
            if (server != null) {
                AccountGroup {
                    if (linked) AccountRow(ArkGlyphs.Linked, "Connected to $server", tint = MaterialTheme.colorScheme.primary)
                    else AccountRow(ArkGlyphs.Unlinked, "Can't Reach $server", under = "What's on this phone still works", tint = MaterialTheme.colorScheme.error)
                }
            }
            rows(leave)
            AccountGroup { AccountRow(ArkGlyphs.Settings, "Settings", onClick = { leave(onSettings) }, chevron = true) }
            AccountGroup {
                val red = MaterialTheme.colorScheme.error
                AccountRow(ArkGlyphs.SignOut, signOut, onClick = { leave(onSignOut) }, tint = red, ink = red)
            }
        }
    }
}

/** A group of the account sheet's rows, on a container a step off the sheet, as Settings' groups are. */
@Composable
public fun AccountGroup(content: @Composable ColumnScope.() -> Unit) {
    Surface(
        color = MaterialTheme.colorScheme.surfaceContainer,
        shape = RoundedCornerShape(Account.Corner),
        modifier = Modifier.fillMaxWidth().padding(horizontal = Space.page, vertical = Space.s),
    ) {
        Column(content = content)
    }
}

/**
 * A row of the account sheet: a glyph in [tint], what it says in [ink] and [under] it, and, when it
 * does something ([onClick]), a press and, for one that opens a page, a [chevron].
 */
@Composable
public fun AccountRow(
    icon: ImageVector,
    label: String,
    onClick: (() -> Unit)? = null,
    under: String? = null,
    tint: Color = MaterialTheme.colorScheme.onSurfaceVariant,
    ink: Color = MaterialTheme.colorScheme.onSurface,
    chevron: Boolean = false,
) {
    Row(
        Modifier.fillMaxWidth()
            .let { if (onClick != null) it.pressable(onClick = onClick, press = Press.Highlight, role = Role.Button) else it.semantics(mergeDescendants = true) {} }
            .heightIn(min = Account.RowHeight)
            .padding(horizontal = Space.l, vertical = Space.s),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, contentDescription = null, tint = tint, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(Space.l))
        Column(Modifier.weight(1f)) {
            Text(label, style = MaterialTheme.typography.bodyLarge, color = ink)
            if (under != null) Text(under, style = quiet(MaterialTheme.typography.bodySmall))
        }
        if (chevron) Chevron()
    }
}
