// Settings: who is signed in, which server and whether it can be reached,
// the podcasts and sermons the library follows, signing out, and what this
// is. Nothing a person sees here is a cursor, a
// hash or a count of pending intents: those are the diagnostics, behind
// the version — tapped seven times, the way Android's own build number
// unlocks its developer options — and remembered once found. And who is
// signed in as a face, their picture round, which Home's bar opens this by.
package dev.harken.android

import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.FilterChip
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.TextButton
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import dev.arkdb.ffi.Status
import dev.harken.android.ui.Art
import dev.harken.android.ui.Glyph
import dev.harken.android.ui.LocalArkTheme
import dev.harken.android.ui.LocalCoverLoader
import dev.harken.android.ui.PageBar
import dev.harken.android.ui.Space
import dev.harken.android.ui.quiet
import dev.harken.android.ui.rememberDestinationScrollState

/** What Settings draws. */
data class SettingsData(
    val name: String,
    val user: String,
    val email: String,
    val server: String,
    val linked: Boolean,
    val version: String,
    val diagnostics: Boolean,
    /** Their provider's picture of them, or empty. */
    val picture: String = "",
)

/** Who is signed in, as their face draws them: what to call them, and their provider's picture or none. */
@Immutable
data class Who(val name: String, val picture: String = "") {
    companion object {
        /**
         * What a face says without a picture: the first letters of the name's first and last words
         * (`Ada Lovelace` is `AL`, `Ada` is `A`), upper case — the desktop's `Who::initials`.
         */
        fun initials(name: String): String {
            fun first(w: String): Char? = w.firstOrNull { it.isLetterOrDigit() }
            val words = name.split(Regex("\\s+")).filter { first(it) != null }
            val letters = when (words.size) {
                0 -> emptyList()
                1 -> listOfNotNull(first(words[0]))
                else -> listOfNotNull(first(words.first()), first(words.last()))
            }
            return letters.joinToString("") { it.uppercase() }
        }

        /** Black or white, whichever reads on `fill`: the desktop's rule, on the same colours. */
        fun inkOn(fill: Color): Color = if (0.2126f * fill.red + 0.7152f * fill.green + 0.0722f * fill.blue > 0.55f) Color.Black else Color.White
    }
}

/**
 * Who is signed in, round, `side` across: their provider's picture once it has arrived — fetched
 * from the provider and kept as a cover is, by the app's cover loader — over their initials on
 * the name's own colour, as the desktop's account button draws them (`arkui::account::face`). It
 * says `description` and nothing else to a screen reader.
 */
@Composable
fun Avatar(who: Who, side: Dp, description: String? = null, modifier: Modifier = Modifier) {
    val tint = Art.pick(LocalArkTheme.current.art, who.name).first
    val said = Modifier.clearAndSetSemantics { if (description != null) contentDescription = description }
    Box(modifier.then(said).size(side).clip(CircleShape).background(tint), contentAlignment = Alignment.Center) {
        Text(Who.initials(who.name), color = Who.inkOn(tint), fontSize = (side.value * 0.4f).sp, fontWeight = FontWeight.SemiBold)
        val covers = LocalCoverLoader.current
        val url = remember(covers, who.picture) { covers?.urlOf(who.picture) }
        if (covers != null && url != null) {
            var bitmap by remember(url) { mutableStateOf<ImageBitmap?>(covers.cached(url)) }
            if (bitmap == null) LaunchedEffect(url) { bitmap = covers.load(url, side) }
            bitmap?.let { Image(it, contentDescription = null, contentScale = ContentScale.Crop, modifier = Modifier.fillMaxSize()) }
        }
    }
}

@Composable
fun SettingsScreen(
    data: SettingsData,
    onBack: () -> Unit,
    onSignOut: () -> Unit,
    onVersion: () -> Unit,
    onDiagnostics: () -> Unit,
    feeds: List<Subscription> = emptyList(),
    onFollow: (kind: String, url: String) -> Unit = { _, _ -> },
    onUnfollow: (Subscription) -> Unit = {},
) {
    Column(Modifier.fillMaxSize()) {
        PageBar("Settings", onBack)
        Column(Modifier.fillMaxWidth().verticalScroll(rememberDestinationScrollState("settings")).padding(horizontal = Space.page)) {
            Group("Account") {
                Row(Modifier.padding(Space.l), verticalAlignment = Alignment.CenterVertically) {
                    Avatar(Who(data.name, data.picture), 48.dp)
                    Spacer(Modifier.width(Space.l))
                    Column {
                        Text(data.name, style = MaterialTheme.typography.titleMedium)
                        Text(data.email.ifEmpty { data.user }, style = quiet(MaterialTheme.typography.bodyMedium))
                    }
                }
            }
            Group("Server") {
                Line(hostOf(data.server), if (data.linked) "Connected" else "Can't reach it right now — what's on this phone still works")
            }
            Group("Podcasts and sermons") { Feeds(feeds, onFollow, onUnfollow) }
            Spacer(Modifier.height(Space.l))
            OutlinedButton(
                onClick = onSignOut,
                colors = ButtonDefaults.outlinedButtonColors(contentColor = MaterialTheme.colorScheme.error),
                modifier = Modifier.fillMaxWidth(),
            ) {
                Glyph(Glyphs.SignOut, null, size = 18.dp)
                Spacer(Modifier.width(Space.s))
                Text("Sign out")
            }
            Text(
                "Anything not synced yet stays on this phone, and goes when you sign in again.",
                style = quiet(MaterialTheme.typography.bodySmall),
                modifier = Modifier.padding(top = Space.s),
            )
            Group("About") {
                Line("Harken", "Your music, from your own server")
                Line("Version", data.version, onClick = onVersion)
                if (data.diagnostics) Line("Diagnostics", "What this phone's copy of the library is doing", onClick = onDiagnostics, chevron = true)
            }
            Spacer(Modifier.height(Space.xl))
        }
    }
}

/**
 * The feeds the library follows, each with how much of it is here, why its last read failed and
 * its Unfollow; and one followed by its URL, as a podcast or as sermons — the desktop's Settings.
 */
@Composable
private fun Feeds(feeds: List<Subscription>, onFollow: (String, String) -> Unit, onUnfollow: (Subscription) -> Unit) {
    var url by remember { mutableStateOf("") }
    var kind by remember { mutableStateOf("podcast") }
    for (f in feeds) {
        Row(Modifier.fillMaxWidth().padding(start = Space.l, end = Space.s, top = Space.s, bottom = Space.s), verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(f.title.ifEmpty { f.url }, style = MaterialTheme.typography.bodyLarge, maxLines = 2)
                val what = if (f.kind == "sermon") "Sermons" else "Podcast"
                val here = if (f.title.isEmpty() && f.error.isEmpty()) "not read yet" else "${f.episodes} of ${Media.count(f.items.toInt(), "episode")} here"
                Text("$what · $here", style = quiet(MaterialTheme.typography.bodyMedium))
                if (f.error.isNotEmpty()) Text("Could not read it: ${f.error}", style = quiet(MaterialTheme.typography.bodySmall))
            }
            TextButton(onClick = { onUnfollow(f) }) { Text("Unfollow") }
        }
    }
    Column(Modifier.padding(Space.l)) {
        Text(
            "Follow a podcast or a church's sermons by its RSS feed. The server downloads its newest episodes and checks it every hour; a show's page fetches older ones.",
            style = quiet(MaterialTheme.typography.bodyMedium),
        )
        Row(Modifier.padding(top = Space.s), verticalAlignment = Alignment.CenterVertically) {
            FilterChip(selected = kind == "podcast", onClick = { kind = "podcast" }, label = { Text("Podcast") })
            Spacer(Modifier.width(Space.s))
            FilterChip(selected = kind == "sermon", onClick = { kind = "sermon" }, label = { Text("Sermons") })
        }
        OutlinedTextField(
            value = url,
            onValueChange = { url = it },
            label = { Text("Feed URL") },
            singleLine = true,
            modifier = Modifier.fillMaxWidth().padding(top = Space.s),
        )
        OutlinedButton(
            onClick = {
                onFollow(kind, url)
                url = ""
            },
            enabled = url.isNotBlank(),
            modifier = Modifier.padding(top = Space.s),
        ) {
            Text("Follow")
        }
    }
}

// The host and port of a server URL, as a person writes it.
private fun hostOf(url: String): String = url.substringAfter("://")

@Composable
private fun Group(title: String, content: @Composable () -> Unit) {
    Text(title, style = quiet(MaterialTheme.typography.labelLarge), modifier = Modifier.padding(start = Space.xs, top = Space.xl, bottom = Space.s))
    Surface(color = MaterialTheme.colorScheme.surfaceContainer, shape = RoundedCornerShape(12.dp), modifier = Modifier.fillMaxWidth()) {
        Column { content() }
    }
}

@Composable
private fun Line(title: String, under: String, onClick: (() -> Unit)? = null, chevron: Boolean = false) {
    Row(
        Modifier.fillMaxWidth().let { if (onClick != null) it.clickable(onClick = onClick) else it }.padding(Space.l),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge)
            Text(under, style = quiet(MaterialTheme.typography.bodyMedium))
        }
        if (chevron) Icon(Glyphs.Chevron, contentDescription = null, tint = MaterialTheme.colorScheme.onSurfaceVariant, modifier = Modifier.size(18.dp))
    }
}

/**
 * Every number the old settings screen showed, every change this phone made with where it stands,
 * and every number this end of the listening session holds — the desktop's `D`: whether this phone
 * stands in it, who is here, where the sound is, everybody's scrubber, and what was refused. (A
 * chain checks itself entry by entry, §1.6: there is no verify to ask for.)
 */
@Composable
fun DiagnosticsScreen(d: Diagnostics?, onBack: () -> Unit, heard: Heard = Heard()) {
    Column(Modifier.fillMaxSize()) {
        PageBar("Diagnostics", onBack)
        Column(Modifier.fillMaxWidth().verticalScroll(rememberDestinationScrollState("diagnostics")).padding(horizontal = Space.page)) {
            val s = d?.status
            Group("Peer") {
                if (s == null) Line("Opening", "…") else for ((k, v) in numbers(s)) Number(k, v)
            }
            Group("Listening") { for ((k, v) in listeningNumbers(heard)) Number(k, v) }
            Group("Changes from this phone") {
                val changes = d?.changes.orEmpty()
                if (changes.isEmpty()) Line("None yet", "What you change here is listed with where it stands")
                for ((what, standing) in changes) Line(what, standing)
            }
            Spacer(Modifier.height(Space.xl))
        }
    }
}

/** The status's numbers, by name, as the peer says them. */
fun numbers(s: Status): List<Pair<String, String>> =
    listOf(
        "user" to s.user,
        "session" to s.session,
        "link" to s.link,
        "linked" to s.linked.toString(),
        "epoch" to s.epoch.toString(),
        "cursor" to s.cursor.toString(),
        "not synced" to s.pending.toString(),
        "joining" to s.joining.toString(),
        "alone" to s.alone.toString(),
        "disk" to (s.failure ?: "—"),
        "denied" to (s.denied ?: "—"),
    )

/** The listening session's numbers, by name: what `D` draws on the desktop. */
fun listeningNumbers(h: Heard): List<Pair<String, String>> {
    val s = h.listening
    val a = s?.active
    val out =
        mutableListOf(
            "device" to h.me.ifEmpty { "—" },
            "part of the session" to h.joined.toString(),
            "linked" to h.linked.toString(),
            "epoch" to h.epoch.toString(),
            "calls" to h.stats.calls.toString(),
            "refused" to h.stats.refused.toString(),
            "plays recorded" to h.stats.plays.toString(),
            "came back playing" to h.stats.cameBackPlaying.toString(),
            "last refusal" to (h.refused ?: h.stats.lastRefusal ?: "—"),
        )
    out +=
        "session" to
            when {
                s == null -> "—"
                a == null -> "idle"
                else -> "${a.at.toInt() + 1} of ${a.queue.size}, ${if (a.clock.playing) "playing" else "paused"} at ${Media.clock(a.position(h.serverNow()))}"
            }
    out +=
        "output" to
            when (val o = a?.output) {
                null -> "—"
                is dev.harken.android.gen.Output.Device -> o.id + if (o.moving) " (moving)" else ""
                is dev.harken.android.gen.Output.Away -> "${o.id} (away)"
                dev.harken.android.gen.Output.Nowhere -> "nowhere"
            }
    for (d in s?.devices?.values.orEmpty()) {
        out += "device ${d.name}" to "${d.kind}, ${if (d.canPlay) "can play" else "cannot play"}, ${d.id}"
        // Its volume, its family and its leader: the desktop's `D`, a line per device.
        val volume = if (!d.canVolume) "none" else if (d.muted) "${d.volume} (muted)" else "${d.volume}"
        out += "  volume" to "$volume · family ${d.family ?: "none"} · follows ${d.follows ?: "nobody"}"
    }
    // The group that plays: its leader, who follows it, and its level.
    if (s != null)
        out +=
            "group" to
                (s.leader?.takeIf { s.grouped }?.let { l ->
                    val level = s.groupLevel(l)?.toString() ?: "none"
                    "$l + ${s.followers(l).joinToString(", ") { it.id }} · volume $level${if (s.groupMuted(l)) " (muted)" else ""}"
                } ?: "none")
    if (s != null && s.scrub.isEmpty()) out += "scrub" to "none"
    for ((id, at) in s?.scrub.orEmpty()) out += "scrub ${if (id == h.me) "(this phone)" else id}" to Media.clock(at)
    return out
}

@Composable
private fun Number(name: String, value: String) {
    Row(Modifier.fillMaxWidth().padding(horizontal = Space.l, vertical = Space.s)) {
        Text(name, style = quiet(MaterialTheme.typography.bodyMedium), modifier = Modifier.weight(1f))
        Text(value, style = MaterialTheme.typography.bodyMedium.copy(fontFamily = FontFamily.Monospace))
    }
}
