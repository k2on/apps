// A session: the replicas of a module's scopes, the client machine over
// them, the link to a server when there is one and an authority of its own
// when there is not (docs/arkdb.md §3.10), and the files each replica
// survives a restart in. An app makes one, pumps it on a timer, and asks it
// to mutate and to query.
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

/** One mutation as it is being authored: its function, scope and hash, and the autos drawn for it. */
private class Mutation(
    val name: String,
    val scope: String,
    val hash: FnHash,
    val autos: Args,
)

public data class ScopeStatus(val scope: String, val cursor: Seq, val pending: Int, val rejections: Int, val diverged: Int)

public data class Status(
    /** The socket is open and the machine has said hello. */
    val linked: Boolean,
    /** No server: this peer sequences its own scopes. */
    val serverless: Boolean,
    /** The server turned this peer away; it has stopped reconnecting. */
    val denied: String?,
    val scopes: List<ScopeStatus>,
    val retryInMs: Long?,
    val lastClose: String?,
) {
    val pending: Int get() = scopes.sumOf { it.pending }
}

public class Session private constructor(
    public val dir: File,
    public val module: Module,
    public val user: String,
    public val serverUrl: String?,
    private val autoSource: AutoSource,
    transport: Transport?,
    clock: () -> Long,
    procedures: List<Pair<FnHash, Procedure>>,
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

    public val ctx: Ctx

    public val client: Client

    /** The link to the server, or null when working alone. */
    public val link: Link?

    /** The authorities this peer runs for itself, one per scope, when there is no server. */
    public val authorities: Map<String, Authority>?

    private var listener: ((Map<String, Changes>) -> Unit)? = null

    private val persisted = HashMap<String, Pair<Seq, List<Id>>>()

    private var merged: MemoryStore? = null

    init {
        val dev = Durable.deviceFile(dir)
        sessionId = if (dev.isFile) {
            String(Durable.readAll(dev), Charsets.UTF_8).trim()
        } else {
            val id = autoSource.newId("").hex
            Durable.writeAtomically(dev, id.toByteArray(Charsets.UTF_8))
            id
        }
        ctx = Ctx(user, sessionId)
        client = Client(schema, user)
        val auths = if (serverUrl == null) HashMap<String, Authority>() else null
        for (sc in schema.scopes) {
            val d = Durable.readScope(schema, dir, sc.name)
            val r = if (d == null) {
                Replica.open(schema, sc.name, bodies, MemoryStore(schema), 0, emptyList(), natives)
            } else {
                Replica.open(schema, sc.name, bodies, d.confirmed, d.cursor, d.pending, natives)
            }
            client.subscribe(Mode.Whole, r)
            if (auths != null) {
                // Alone: the authority is rebuilt by adopting the log this peer
                // kept, which replays every intent and checks the hash — so a
                // store that does not match the log it claims is refused here.
                val a = if (d?.log == null) {
                    Authority(sc.name, schema, bodies, natives)
                } else {
                    Authority.adopt(schema, sc.name, bodies, d.asLog(schema), natives)
                }
                if (!Hash.stateHash(a.store).contentEquals(Hash.stateHash(r.confirmed))) {
                    throw IllegalStateException("scope ${sc.name}: the durable store is not the state of its own log")
                }
                auths[sc.name] = a
            }
            if (d != null) persisted[sc.name] = d.cursor to d.pending.map { it.id }
        }
        authorities = auths
        link = if (serverUrl == null) {
            null
        } else {
            Link(client, transport ?: WebSocketTransport(serverUrl), clock).also { it.connect() }
        }
        persistAll()
    }

    public companion object {
        /**
         * Open (or create) a session in `dir`. With a `serverUrl` the peer is
         * a replica of that server's scopes; without one it is its own
         * authority. `transport` replaces the WebSocket, for a test.
         * `procedures` are run natively; every other function of `module`
         * by the interpreter.
         */
        public fun open(
            dir: File,
            module: Module,
            user: String,
            serverUrl: String? = null,
            autos: AutoSource = AutoSource.Default,
            transport: Transport? = null,
            clock: () -> Long = System::currentTimeMillis,
            procedures: List<Pair<FnHash, Procedure>> = emptyList(),
        ): Session {
            dir.mkdirs()
            return Session(dir, module, user, serverUrl, autos, transport, clock, procedures)
        }

        /** Open a session over an authored domain: its emitted module, and every procedure of it run natively. */
        public fun open(
            dir: File,
            domain: dev.arkdb.authoring.Module,
            user: String,
            serverUrl: String? = null,
            autos: AutoSource = AutoSource.Default,
            transport: Transport? = null,
            clock: () -> Long = System::currentTimeMillis,
        ): Session = open(dir, domain.ir, user, serverUrl, autos, transport, clock, domain.procedures())

        /** The module from its canonical bytes, as `MODULE_BYTES` carries them. */
        public fun moduleOf(bytes: ByteArray): Module = Decode.fromValue(Canon.decode(bytes))

        public fun moduleOfHex(hex: String): Module = moduleOf(Hex.decode(hex))
    }

    // Authoring ---------------------------------------------------------------

    private fun prepare(name: String): Mutation {
        val (h, c) = byName[name] ?: throw IllegalArgumentException("no function $name in the module")
        val fn = c.fn
        if (fn.kind != FnKind.Mutator) throw IllegalArgumentException("$name is not a mutator")
        val scope = fn.scope ?: throw IllegalArgumentException("$name has no scope")
        val autos = LinkedHashMap<String, Value>()
        for ((n, a) in fn.autos) {
            autos[n] = when (a) {
                is Auto.NewId -> Value.id(autoSource.newId(a.table))
                is Auto.Now -> Value.int(autoSource.now())
            }
        }
        return Mutation(name, scope, h, autos)
    }

    /**
     * Author an intent: the procedure `name`, natively when this session
     * holds it and through the interpreter otherwise — the entry recorded is
     * the same either way.
     */
    public fun mutate(name: String, args: Args): Outcome {
        val m = prepare(name)
        val r = client.replica(m.scope) ?: return Outcome.Refused("not holding scope ${m.scope}")
        val rejectionsBefore = r.rejections.size
        val entry = try {
            client.mutate(m.scope, autoSource.entryId(), ctx, m.hash, m.autos, args)
        } catch (f: Fault.Refuse) {
            return Outcome.Refused(f.refusal.text)
        }
        merged = null
        val auth = authorities?.get(m.scope)
        if (auth != null) Authority.localCommit(auth, r)
        persistIfMoved(m.scope)
        settle()
        // Alone, the authority's verdict is immediate; a later one a server
        // gives arrives through the replica's `rejections`.
        if (auth != null && r.rejections.size > rejectionsBefore) {
            return Outcome.Refused(r.rejections.last().second.text)
        }
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

    /** The store a query reads: every scope's optimistic view, together. */
    public fun store(): MemoryStore {
        merged?.let { return it }
        val views = client.scopes.values.map { it.first.view }
        val m = if (views.size == 1) views[0] else views.drop(1).fold(views[0]) { acc, v -> acc.merge(v) }
        merged = m
        return m
    }

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

    /** Read the merged view directly. */
    public fun <T> read(f: (Store) -> T): T = f(store())

    // The loop ----------------------------------------------------------------

    /** Every 50 ms or so, from the caller's timer: the link's turn, then what moved. */
    public fun pump() {
        val l = link ?: return
        val before = l.received
        l.pump()
        if (l.received != before) merged = null
        for (sc in client.scopes.keys) persistIfMoved(sc)
        settle()
    }

    /** Called after every mutation and every frame that changed a view, with what each scope reports. */
    public fun onChange(f: ((Map<String, Changes>) -> Unit)?) {
        listener = f
    }

    /** What every replica reports since it was last asked; also delivered to the listener. */
    public fun takeChanges(): Map<String, Changes> {
        val out = LinkedHashMap<String, Changes>()
        for ((s, rm) in client.scopes) {
            val ch = rm.first.takeChanges()
            val moved = ch is Changes.Rebuilt || (ch is Changes.Applied && ch.changes.isNotEmpty())
            if (moved) out[s] = ch
        }
        return out
    }

    // Every turn drains the replicas, listener or not: what nobody is
    // listening for is dropped, so a listener set later starts from the
    // store as it stands rather than from a stale `Rebuilt`.
    private fun settle() {
        val ch = takeChanges()
        val f = listener ?: return
        if (ch.isNotEmpty()) f(ch)
    }

    // Verifying ---------------------------------------------------------------

    /**
     * Ask whether the authority agrees with every replica's confirmed state.
     * Alone, the answer is immediate; linked, it is asked of the server and
     * the answers arrive in `takeAgreed()`.
     */
    public fun verify(): List<Triple<String, Seq, Boolean>> {
        val auths = authorities
        if (auths == null) {
            client.verifyAll()
            return emptyList()
        }
        return client.scopes.map { (s, rm) ->
            val (n, h) = rm.first.verifyAt()
            val a = auths.getValue(s)
            Triple(s, n, a.log.headSeq == n && Hash.stateHash(a.store).contentEquals(h))
        }
    }

    public fun takeAgreed(): List<Triple<String, Seq, Boolean>> {
        val out = client.agreed.toList()
        client.agreed.clear()
        return out
    }

    // Status ------------------------------------------------------------------

    public val status: Status
        get() = Status(
            linked = link?.linked ?: false,
            serverless = authorities != null,
            denied = client.denied,
            scopes = client.scopes.map { (s, rm) ->
                val r = rm.first
                ScopeStatus(s, r.cursor, r.pending.size, r.rejections.size, r.diverged.size)
            },
            retryInMs = link?.retryIn(),
            lastClose = link?.lastClose,
        )

    /** Verdicts against this peer's own intents, newest last, and the slate wiped. */
    public fun takeRejections(): List<Pair<Id, String>> {
        val out = ArrayList<Pair<Id, String>>()
        for ((_, rm) in client.scopes) {
            for ((i, why) in rm.first.rejections) out.add(i to why.text)
            rm.first.rejections.clear()
        }
        return out
    }

    // Durability --------------------------------------------------------------

    private fun durable(scope: String): DurableScope {
        val r = client.replica(scope)!!
        val log = authorities?.get(scope)?.log?.entries?.map { (n, ef) -> Triple(n, ef.first, ef.second) }
        return DurableScope(scope, r.cursor, r.confirmed, r.pending, log)
    }

    private fun persistIfMoved(scope: String) {
        val r = client.replica(scope) ?: return
        val key = r.cursor to r.pending.map { it.id }
        if (persisted[scope] == key) return
        Durable.writeScope(dir, durable(scope))
        persisted[scope] = key
    }

    /** Write every scope now, whether or not it moved. */
    public fun persistAll() {
        for (sc in client.scopes.keys) {
            Durable.writeScope(dir, durable(sc))
            val r = client.replica(sc)!!
            persisted[sc] = r.cursor to r.pending.map { it.id }
        }
    }

    /** Stop the link and write everything down. */
    public fun close() {
        link?.disconnect()
        persistAll()
    }
}
