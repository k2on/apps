// A session: the replica of a module's log, the client machine over it,
// the link to a server when there is one and an authority of its own when
// there is not (docs/arkdb.md §3.10), and the file the replica survives a
// restart in. An app makes one, pumps it on a timer, and asks it to mutate
// and to query — and, for anything it authored, what became of it.
//
// Single-threaded by design, like the engine: every method is to be called
// from one thread, the one that calls `pump()`; the link marshals what a
// socket says onto it.
package dev.arkdb.client

import dev.arkdb.Args
import dev.arkdb.Authority
import dev.arkdb.Auto
import dev.arkdb.Canon
import dev.arkdb.Changes
import dev.arkdb.Client
import dev.arkdb.Closure
import dev.arkdb.Ctx
import dev.arkdb.Decode
import dev.arkdb.Entry
import dev.arkdb.Eval
import dev.arkdb.Fault
import dev.arkdb.FnHash
import dev.arkdb.FnKind
import dev.arkdb.Hash
import dev.arkdb.Hex
import dev.arkdb.Id
import dev.arkdb.MemoryStore
import dev.arkdb.Mode
import dev.arkdb.Module
import dev.arkdb.Procedure
import dev.arkdb.Replica
import dev.arkdb.Seq
import dev.arkdb.Store
import dev.arkdb.Value
import java.io.File
import java.security.SecureRandom

/** Where a mutation's non-determinism comes from, drawn once at the originating peer and frozen in the log. */
public interface AutoSource {
    /** A fresh id for a `NewId` auto naming `table`. */
    public fun newId(table: String): Id

    /** The clock, for a `Now` auto, in milliseconds. */
    public fun now(): Long

    /** The entry's own id. */
    public fun entryId(): Id = newId("")

    public object Default : AutoSource {
        private val random = SecureRandom()

        override fun newId(table: String): Id = Id(ByteArray(16).also { random.nextBytes(it) })

        override fun now(): Long = System.currentTimeMillis()
    }
}

/** What authoring an intent came to. */
public sealed class Outcome {
    public data class Applied(val entry: Entry) : Outcome()
    public data class Refused(val reason: String) : Outcome()
}

/** One mutation as it is being authored: its function and hash, and the autos drawn for it. */
private class Mutation(
    val name: String,
    val hash: FnHash,
    val autos: Args,
)

/**
 * Where an intent this peer authored stands: what a screen draws beside the
 * item it made. `Rejected.reason` is the sentence the authority gave
 * (`Protocol.refusalText`), or this peer's own when its replay refused it.
 */
public sealed class ItemState {
    /** Authored here and not yet answered: on the view, not yet in the log. */
    public object Pending : ItemState() {
        override fun toString(): String = "Pending"
    }

    /** In the log, at `seq`. */
    public data class Confirmed(val seq: Seq) : ItemState()

    /** It did not happen, and why. */
    public data class Rejected(val reason: String) : ItemState()

    /** Not an intent this peer authored (or one it has forgotten). */
    public object Unknown : ItemState() {
        override fun toString(): String = "Unknown"
    }
}

public data class Status(
    /** The socket is open and the machine has said hello. */
    val linked: Boolean,
    /** No server: this peer sequences its own log. */
    val serverless: Boolean,
    /** The server turned this peer away; it has stopped reconnecting. */
    val denied: String?,
    /** The last sequence applied. */
    val cursor: Seq,
    /** Intents authored here that no verdict has answered. */
    val pending: Int,
    /** Intents authored here that were rejected, ever (see `Session.statusOf`). */
    val rejected: Int,
    /** Sequences at which this peer's replay disagreed with the authority's facts. */
    val diverged: Int,
    val retryInMs: Long?,
    val lastClose: String?,
)

public class Session private constructor(
    public val dir: File,
    public val module: Module,
    user: String?,
    public val serverUrl: String?,
    private val autoSource: AutoSource,
    transport: Transport?,
    clock: () -> Long,
    procedures: List<Pair<FnHash, Procedure>>,
    login: String?,
    token: String?,
) {
    public val schema = module.schema

    /** Every function of the module, by the hash an entry names it by. */
    public val bodies: Map<FnHash, Closure> = Hash.closures(module)

    /**
     * The procedures this peer runs natively (AUTHORING.md §3), by the same
     * hashes: what it authors and what it replays is applied by them; the
     * interpreter runs only what arrives with no native procedure.
     */
    public val natives: Map<FnHash, Procedure> = procedures.toMap()

    private val byName: Map<String, Pair<FnHash, Closure>> = bodies.entries.associate { (h, c) -> c.fn.name to (h to c) }

    /** The device this is: one id per directory, kept across opens. */
    public val sessionId: String

    /** Who authors, and under which login: the entry's actor and session; `Ctx.nobody` until somebody signs in. */
    public var ctx: Ctx
        private set

    /** Who authors: the user signed in, or empty while nobody is. */
    public val user: String get() = ctx.user

    /** Whether somebody is signed in; until then everything authored is nobody's, pending, and said to no server. */
    public val signedIn: Boolean get() = !ctx.isNobody

    public val client: Client

    /** The link to the server, or null when working alone. */
    public val link: Link?

    /** The authority this peer runs for itself, when there is no server. */
    public val authority: Authority?

    /** The replica of the log (a snapshot from the server replaces it, so it is read through the client). */
    public val replica: Replica get() = client.replica

    private var listener: ((Changes) -> Unit)? = null

    private var persisted: Pair<Seq, List<Id>>? = null

    /** What became of each intent this peer authored, once answered, oldest first. */
    private val verdicts = LinkedHashMap<Id, Verdict>()
    private var verdictsWritten = 0

    /** Rejections not yet taken by `takeRejections`. */
    private val untaken = ArrayList<Pair<Id, String>>()

    init {
        val dev = Durable.deviceFile(dir)
        sessionId = if (dev.isFile) {
            String(Durable.readAll(dev), Charsets.UTF_8).trim()
        } else {
            val id = autoSource.newId("").hex
            Durable.writeAtomically(dev, id.toByteArray(Charsets.UTF_8))
            id
        }
        // Alone, a peer authors under its device; linked, under the login it
        // proves — `"dev"` by default, which is what the spec's `trusting`
        // makes of a token that is a name. Signed out, as nobody.
        if (user == null && serverUrl == null) {
            throw IllegalArgumentException("a session alone is its own authority, and somebody has to be signed in to it")
        }
        ctx = if (user == null) Ctx.nobody else Ctx(user, login ?: if (serverUrl == null) sessionId else "dev")
        val d = Durable.readReplica(schema, dir)
        val r = if (d == null) {
            Replica.open(schema, bodies, MemoryStore(schema), 0, emptyList(), natives)
        } else {
            Replica.open(schema, bodies, d.confirmed, d.cursor, d.pending, natives)
        }
        if (d != null) {
            for ((i, v) in d.verdicts) verdicts[i] = v
            verdictsWritten = verdicts.size
            persisted = d.cursor to d.pending.map { it.id }
        }
        client = Client(r, Mode.Whole, null)
        // What was authored before anyone signed in, on this device, is the
        // signer's (`Client.signIn`); signed out, nothing is said to a server.
        if (serverUrl != null && user != null) client.signIn(ctx, token ?: user)
        authority = if (serverUrl != null) {
            null
        } else {
            // Alone: the authority is rebuilt by adopting the log this peer
            // kept, which replays every intent and checks the hash — so a
            // store that does not match the log it claims is refused here.
            val a = if (d?.log == null) {
                Authority(schema, bodies, natives)
            } else {
                Authority.adopt(schema, bodies, d.asLog(schema), natives)
            }
            if (!Hash.stateHash(a.store).contentEquals(Hash.stateHash(r.confirmed))) {
                throw IllegalStateException("the durable store is not the state of its own log")
            }
            a
        }
        link = if (serverUrl == null) {
            null
        } else {
            Link(client, transport ?: WebSocketTransport(serverUrl), clock).also { if (signedIn) it.connect() else it.disconnect() }
        }
        persist()
    }

    public companion object {
        /**
         * Open (or create) a session in `dir`. With a `serverUrl` the peer is
         * a replica of that server's log; without one it is its own
         * authority. `transport` replaces the WebSocket, for a test.
         * `procedures` are run natively; every other function of `module`
         * by the interpreter. `login` is the session entries are authored
         * under and `token` what the `Hello` proves it with; by default the
         * token is `user` and the login `"dev"`, which is dev auth. A null
         * `user` (with a `serverUrl`) opens it signed out: everything is
         * authored as `Ctx.nobody`, kept pending and written down, and no
         * connection is made until `signIn`.
         */
        public fun open(
            dir: File,
            module: Module,
            user: String?,
            serverUrl: String? = null,
            autos: AutoSource = AutoSource.Default,
            transport: Transport? = null,
            clock: () -> Long = System::currentTimeMillis,
            procedures: List<Pair<FnHash, Procedure>> = emptyList(),
            login: String? = null,
            token: String? = null,
        ): Session {
            dir.mkdirs()
            return Session(dir, module, user, serverUrl, autos, transport, clock, procedures, login, token)
        }

        /** Open a session over an authored domain: its emitted module, and every procedure of it run natively. */
        public fun open(
            dir: File,
            domain: dev.arkdb.authoring.Module,
            user: String?,
            serverUrl: String? = null,
            autos: AutoSource = AutoSource.Default,
            transport: Transport? = null,
            clock: () -> Long = System::currentTimeMillis,
            login: String? = null,
            token: String? = null,
        ): Session = open(dir, domain.ir, user, serverUrl, autos, transport, clock, domain.procedures(), login, token)

        /** The module from its canonical bytes, as `MODULE_BYTES` carries them. */
        public fun moduleOf(bytes: ByteArray): Module = Decode.fromValue(Canon.decode(bytes))

        public fun moduleOfHex(hex: String): Module = moduleOf(Hex.decode(hex))
    }

    /**
     * Somebody signs in: `user` under the login `login`, proven with `token`
     * on the next connection, which starts now. Everything authored while
     * nobody was signed in becomes theirs (`Client.signIn`: re-stamped, the
     * view replayed) and is written down before anything is said; the
     * first `Hello` pushes it. Intents pending under an older login of
     * anybody keep it: a server that knows the user owns it
     * (`Server.withOwns`) accepts them, and one that does not rejects them
     * "not yours" — which `statusOf` then says.
     */
    public fun signIn(user: String, login: String = "dev", token: String? = user) {
        require(user.isNotEmpty()) { "somebody signs in: the user is not empty" }
        ctx = Ctx(user, login)
        client.signIn(ctx, token)
        settle()
        persist()
        link?.let {
            it.disconnect()
            it.connect()
        }
    }

    // Authoring ---------------------------------------------------------------

    private fun prepare(name: String): Mutation {
        val (h, c) = byName[name] ?: throw IllegalArgumentException("no function $name in the module")
        val fn = c.fn
        if (fn.kind != FnKind.Mutator) throw IllegalArgumentException("$name is not a mutator")
        val autos = LinkedHashMap<String, Value>()
        for ((n, a) in fn.autos) {
            autos[n] = when (a) {
                is Auto.NewId -> Value.id(autoSource.newId(a.table))
                is Auto.Now -> Value.int(autoSource.now())
            }
        }
        return Mutation(name, h, autos)
    }

    /**
     * Author an intent: the procedure `name`, natively when this session
     * holds it and through the interpreter otherwise — the entry recorded is
     * the same either way. `Applied` is on the view and pending; ask
     * `statusOf(entry.id)` later for what the log made of it.
     */
    public fun mutate(name: String, args: Args): Outcome {
        val m = prepare(name)
        val entry = try {
            client.mutate(autoSource.entryId(), ctx, m.hash, m.autos, args)
        } catch (f: Fault.Refuse) {
            return Outcome.Refused(f.refusal.text)
        }
        val auth = authority
        if (auth != null) Authority.localCommit(auth, replica)
        persistIfMoved()
        settle()
        // Alone, the authority's verdict is immediate; a later one a server
        // gives arrives through `statusOf`.
        val v = verdicts[entry.id]
        if (auth != null && v is Verdict.Rejected) return Outcome.Refused(v.reason)
        return Outcome.Applied(entry)
    }

    /**
     * §1.3 (AUTHORING.md) The form validator: `name`'s input checks over
     * whatever of the input a form has so far, against the store queries
     * read. Each failing field's message, and the normalised values.
     */
    public fun check(name: String, partial: Args): Eval.Checked {
        val (_, c) = byName[name] ?: throw IllegalArgumentException("no function $name in the module")
        return Eval.check(schema, c, partial, store(), ctx)
    }

    // Reading -----------------------------------------------------------------

    /** The store a query reads: the replica's optimistic view. */
    public fun store(): MemoryStore = replica.view

    /**
     * Ask a query of the module by name, as this peer's user: natively when
     * held, through the interpreter otherwise. A check or a middleware may
     * refuse it.
     */
    public fun ask(name: String, args: Args = emptyMap()): Eval.Answer {
        val (h, c) = byName[name] ?: throw IllegalArgumentException("no function $name in the module")
        if (c.fn.kind != FnKind.Query) throw IllegalArgumentException("$name is not a query")
        return natives[h]?.query(schema, ctx, args, store()) ?: Eval.queryClosure(schema, c, ctx, args, store())
    }

    /** `ask`, with a refusal thrown as `Fault.Refuse`. */
    public fun query(name: String, args: Args = emptyMap()): Value = ask(name, args).orThrow()

    /** Read the view directly. */
    public fun <T> read(f: (Store) -> T): T = f(store())

    // The loop ----------------------------------------------------------------

    /** Every 50 ms or so, from the caller's timer: the link's turn, then what moved. */
    public fun pump() {
        val l = link ?: return
        l.pump()
        persistIfMoved()
        settle()
    }

    /** Called after every mutation and every frame that changed the view, with what the replica reports. */
    public fun onChange(f: ((Changes) -> Unit)?) {
        listener = f
    }

    /** What the replica reports since it was last asked, or null when nothing moved; also delivered to the listener. */
    public fun takeChanges(): Changes? {
        val ch = replica.takeChanges()
        val moved = ch is Changes.Rebuilt || (ch is Changes.Applied && ch.changes.isNotEmpty())
        return if (moved) ch else null
    }

    // Every turn drains the replica, listener or not: what nobody is
    // listening for is dropped, so a listener set later starts from the
    // store as it stands rather than from a stale `Rebuilt`.
    private fun settle() {
        collectVerdicts()
        val ch = takeChanges()
        val f = listener ?: return
        if (ch != null) f(ch)
    }

    // What the replica learnt about this peer's own intents since it was
    // last asked: kept by id, so a screen can ask about any one of them.
    private fun collectVerdicts() {
        val r = replica
        if (r.confirmedOwn.isEmpty() && r.rejections.isEmpty()) return
        for ((i, n) in r.confirmedOwn) verdicts[i] = Verdict.Confirmed(n)
        for ((i, why) in r.rejections) {
            verdicts[i] = Verdict.Rejected(why.text)
            untaken.add(i to why.text)
        }
        r.confirmedOwn.clear()
        r.rejections.clear()
        persist()
    }

    // Verifying ---------------------------------------------------------------

    /**
     * Ask whether the authority agrees with the replica's confirmed state.
     * Alone, the answer is immediate; linked, it is asked of the server,
     * null is returned, and the answer arrives in `takeAgreed()`.
     */
    public fun verify(): Pair<Seq, Boolean>? {
        val a = authority
        if (a == null) {
            client.verifyAll()
            return null
        }
        val (n, h) = replica.verifyAt()
        return n to (a.log.headSeq == n && Hash.stateHash(a.store).contentEquals(h))
    }

    public fun takeAgreed(): List<Pair<Seq, Boolean>> {
        val out = client.agreed.toList()
        client.agreed.clear()
        return out
    }

    // Status ------------------------------------------------------------------

    public val status: Status
        get() = Status(
            linked = link?.linked ?: false,
            serverless = authority != null,
            denied = client.denied,
            cursor = replica.cursor,
            pending = replica.pending.size,
            rejected = verdicts.values.count { it is Verdict.Rejected },
            diverged = replica.diverged.size,
            retryInMs = link?.retryIn(),
            lastClose = link?.lastClose,
        )

    /**
     * Where an intent this peer authored stands, by its entry id: pending,
     * confirmed at a sequence, or rejected with the reason — the
     * authority's sentence, word for word. Kept across restarts.
     */
    public fun statusOf(id: Id): ItemState {
        collectVerdicts()
        if (replica.pending.any { it.id == id }) return ItemState.Pending
        return when (val v = verdicts[id]) {
            is Verdict.Confirmed -> ItemState.Confirmed(v.seq)
            is Verdict.Rejected -> ItemState.Rejected(v.reason)
            null -> ItemState.Unknown
        }
    }

    /** Every rejected intent this peer authored, with its reason, oldest first. */
    public fun rejected(): List<Pair<Id, String>> =
        verdicts.entries.mapNotNull { (i, v) -> (v as? Verdict.Rejected)?.let { i to it.reason } }

    /** Rejections since this was last called, newest last, and the slate wiped; `statusOf` still answers for them. */
    public fun takeRejections(): List<Pair<Id, String>> {
        collectVerdicts()
        val out = untaken.toList()
        untaken.clear()
        return out
    }

    /** Stop answering for these intents: a screen that has shown a verdict can let it go. */
    public fun forget(ids: Collection<Id>) {
        var any = false
        for (i in ids) any = verdicts.remove(i) != null || any
        if (any) persist()
    }

    // Durability --------------------------------------------------------------

    private fun durable(): DurableReplica {
        val r = replica
        val log = authority?.log?.entries?.map { (n, ef) -> Triple(n, ef.first, ef.second) }
        return DurableReplica(r.cursor, r.confirmed, r.pending, log, verdicts.entries.map { it.key to it.value })
    }

    private fun persistIfMoved() {
        val r = replica
        val key = r.cursor to r.pending.map { it.id }
        if (persisted == key && verdictsWritten == verdicts.size) return
        persist()
    }

    /** Write the replica now, whether or not it moved. */
    public fun persist() {
        val r = replica
        Durable.writeReplica(dir, durable())
        persisted = r.cursor to r.pending.map { it.id }
        verdictsWritten = verdicts.size
    }

    /** Stop the link and write everything down. */
    public fun close() {
        link?.disconnect()
        collectVerdicts()
        persist()
    }
}
