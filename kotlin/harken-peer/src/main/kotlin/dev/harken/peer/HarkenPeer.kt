// `harken-peer-kt`: `harken-peer` (harken/server/src/bin/harken-peer.rs) on the
// Kotlin client — a harken peer with no screen, speaking exactly its JSON
// lines, so that the fleet drives a Kotlin peer as it drives a Rust one
// (`docs/plan-phones.md` round 4, `docs/plan-fleet.md`).
//
//     harken-peer-kt --dir DIR --server URL [--user NAME]     a peer of that server
//     harken-peer-kt --dir DIR --alone [--user NAME]          its own authority
//       [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
//       [--auth-patience-ms N] [--module FILE] [--roles ROLE,ROLE]
//       [--device KIND,NAME,CAN_PLAY]
//
// The flags, the commands and the answers are the reference's, line for line;
// its first page is the documentation, and this file says only where it is
// not the same program:
//
// - **Nothing is native.** The reference runs harken's procedures natively
//   wherever a hash matches; a phone's runtime interprets every function of
//   the module it loads — `harken.ark`, carried in this program's jar, or the
//   `.ark` file `--module` names (a grown domain, an older revision's, a
//   scoped one).
// - **The directory is the same records.** `DIR/replica` is the storage the
//   reference writes, byte for byte (`kotlin/ark-client`), and `DIR/login.json`
//   and `DIR/rejections.jsonl` are the same files.
// - What happens on the link goes to stderr prefixed `harken-peer-kt:`.
//
// It is pumped as the reference is — **when something happens and at no
// other time** (`docs/plan-reactive.md`): when the socket wakes it (opened, a
// frame, closed), after every command that asks something of the peer, and
// at the one deadline the peer asks for, the redial after a close. A read
// pumps nothing, and `wait` and `settle` pump only on wakes, so a frame the
// transport did not wake anyone for is a fleet that does not converge.
// `--pump-ms` is, as there, how long `settle` waits with no wake before the
// cursor counts as still.
package dev.harken.peer

import dev.arkdb.DeviceDecl
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.Json
import dev.arkdb.JsonError
import dev.arkdb.Kind
import dev.arkdb.Value
import dev.arkdb.client.Auth
import dev.arkdb.client.AuthError
import dev.arkdb.client.DirStorage
import dev.arkdb.client.Domain
import dev.arkdb.client.Link
import dev.arkdb.client.Login
import dev.arkdb.client.Options
import dev.arkdb.client.Patience
import dev.arkdb.client.Peer
import dev.arkdb.client.PeerError
import dev.arkdb.client.Standing
import dev.arkdb.client.Timing
import java.io.File
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import kotlin.system.exitProcess

private const val USAGE =
    """usage: harken-peer-kt --dir DIR (--server URL [--user NAME] | --alone [--user NAME])
                      [--pump-ms 50] [--ping-ms 20000] [--backoff-ms 500,30000]
                      [--auth-patience-ms N] [--module FILE] [--roles ROLE,ROLE]
                      [--device KIND,NAME,CAN_PLAY]
Commands are JSON lines on stdin; see harken/server/src/bin/harken-peer.rs."""

/** What the command line says. */
internal data class Flags(
    val dir: File,
    val server: String?,
    val user: String?,
    val alone: Boolean,
    val pumpMs: Long,
    val timing: Timing,
    val patience: Patience,
    val module: File?,
    val roles: List<String>,
    /** `--device`: what this peer declares itself as at `hello` (`docs/plan-live.md`). */
    val device: DeviceDecl? = null,
)

internal class FlagError(what: String) : Exception(what)

internal fun flags(args: List<String>): Flags {
    var dir: File? = null
    var server: String? = null
    var user: String? = null
    var alone = false
    var pump = 50L
    var timing = Timing()
    var patience = Patience.DEFAULT
    var module: File? = null
    var roles = emptyList<String>()
    var device: DeviceDecl? = null
    val it = args.iterator()
    fun next(what: String): String = if (it.hasNext()) it.next() else throw FlagError(what)
    fun ms(name: String): Long =
        next("$name needs a value").toLongOrNull()?.takeIf { it >= 0 }
            ?: throw FlagError("$name takes milliseconds")
    while (it.hasNext()) {
        when (val a = it.next()) {
            "--dir" -> dir = File(next("--dir needs a value"))
            "--server" -> server = next("--server needs a value").trimEnd('/')
            "--user" -> user = next("--user needs a value")
            "--alone" -> alone = true
            "--pump-ms" -> pump = maxOf(ms("--pump-ms"), 1)
            "--ping-ms" -> timing = timing.copy(pingEveryMs = ms("--ping-ms"))
            "--backoff-ms" -> {
                val v = next("--backoff-ms needs FIRST,MAX")
                val parts = v.split(',', limit = 2)
                if (parts.size != 2) throw FlagError("--backoff-ms takes FIRST,MAX")
                val (f, m) =
                    parts.map {
                        it.trim().toLongOrNull()?.takeIf { n -> n >= 0 }
                            ?: throw FlagError("--backoff-ms takes milliseconds")
                    }
                timing = timing.copy(firstBackoffMs = f, maxBackoffMs = m)
            }
            "--auth-patience-ms" -> patience = Patience.of(maxOf(ms("--auth-patience-ms"), 1))
            "--module" -> module = File(next("--module needs a file"))
            "--roles" ->
                roles =
                    next("--roles needs ROLE,ROLE")
                        .split(',')
                        .map { it.trim() }
                        .filter { it.isNotEmpty() }
            "--device" -> {
                // The kind before the first comma, whether it can play after the last, and the
                // name — which may hold commas of its own — between.
                val v = next("--device needs KIND,NAME,CAN_PLAY")
                val first = v.indexOf(',')
                val last = v.lastIndexOf(',')
                if (first < 0 || last == first) throw FlagError("--device takes KIND,NAME,CAN_PLAY")
                val canPlay =
                    when (v.substring(last + 1)) {
                        "true" -> true
                        "false" -> false
                        else -> throw FlagError("--device's CAN_PLAY is true or false")
                    }
                device =
                    DeviceDecl("", v.substring(0, first), v.substring(first + 1, last), canPlay)
            }
            "-h",
            "--help" -> throw FlagError(USAGE)
            else -> throw FlagError("unexpected argument $a\n$USAGE")
        }
    }
    val d = dir ?: throw FlagError("--dir is required\n$USAGE")
    if (alone && server != null) throw FlagError("--alone has no server")
    if (!alone && server == null) throw FlagError("--server or --alone\n$USAGE")
    return Flags(d, server, user, alone, pump, timing, patience, module, roles, device)
}

// -- the login, remembered under DIR ------------------------------------------------

private fun loginFile(dir: File): File = File(dir, "login.json")

/** `DIR/login.json`, `{"server":…,"login":…}`: the login for one server. */
internal fun recall(dir: File, server: String): Login? =
    try {
        val v = Json.decode(loginFile(dir).readText(Charsets.UTF_8)).asStruct()
        if (v["server"].asText() != server) null else Login.of(v["login"])
    } catch (e: Exception) {
        null
    }

internal fun remember(dir: File, server: String, login: Login) {
    val tmp = File(dir, ".login.json.tmp")
    tmp.writeText("{\"server\":${Json.quoted(server)},\"login\":${login.toJson()}}", Charsets.UTF_8)
    if (!tmp.renameTo(loginFile(dir))) throw PeerError("${loginFile(dir)}: cannot rename")
}

private fun forget(dir: File) {
    loginFile(dir).delete()
}

/** Sign `user` in at `server`: a dev server answers with a code and no page. */
private fun logIn(server: String, user: String, patience: Patience): Login =
    Auth.login(server, user, patience) { url -> say("open $url to sign in") }

private fun say(line: String) {
    System.err.println("harken-peer-kt: $line")
    System.err.flush()
}

// -- a command, and its answer --------------------------------------------------------

/** One line from stdin, read. */
internal sealed class Cmd {
    data class Mutate(val name: String, val args: Map<String, Value>) : Cmd()

    object Status : Cmd()

    object HashCmd : Cmd()

    data class Wait(val cursor: Long, val timeoutMs: Long) : Cmd()

    data class Settle(val timeoutMs: Long) : Cmd()

    data class Query(val name: String, val args: Map<String, Value>) : Cmd()

    data class Rejections(val all: Boolean) : Cmd()

    data class StandingOf(val id: Id) : Cmd()

    object Disconnect : Cmd()

    object Reconnect : Cmd()

    data class SignIn(val user: String) : Cmd()

    object SignOut : Cmd()

    data class Join(val server: String, val user: String?) : Cmd()

    object Leave : Cmd()

    data class Call(val live: String, val transition: String, val input: Map<String, Value>) :
        Cmd()

    data class Live(val name: String) : Cmd()

    object LiveRefusals : Cmd()

    object Persist : Cmd()

    object Quit : Cmd()
}

/** How long a `wait` or `settle` waits when the line does not say. */
private const val DEFAULT_TIMEOUT_MS = 30_000L

internal class ParseError(what: String) : Exception(what)

internal fun parse(line: String): Cmd {
    val v =
        try {
            Json.decode(line)
        } catch (e: JsonError) {
            throw ParseError("not JSON in the dialect: ${e.why} at byte ${e.at}")
        }
    val m = (v as? Value.VStruct)?.fields ?: throw ParseError("a command is an object")
    fun text(k: String): String =
        when (val x = m[k]) {
            is Value.VText -> x.value
            null -> throw ParseError("`$k` is missing")
            else -> throw ParseError("`$k` is text")
        }
    fun int(k: String): Long? =
        when (val x = m[k]) {
            is Value.VInt -> x.value
            null -> null
            else -> throw ParseError("`$k` is an integer")
        }
    fun args(): Map<String, Value> =
        when (val x = m["args"]) {
            is Value.VStruct -> x.fields
            null -> emptyMap()
            else -> throw ParseError("`args` is an object")
        }
    fun timeout(): Long = int("timeout_ms")?.let { maxOf(it, 0) } ?: DEFAULT_TIMEOUT_MS
    return when (val c = text("cmd")) {
        "mutate" -> Cmd.Mutate(text("name"), args())
        "status" -> Cmd.Status
        "hash" -> Cmd.HashCmd
        "wait" -> Cmd.Wait(int("cursor") ?: throw ParseError("`cursor` is missing"), timeout())
        "settle" -> Cmd.Settle(timeout())
        "query" -> Cmd.Query(text("name"), args())
        "rejections" -> Cmd.Rejections(m["all"] == Value.VBool(true))
        "standing" ->
            when (val x = m["id"]) {
                is Value.VId -> Cmd.StandingOf(x.value)
                else -> throw ParseError("`id` is an id: {\"\$id\":\"8-4-4-4-12\"}")
            }
        "disconnect" -> Cmd.Disconnect
        "reconnect" -> Cmd.Reconnect
        "sign_in" -> Cmd.SignIn(text("user"))
        "sign_out" -> Cmd.SignOut
        "join" ->
            Cmd.Join(
                text("server").trimEnd('/'),
                when (m["user"]) {
                    null,
                    Value.VNull -> null
                    else -> text("user")
                })
        "leave" -> Cmd.Leave
        "call" ->
            Cmd.Call(
                text("live"),
                text("transition"),
                when (val x = m["input"]) {
                    is Value.VStruct -> x.fields
                    null -> emptyMap()
                    else -> throw ParseError("`input` is an object")
                })
        "live" -> Cmd.Live(text("name"))
        "live_refusals" -> Cmd.LiveRefusals
        "persist" -> Cmd.Persist
        "quit" -> Cmd.Quit
        else -> throw ParseError("no command ${Json.quoted(c)}")
    }
}

/** An answer, a field at a time, each already printed: `{"k":v,…}` in the order written. */
internal class Answer {
    private val fields = ArrayList<Pair<String, String>>()

    fun raw(k: String, v: String): Answer = apply { fields.add(k to v) }

    fun num(k: String, n: Long): Answer = raw(k, n.toString())

    fun text(k: String, t: String): Answer = raw(k, Json.quoted(t))

    fun value(k: String, v: Value): Answer = raw(k, Json.json(v))

    fun line(): String = fields.joinToString(",", "{", "}") { (k, v) -> "${Json.quoted(k)}:$v" }

    companion object {
        fun ok(ok: Boolean): Answer = Answer().raw("ok", ok.toString())

        fun refused(why: String): Answer = ok(false).text("why", why)
    }
}

// -- the peer ----------------------------------------------------------------------------

internal class Headless(f: Flags, domain: Domain) {
    val dir: File = f.dir
    var server: String? = f.server
    private val patience = f.patience
    /** How long `settle` waits with no wake before the cursor counts as still for a turn. */
    private val stillMs: Long = f.pumpMs
    /** Where the socket's wakes and stdin's lines arrive, in one queue: [WAKE], a line, [END]. */
    val events = LinkedBlockingQueue<Any>()
    /** Lines that arrived while `wait` or `settle` was waiting on a wake: answered after it. */
    private val held = ArrayDeque<Any>()
    /** Verdicts collected and not yet asked for; each also appended to `DIR/rejections.jsonl`. */
    private val unasked = ArrayList<String>()
    /** `--roles`: what this device's `Ctx` carries beside its login's. */
    private val roles = f.roles
    val peer: Peer

    init {
        if (!dir.isDirectory && !dir.mkdirs()) throw PeerError("$dir: cannot make it")
        var loginRoles = emptyList<String>()
        val opts =
            if (f.alone) Options.alone(f.user ?: "me")
            else {
                val s = f.server!!
                val remembered =
                    recall(dir, s)?.takeIf { l -> f.user == null || f.user == l.user.id }
                val login =
                    remembered
                        ?: f.user?.let { u ->
                            val l =
                                try {
                                    logIn(s, u, patience)
                                } catch (e: AuthError) {
                                    throw PeerError("signing $u in at $s: ${e.message}")
                                }
                            remember(dir, s, l)
                            l
                        }
                if (login != null) {
                    loginRoles = login.user.roles
                    Options.server(login.user.id, login.session, login.token)
                } else Options.signedOut()
            }
        peer =
            Peer.open(
                domain,
                DirStorage(File(dir, "replica")),
                opts.withTiming(f.timing).let { o -> f.device?.let(o::withDevice) ?: o })
        peer.setRoles(loginRoles + roles)
        // Before the link exists, so it and every one after hands this to its socket.
        peer.setWaker { events.put(WAKE) }
        f.server?.let { peer.connect(Auth.socketUrl(it)) }
    }

    /** One turn, with whatever it came to said on stderr. */
    fun pump() {
        val p = peer.pump()
        if (p.opened) say("linked (epoch ${peer.epoch}, cursor ${peer.cursor})")
        p.dropped?.let { say("dropped: $it") }
        p.denied?.let { say("denied: $it") }
        p.note?.let { say(it) }
        collect()
    }

    /** Take the verdicts that arrived, and write each down. */
    private fun collect() {
        val fresh =
            peer.takeRejections().map { r ->
                Answer().value("id", Value.VId(r.id)).text("reason", r.reason).line()
            }
        if (fresh.isEmpty()) return
        val file = File(dir, "rejections.jsonl")
        try {
            file.appendText(fresh.joinToString("\n") + "\n", Charsets.UTF_8)
        } catch (e: java.io.IOException) {
            say("$file: ${e.message}")
        }
        unasked.addAll(fresh)
    }

    /**
     * Wait until `done`, or the timeout, pumping on every wake and at the peer's deadline and at no
     * other time — so a frame the socket did not wake anyone for is a wait that never ends, which
     * is the point. With `stepMs`, that long passing with no wake is a turn `done` may count; it is
     * looked at and not pumped. Whether it was done.
     */
    private fun pumpUntil(timeoutMs: Long, stepMs: Long?, done: (Peer) -> Boolean): Boolean {
        val deadline = System.nanoTime() + timeoutMs * 1_000_000
        while (true) {
            if (done(peer)) return true
            val left = (deadline - System.nanoTime()).coerceAtLeast(0) / 1_000_000
            if (left <= 0) return false
            val due = untilDue()
            var wait = minOf(left, due ?: left)
            if (stepMs != null) wait = minOf(wait, stepMs)
            when (val got = events.poll(wait, TimeUnit.MILLISECONDS)) {
                WAKE -> pump()
                null -> if (due != null && due <= wait) pump()
                else -> held.addLast(got)
            }
        }
    }

    /** How long until the peer's deadline, in milliseconds, if it has one. */
    private fun untilDue(): Long? = peer.deadline()?.let { maxOf(it - Link.monotonicMs(), 0) }

    /**
     * The next thing to answer: a line held over from a wait, or whatever wakes the loop — pumping
     * at the peer's deadline on the way.
     */
    fun next(): Any {
        held.removeFirstOrNull()?.let {
            return it
        }
        while (true) {
            val due = untilDue()
            val got = if (due != null) events.poll(due, TimeUnit.MILLISECONDS) else events.take()
            if (got != null) return got
            pump()
        }
    }

    private fun status(): Answer {
        val s = peer.status()
        return Answer()
            .num("cursor", s.cursor)
            .num("pending", s.pending.toLong())
            .num("held", s.held.toLong())
            .raw("behind", s.behind.toString())
            .raw("linked", s.linked.toString())
            .raw("denied", s.denied?.let(Json::quoted) ?: "null")
            .text("user", s.user)
            .text("session", s.session)
            .raw("signed_out", s.signedOut.toString())
            .num("epoch", s.epoch)
            .text("link", s.link)
            .num("opens", s.opens)
            .num("diverged", s.diverged.toLong())
            .num("joining", s.joining.toLong())
            .num("fork", s.fork.cursor)
    }

    private fun why(e: Exception): String = e.message ?: e.javaClass.simpleName

    /** Answer one command. `quit` is answered like `persist`; the loop is what stops. */
    fun handle(cmd: Cmd): Answer =
        when (cmd) {
            is Cmd.Mutate ->
                try {
                    Answer.ok(true).value("id", Value.VId(peer.mutate(cmd.name, cmd.args)))
                } catch (e: Exception) {
                    Answer.refused(why(e))
                }
            is Cmd.Status -> status()
            is Cmd.HashCmd -> {
                val r = peer.replica
                Answer()
                    .num("cursor", r.cursor)
                    .text("hash", Hex.encode(Hash.stateHash(r.confirmed)))
                    .text("view", Hex.encode(Hash.stateHash(r.view)))
            }
            is Cmd.Wait -> {
                val ok = pumpUntil(cmd.timeoutMs, null) { it.cursor >= cmd.cursor }
                Answer.ok(ok).num("cursor", peer.cursor)
            }
            is Cmd.Settle -> {
                // Linked, nothing pending, and the cursor still for two
                // turns — a turn being a wake, or `--pump-ms` with none:
                // what "caught up" can mean from this end alone.
                var last = -1L
                var still = 0
                val ok =
                    pumpUntil(cmd.timeoutMs, stillMs) { p ->
                        if (p.cursor == last) still++
                        else {
                            last = p.cursor
                            still = 0
                        }
                        p.linked && p.pendingLen == 0 && still >= 2
                    }
                Answer.ok(ok).num("cursor", peer.cursor).num("pending", peer.pendingLen.toLong())
            }
            is Cmd.Query ->
                try {
                    Answer().value("rows", peer.query(cmd.name, cmd.args))
                } catch (e: Exception) {
                    Answer.refused(why(e))
                }
            is Cmd.Rejections -> {
                collect()
                val asked = ArrayList(unasked)
                unasked.clear()
                val items =
                    if (cmd.all)
                        File(dir, "rejections.jsonl")
                            .let { if (it.isFile) it.readLines(Charsets.UTF_8) else emptyList() }
                            .filter { it.isNotEmpty() }
                    else asked
                Answer().raw("rejections", Json.array(items))
            }
            is Cmd.StandingOf ->
                when (val s = peer.standing(cmd.id)) {
                    is Standing.Pending -> Answer().text("standing", "pending")
                    is Standing.Confirmed -> Answer().text("standing", "confirmed")
                    is Standing.Rejected -> Answer().text("standing", "rejected").text("why", s.why)
                    is Standing.Unknown -> Answer().text("standing", "unknown")
                }
            is Cmd.Disconnect -> {
                peer.disconnect()
                Answer.ok(true)
            }
            is Cmd.Reconnect -> {
                peer.reconnect()
                Answer.ok(true)
            }
            is Cmd.SignIn -> signIn(cmd.user)
            is Cmd.SignOut -> {
                peer.signOut()
                forget(dir)
                Answer.ok(true)
            }
            is Cmd.Join -> join(cmd.server, cmd.user)
            is Cmd.Leave ->
                try {
                    peer.leave()
                    server = null
                    Answer.ok(true).num("cursor", peer.cursor)
                } catch (e: Exception) {
                    Answer.refused(why(e))
                }
            is Cmd.Call ->
                try {
                    val kind = peer.call(cmd.live, cmd.transition, cmd.input)
                    Answer.ok(true)
                        .text("kind", if (kind == Kind.Ordered) "ordered" else "ephemeral")
                } catch (e: Exception) {
                    Answer.refused(why(e))
                }
            is Cmd.Live -> {
                val st = peer.liveStatus(cmd.name)
                Answer()
                    .raw("value", peer.live(cmd.name)?.let(Json::json) ?: "null")
                    .text("device", peer.ctx.session)
                    .num("seq", st?.seq ?: 0)
                    .num("pending", (st?.pending ?: 0).toLong())
                    .raw("synced", (st?.synced ?: false).toString())
            }
            is Cmd.LiveRefusals ->
                Answer()
                    .raw(
                        "refusals",
                        Json.array(peer.takeLiveRefusals().map { Json.quoted(it.reason) }))
            is Cmd.Persist,
            is Cmd.Quit -> {
                pump()
                try {
                    peer.persist()
                    Answer.ok(true)
                } catch (e: Exception) {
                    Answer.refused(why(e))
                }
            }
        }

    private fun signIn(user: String): Answer {
        val s = server ?: return Answer.refused("a peer alone has no server to sign in to")
        val login =
            try {
                logIn(s, user, patience)
            } catch (e: AuthError) {
                return Answer.refused("signing $user in at $s: ${e.message}")
            }
        try {
            remember(dir, s, login)
        } catch (e: Exception) {
            say("the login is not remembered: ${why(e)}")
        }
        return try {
            peer.signIn(login.user.id, login.session, login.token)
            peer.setRoles(login.user.roles + roles)
            Answer.ok(true).text("user", login.user.id).text("session", login.session)
        } catch (e: Exception) {
            Answer.refused(why(e))
        }
    }

    private fun join(to: String, user: String?): Answer {
        val login =
            user?.let { u ->
                try {
                    logIn(to, u, patience)
                } catch (e: AuthError) {
                    return Answer.refused("signing $u in at $to: ${e.message}")
                }
            }
        if (login != null)
            try {
                remember(dir, to, login)
            } catch (e: Exception) {
                say("the login is not remembered: ${why(e)}")
            }
        val took = System.nanoTime()
        return try {
            peer.join(Auth.socketUrl(to), login)
            peer.setRoles((login?.user?.roles ?: emptyList()) + roles)
            say(
                "joined $to: ${peer.status().joining} re-queued in ${(System.nanoTime() - took) / 1_000_000}ms")
            server = to
            Answer.ok(true).num("pending", peer.pendingLen.toLong()).num("cursor", peer.cursor)
        } catch (e: Exception) {
            Answer.refused(why(e))
        }
    }
}

/** The module: `--module FILE`, or harken's own, as this program's jar carries it. */
internal fun domain(module: File?): Domain {
    val bytes =
        if (module != null) module.readBytes()
        else
            Headless::class.java.getResourceAsStream("/harken.ark")?.use { it.readBytes() }
                ?: throw PeerError("this build carries no harken.ark")
    return try {
        Domain.fromBytes(bytes)
    } catch (e: Exception) {
        throw PeerError(
            "the module ${module ?: "harken.ark"}: ${e.message ?: e.javaClass.simpleName}")
    }
}

/** The socket asked for a pump. */
private val WAKE = Any()

/** The end of stdin, which is a quit. */
private val END = Any()

/**
 * Every line of stdin, on a thread of its own, into the queue the socket's wakes arrive in, then
 * [END].
 */
private fun lines(q: LinkedBlockingQueue<Any>) {
    val t = Thread {
        val r = System.`in`.bufferedReader(Charsets.UTF_8)
        while (true) {
            val l =
                try {
                    r.readLine()
                } catch (e: java.io.IOException) {
                    null
                } ?: break
            q.put(l)
        }
        q.put(END)
    }
    t.isDaemon = true
    t.start()
}

/** What only reads: answered from what the peer has, pumping nothing. */
private fun reads(cmd: Cmd): Boolean =
    when (cmd) {
        is Cmd.Status,
        is Cmd.HashCmd,
        is Cmd.Query,
        is Cmd.Rejections,
        is Cmd.StandingOf,
        is Cmd.Wait,
        is Cmd.Settle,
        is Cmd.Live,
        is Cmd.LiveRefusals -> true
        else -> false
    }

private fun run(f: Flags) {
    val h = Headless(f, domain(f.module))
    lines(h.events)
    val out =
        java.io.PrintStream(java.io.FileOutputStream(java.io.FileDescriptor.out), false, "UTF-8")
    // One queue, so a driver asking `status` every ten milliseconds is a peer
    // that still pumps on every wake between its lines: the socket's wakes and
    // the lines are answered in the order they arrived.
    h.pump()
    while (true) {
        val got = h.next()
        if (got === WAKE) {
            h.pump()
            continue
        }
        if (got === END) {
            h.handle(Cmd.Quit)
            h.peer.close()
            return
        }
        val line = got as String
        if (line.isBlank()) continue
        var quit = false
        var read = true
        val answer =
            try {
                val cmd = parse(line)
                quit = cmd == Cmd.Quit
                read = reads(cmd)
                h.handle(cmd)
            } catch (e: ParseError) {
                Answer.refused(e.message ?: "not a command")
            }
        out.println(answer.line())
        out.flush()
        // A reader that has gone is a driver that has gone: stop, with what is
        // pending written down.
        if (out.checkError() || quit) {
            h.peer.close()
            return
        }
        // What a command asked of the peer goes now — the engine's frames to
        // the socket, the store to disk — as an app's own call is a wake. A
        // read asks nothing, so it pumps nothing: a driver asking `status` in
        // a loop must not be what drains the socket.
        if (!read) h.pump()
    }
}

public fun main(args: Array<String>) {
    val f =
        try {
            flags(args.toList())
        } catch (e: FlagError) {
            say(e.message ?: "")
            exitProcess(2)
        }
    try {
        run(f)
    } catch (e: Exception) {
        say(e.message ?: e.javaClass.simpleName)
        exitProcess(1)
    }
    exitProcess(0)
}
