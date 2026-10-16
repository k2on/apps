// A peer on ArkDB, as `rust/ark-client/src/peer.rs` is one: the engine's
// `Client` over its `Replica`, kept durable on a `Storage` record for record
// as the reference keeps it, and — alone — its own `Authority` over a local
// history kept in `ark::journal`'s shape. What it adds over the sans-io
// client is a driver's: ids and times drawn (`Autos`), who authors (a
// login, or nobody), what is written when, and going alone and back.
//
// The socket is a `Link` (`Link.kt`, the reference's `ark_client::link`):
// `connect` makes one, and every `pump` dials if due, hands the engine what
// arrived, settles once, sends what it queued and writes what moved. The
// sans-io half is still here for a transport of the caller's own:
// `connected`, `recvFrame` and `takeOutgoingFrames`.
//
// **No loop** (`docs/plan-reactive.md`). A host hands the peer a `Wake`
// ([Peer.setWaker]) and pumps when it is rung — the socket opened, a frame
// arrived, it closed — after anything it asks of the peer, and at
// [Peer.deadline], the one timer there is (the redial after a close). What a
// pump moved is in its [Pumped], so a host publishes nothing when nothing
// did. Connected and idle, nothing runs.
package dev.arkdb.client

import dev.arkdb.Authority
import dev.arkdb.Canon
import dev.arkdb.Changes
import dev.arkdb.Client
import dev.arkdb.ClientMsg
import dev.arkdb.Closure
import dev.arkdb.Ctx
import dev.arkdb.Decode
import dev.arkdb.DeviceDecl
import dev.arkdb.FactList
import dev.arkdb.FnKind
import dev.arkdb.Function
import dev.arkdb.Hash
import dev.arkdb.Id
import dev.arkdb.Instance
import dev.arkdb.Journal
import dev.arkdb.Kind
import dev.arkdb.Layout
import dev.arkdb.LiveRefusal
import dev.arkdb.Log
import dev.arkdb.LogEntry
import dev.arkdb.LogRecords
import dev.arkdb.MemoryStore
import dev.arkdb.Mode
import dev.arkdb.Module
import dev.arkdb.Received
import dev.arkdb.Refusal
import dev.arkdb.Replica
import dev.arkdb.Runner
import dev.arkdb.Schema
import dev.arkdb.Seq
import dev.arkdb.ServerMsg
import dev.arkdb.Value
import dev.arkdb.Verify
import dev.arkdb.Watch
import dev.arkdb.localCommit
import dev.arkdb.refusalText
import dev.arkdb.stripModule

/** What a peer cannot do, or found wrong (`ark_client::Error`). */
public open class PeerError(what: String) : Exception(what)

/** A verdict: nothing changed (`Error::Refused`). */
public class Refused(public val refusal: Refusal) : PeerError(refusalText(refusal))

/**
 * The module a peer runs (`ark_client::Domain`): its closures by hash, and its hash as a client's
 * module has it — every private block stripped, so a server built from the blocks and a phone built
 * from `harken.ark` are one module to each other.
 */
public class Domain(public val module: Module) {
    /** Every function's closure, by its hash as hex. */
    public val closures: Map<String, Closure> = Hash.closures(module)
    private val byName: Map<String, String> =
        closures.entries.associate { (h, c) -> c.function.name to h }

    /** `Domain::hash`: the module hash of the module stripped of private blocks. */
    public val hash: ByteArray = Hash.moduleHash(stripModule(module))

    /** `Domain::live`: the module's live states, interpreted (`docs/plan-live.md`). */
    public val live: Runner = Runner.ofModule(module)

    /** A function by name: its hash and its definition. */
    public fun function(name: String): Pair<ByteArray, Function>? =
        byName[name]?.let { h -> dev.arkdb.Hex.decode(h) to closures[h]!!.function }

    /** `Domain::query`. */
    public fun query(name: String): Pair<ByteArray, Function> {
        val (h, f) = function(name) ?: throw PeerError("the module has no function $name")
        if (f.kind != FnKind.Query) throw PeerError("$name is not a query")
        return h to f
    }

    /** `Domain::mutator`. */
    public fun mutator(name: String): Pair<ByteArray, Function> {
        val (h, f) = function(name) ?: throw PeerError("the module has no function $name")
        if (f.kind != FnKind.Mutator) throw PeerError("$name is not a mutator")
        return h to f
    }

    public companion object {
        /** `Domain::from_bytes`: the canonical bytes of an `.ark` file, verified. */
        public fun fromBytes(bytes: ByteArray): Domain =
            Domain(Verify.verify(Decode.module(Canon.decode(bytes))))

        /**
         * Bytes [fromBytes] verified before: the same module — decoded, its orders completed and
         * its functions normalized as `verify` does — without the checks, whose answer is a
         * function of the bytes. Only for bytes known to have passed; an app remembers which
         * (`dev.arkdb.android.ModuleMemory`, by the bytes' SHA-256).
         */
        public fun fromVerifiedBytes(bytes: ByteArray): Domain =
            Domain(Verify.prepare(Decode.module(Canon.decode(bytes))))
    }
}

/**
 * How a peer opens (`ark_client::Options`): who authors, the token, alone or not, the autos, and
 * the numbers its link runs on.
 */
public data class Options(
    val user: String,
    val session: String,
    val token: String?,
    val alone: Boolean = false,
    val autos: Autos = Autos.System(),
    val timing: Timing = Timing(),
    /**
     * `docs/plan-live.md` The device this peer declares at `hello`: it subscribes to the live
     * instances its scope gives it. Null — the default — and it holds no live state and says
     * nothing it did not say.
     */
    val device: DeviceDecl? = null,
) {
    /** The same, with a link's numbers (`with_timing`). */
    public fun withTiming(t: Timing): Options = copy(timing = t)

    /**
     * `with_device`: declare this peer a device — its id (empty is the login's session), its kind,
     * its name, and whether it can play.
     */
    public fun withDevice(d: DeviceDecl): Options = copy(device = d)

    public companion object {
        /** A login of a server. */
        public fun server(user: String, session: String, token: String?): Options =
            Options(user, session, token)

        /** Nobody signed in: authoring as nobody, dialling nothing until a sign-in. */
        public fun signedOut(): Options = Options("", "", null)

        /** Its own authority, as `user` under the session `local`. */
        public fun alone(user: String): Options = Options(user, "local", null, alone = true)
    }
}

/** Where one of this peer's own intents stands (`ark_client::Standing`). */
public sealed class Standing {
    /** Applied here, not yet answered by the authority. */
    public object Pending : Standing()

    /** In the log. */
    public object Confirmed : Standing()

    /** Never going to be in the log, and the reason every replica reaches. */
    public data class Rejected(val why: String) : Standing()

    /** Not an intent this peer authored (in this run, or pending from the last). */
    public object Unknown : Standing()
}

/**
 * What one [Peer.pump] came to (`ark_client::Pumped`) — precisely what moved, so that a host
 * publishes nothing when nothing did (`docs/plan-reactive.md`): a screen is told *what* moved,
 * never that a pump happened. [quiet] is the whole of "nothing".
 */
public class Pumped {
    /** A frame arrived. */
    public var moved: Boolean = false
    /** The socket opened, and `Hello` went out. */
    public var opened: Boolean = false
    /** The socket that was open went, and why. The link dials again. */
    public var dropped: String? = null
    /** The server turned this login away; the link stops until a new token and a reconnect. */
    public var denied: String? = null
    /** Verdicts that arrived. */
    public var rejected: Int = 0
    /** Something worth a status line: a frame that did not decode, a write that failed. */
    public var note: String? = null
    /**
     * The optimistic store moved since the last pump — by this pump's frames, or by whatever the
     * app authored in between: [Peer.takeChanges] has something new for the views. False is a pump
     * no view needs to hear of.
     */
    public var views: Boolean = false
    /**
     * `docs/plan-live.md` A live instance's view moved, or a verdict on a call arrived: [Peer.live]
     * and [Peer.takeLiveRefusals] have something new.
     */
    public var live: Boolean = false
    /**
     * [Peer.status], when it differs by value from what the last pump reported: the one way a
     * status line learns it changed.
     */
    public var status: Status? = null
    /**
     * When to pump again if nothing wakes the host first: the link's redial ([Link.deadline]), a
     * monotonic millisecond. Null is no timer at all, which is a link dialling, open, or stopped.
     */
    public var deadline: Long? = null

    /**
     * Nothing moved: no frame, no change of the link, no verdict, no note, nothing for a view,
     * nothing live, the status as it was. A host answering a wake with a quiet pump publishes
     * nothing.
     */
    public fun quiet(): Boolean =
        !moved &&
            !opened &&
            dropped == null &&
            denied == null &&
            rejected == 0 &&
            note == null &&
            !views &&
            !live &&
            status == null
}

/**
 * The host's [Wake] behind a "drain requested" flag (`docs/plan-reactive.md`): the first event
 * after a pump rings it, and every event after that until the next pump is already answered — so a
 * burst of fifty frames is one wake, one pump, one settle. The flag is cleared as a pump starts,
 * before it drains, so an event that lands during the drain rings again rather than waiting for
 * somebody else.
 */
private class Coalesced(host: Wake) {
    private val asked = java.util.concurrent.atomic.AtomicBoolean(false)
    val wake: Wake = { if (!asked.getAndSet(true)) host() }

    /** A pump is starting: the next event asks again. */
    fun answered() {
        asked.set(false)
    }
}

/** Everything a status line or a debug screen shows (`ark_client::Status`). */
public data class Status(
    val user: String,
    val session: String,
    val alone: Boolean,
    /** Nobody is signed in: nothing is dialled. */
    val signedOut: Boolean,
    /** The engine is linked: the socket is open and `Hello` was said. */
    val linked: Boolean,
    /**
     * The link in a word: `alone`, `signed out`, `offline`, `idle`, `connecting`, `joining`,
     * `open`, `waiting`.
     */
    val link: String,
    /** Intents a join re-queued that the server has not yet answered. */
    val joining: Int,
    /** Where this peer last shared a log with a server. */
    val fork: Fork,
    val url: String?,
    /** Connections opened. */
    val opens: Long,
    /** The connection count the engine keeps: what a live room compares. */
    val epoch: Long,
    /** The last confirmed sequence. */
    val cursor: Seq,
    val pending: Int,
    /** Pending intents the server answered `held`: it does not run their function yet (D1). */
    val held: Int,
    /** The server runs another module than this peer's (D1). */
    val behind: Boolean,
    val diverged: Int,
    val denied: String?,
    val lastClose: String?,
    /** Live state frames received (`applied`, `refused`, `live_snapshot`). */
    val heardFrames: Long,
    val badFrames: Long,
)

/**
 * `THROTTLE_MS`: the least time between two ephemeral calls [Peer.callThrottled] sends for one
 * transition, in milliseconds — twenty a second, under the server's thirty, the final value always
 * sent.
 */
public const val THROTTLE_MS: Long = 50

/** A verdict against an intent this peer authored (`ark_client::Rejection`). */
public data class Rejection(val id: Id, val reason: String)

/**
 * A peer (`ark_client::Peer`): opened over a storage, authoring by name, written down on every
 * `persist` in the reference's records and order.
 */
public class Peer
private constructor(
    public val domain: Domain,
    private val schema: Schema,
    /** The engine's client: the replica, and what is queued for the server. */
    public val client: Client,
    private val storage: Storage,
    ctx: Ctx,
    private val autos: Autos,
    private val timing: Timing,
    private val writer: ReplicaWriter,
    public var fork: Fork,
    authored: Set<Id>,
    signedOut: Boolean,
) {
    /** Who this peer authors as. */
    public var ctx: Ctx = ctx
        private set

    public var alone: Boolean = false
        private set

    /** Nobody is signed in: nothing is dialled until a sign-in. */
    public var signedOut: Boolean = signedOut
        private set

    private var authority: Authority? = null
    private var aloneLog: AloneLog? = null
    private val authored = HashSet(authored)
    private val rejected = HashMap<Id, String>()
    private val local = HashSet<Id>()

    /** Every verdict against an intent of this peer's, oldest first, each once. */
    public val rejections: MutableList<Rejection> = ArrayList()
    private val checks = Checks()

    /** The link, if [connect] made one. */
    public var link: Link? = null
        private set

    private var heardFrames: Long = 0
    private var badFrames: Long = 0

    // The host's wake, coalesced: handed to every link this peer makes.
    private var waker: Coalesced? = null
    // The status the last pump reported, for `Pumped.status`.
    private var published: Status? = null
    // What the store's untaken changes were when the last pump ended — how
    // many, and whether rebuilt — so `Pumped.views` says what moved since,
    // whether or not the host has taken them.
    private var told: Pair<Int, Boolean> = 0 to false

    /** Settles run, for the tests that count a wake's. */
    public var settles: Long = 0
        private set

    // `docs/plan-live.md` What this peer knows of the server's clock: the `at` of every ordered
    // `applied` it heard, less its own clock when it heard it, the latest few.
    private val offsets = ArrayDeque<Long>()
    // The counter of this device's ephemeral calls: from the wall clock at open, so a peer started
    // again goes on above where it was.
    private var counter: Long = java.lang.System.currentTimeMillis() * 1000
    // Ephemeral calls held back by `callThrottled`, each with when it may go.
    private val throttled = ArrayList<Throttled>()
    // A live view moved since the last pump.
    private var liveMoved = false

    // An ephemeral call `callThrottled` holds until it may go: the newest per transition of a
    // state, sent at most once per THROTTLE_MS, the last always.
    private class Throttled(
        val state: String,
        val transition: String,
        var waiting: Map<String, Value>?,
        var sent: Long
    )

    private class AloneLog(
        val journal: Journal,
        val unwritten: MutableList<Triple<Seq, LogEntry, FactList>>
    )

    private class Checks {
        var epoch: Long = 0
        var at: Seq? = null
        val asked = ArrayDeque<Boolean>()
        var served = false
        val agreed = ArrayList<Pair<Seq, Boolean>>()
        val unknown = ArrayList<Seq>()
    }

    /** The replica. */
    public val replica: Replica
        get() = client.replica

    /**
     * The schema this peer's store is laid out under: the module's, or — for a peer holding a union
     * of the module's scopes — the device's, whose tables have only the held columns (D2).
     */
    public val storeSchema: Schema
        get() = client.replica.schema

    /** The optimistic store — `confirmed` with `pending` replayed — which every query reads. */
    public val store: MemoryStore
        get() = client.replica.view

    /** The cursor: the last sequence confirmed. */
    public val cursor: Seq
        get() = client.replica.cursor

    /** Every answer to a verify this peer was asked to make: agreed, or not. */
    public val agreed: List<Pair<Seq, Boolean>>
        get() = checks.agreed

    /** Every verify the authority could not answer. */
    public val unknown: List<Seq>
        get() = checks.unknown

    public companion object {
        /**
         * `Peer::open`: the replica from `storage` (or empty), what was pending replayed on top,
         * and — alone — sequenced. A storage last used the other way is carried across: opened
         * alone over a server's replica, the peer leaves it; opened for a server over local
         * history, the history is re-queued as a join does.
         */
        public fun open(domain: Domain, storage: Storage, opts: Options): Peer {
            val schema = domain.module.schema
            val st = Records.load(storage, schema)
            val confirmed: MemoryStore
            val cursor: Seq
            val pending: List<LogEntry>
            val was: Ctx
            val writer: ReplicaWriter
            val fork: Fork
            var partial: Map<String, List<String>>? = null
            if (st != null) {
                partial = st.file.partial
                val f = st.file
                was = Ctx(f.user, f.session)
                writer = ReplicaWriter.of(storage, st)
                confirmed = f.confirmed
                cursor = f.cursor
                pending = f.pending
                fork = f.fork
            } else {
                val (ps, pst) = Records.loadPending(storage)
                pending = ps ?: emptyList()
                writer = ReplicaWriter.fresh(storage, pst)
                confirmed = MemoryStore.empty(schema)
                cursor = 0
                was = Ctx.nobody
                fork = Fork()
            }
            val hadLog = storage.load(Layout.alone().snapshot) != null
            val r = Replica.open(confirmed.schema, domain.closures, confirmed, cursor, pending)
            r.partial = partial?.let { java.util.TreeMap(it) }
            r.logId = writer.namedLog
            val client = Client.open(r, Mode.Whole, opts.token).withSchema(schema)
            // `docs/plan-live.md` A device of a module with live state holds it.
            if (!domain.live.isEmpty && opts.device != null)
                client.withLive(Watch(domain.live, opts.device))
            client.module = domain.hash
            client.replica.takeConfirmed()
            var ctx = Ctx(opts.user, opts.session)
            val signedOut = !opts.alone && ctx.isNobody
            if (signedOut) {
                ctx = if (hadLog) Ctx.nobody else was
            } else if (!opts.alone &&
                client.replica.pending.any { it.actor.isEmpty() && it.session.isEmpty() }) {
                client.signIn(ctx, opts.token)
                writer.forgetPending()
            }
            val peer =
                Peer(
                    domain,
                    schema,
                    client,
                    storage,
                    ctx,
                    opts.autos,
                    opts.timing,
                    writer,
                    fork,
                    pending.map { it.id }.toSet(),
                    signedOut)
            when {
                hadLog -> peer.resumeAlone()
                opts.alone -> peer.startAlone()
            }
            if (hadLog && !opts.alone) peer.forkBackTo(peer.ctx)
            peer.commitAlone()
            peer.collectRejections()
            peer.persist()
            peer.client.replica.takeChanges()
            return peer
        }
    }

    // -- who authors --------------------------------------------------------------

    /**
     * `sign_in`: `user` under `session`, proven with `token`; every intent still pending authored
     * as nobody becomes theirs, and is written down before anything is said.
     */
    public fun signIn(user: String, session: String, token: String?) {
        if (alone) throw PeerError("a peer alone has no server to sign in to")
        ctx = Ctx(user, session)
        client.signIn(ctx, token)
        signedOut = false
        collectRejections()
        writer.forgetPending()
        persist()
        reconnect()
    }

    /** `sign_out`: the token forgotten and the connection dropped; the login is kept. */
    public fun signOut() {
        if (alone) return
        signedOut = true
        client.token = null
        link?.stop("signed out")
        if (client.linked) client.disconnected()
    }

    /** `set_roles`: the roles this login believes it holds, frozen in what it authors (D1). */
    public fun setRoles(roles: Collection<String>) {
        ctx = ctx.withRoles(roles)
    }

    // -- alone and back -----------------------------------------------------------

    /**
     * `leave`, server → alone (`docs/plan-alone.md` §1): the connection dropped, the fork recorded
     * where the replica stands, and this peer its own authority from its confirmed store on.
     */
    public fun leave() {
        if (alone) return
        link?.stop("alone")
        link = null
        if (client.linked) client.disconnected()
        client.denied = null
        startAlone()
        commitAlone()
        collectRejections()
        persist()
    }

    /**
     * `join`, alone → server, up to the connecting: the local history taken back out of the
     * confirmed store and re-queued as `login`'s (or nobody's), written down; the caller connects.
     */
    public fun join(user: String?, session: String?, token: String?) {
        var who = if (user != null) Ctx(user, session ?: "") else Ctx.nobody
        if (user == null) who = who.withRoles(ctx.roles)
        if (alone) {
            ctx = who
            signedOut = user == null
            client.token = if (user != null) token else null
            forkBackTo(who)
        } else if (user != null) {
            signIn(user, session ?: "", token)
        }
    }

    /**
     * `join`, alone → server (`Peer::join`): [join] up to the connecting, then the link to `url`
     * dialled — as `login`, or as nobody for a sign-in to make the signer's. On a peer that already
     * has a server, a sign-in (with a login) and [connect].
     */
    public fun join(url: String, login: Login?) {
        join(login?.user?.id, login?.session, login?.token)
        connect(url)
    }

    /** What the next `Hello` proves the login with; [reconnect] for it to be said now. */
    public fun setToken(token: String?) {
        client.token = token
    }

    // -- a screen's side ----------------------------------------------------------

    /**
     * `mutate`: an intent by name, its autos drawn here once and frozen in the entry, applied to
     * the optimistic store, kept durable as pending, pushed if linked. The entry's id, or
     * `Refused`.
     */
    public fun mutate(name: String, args: Map<String, Value>): Id {
        val (fh, f) = domain.mutator(name)
        val autosDrawn = autos.draw(f)
        val id = autos.newId()
        val (e, why) = client.mutate(id, ctx, fh, autosDrawn, args)
        if (e == null) throw Refused(why!!)
        authored.add(id)
        commitAlone()
        collectRejections()
        persistLog()
        writer.persistPending(client.replica.pending)
        // Asked for is done at once, the push with it: the intent is durable,
        // so it can go (`docs/plan-reactive.md`, "the app asks").
        flush()
        return id
    }

    /**
     * What a view of the optimistic store is told (`take_changes`): everything since the last ask,
     * or `Rebuilt`. Hand it to every [ClientView.update].
     */
    public fun takeChanges(): Changes {
        told = 0 to false
        return client.replica.takeChanges()
    }

    /** `query`: a query by name over the optimistic store, as this peer's user. */
    public fun query(name: String, args: Map<String, Value>): Value {
        val (h, _) = domain.query(name)
        val st = store
        return try {
            dev.arkdb.Eval.queryClosure(
                st.schema, domain.closures[dev.arkdb.Hex.encode(h)]!!, ctx, args, st)
        } catch (e: dev.arkdb.EvalFault.Verdict) {
            throw Refused(e.refusal)
        } catch (e: dev.arkdb.EvalFault.Bug) {
            throw PeerError("bug: $name: ${dev.arkdb.debugEvalError(e.error)}")
        }
    }

    /** `check`: the form validator over a partial input; a message per failing field. */
    public fun check(name: String, partial: Map<String, Value>): dev.arkdb.Checked {
        val (h, _) = domain.function(name) ?: throw PeerError("the module has no function $name")
        val st = store
        return try {
            dev.arkdb.Eval.check(
                st.schema, domain.closures[dev.arkdb.Hex.encode(h)]!!, ctx, partial, st)
        } catch (e: dev.arkdb.EvalFault) {
            throw PeerError("$name: ${e.message}")
        }
    }

    /** `view`: a query held as a list and kept up to date ([ClientView]). */
    public fun view(name: String, args: Map<String, Value>): ClientView =
        ClientView.open(this, name, args)

    /** Verdicts against this peer's intents since the last ask (`take_rejections`). */
    public fun takeRejections(): List<Rejection> {
        collectRejections()
        val out = ArrayList(rejections)
        rejections.clear()
        return out
    }

    /** Where one of this peer's intents stands: what a screen shows beside the item it made. */
    public fun standing(id: Id): Standing {
        rejected[id]?.let {
            return Standing.Rejected(it)
        }
        val r = client.replica
        r.rejections
            .firstOrNull { it.first == id }
            ?.let {
                return Standing.Rejected(refusalText(it.second))
            }
        return when {
            r.pending.any { it.id == id } -> Standing.Pending
            id in authored -> Standing.Confirmed
            else -> Standing.Unknown
        }
    }

    /** Intents authored here and not yet answered. */
    public val pendingLen: Int
        get() = client.replica.pending.size

    /** Whether the engine is linked: the socket is open and `Hello` said. */
    public val linked: Boolean
        get() = client.linked

    /** How many connections this peer has had. */
    public val epoch: Long
        get() = client.epoch

    /** Why the server last turned this login away, until a reconnect. */
    public val denied: String?
        get() = client.denied

    // -- live state ---------------------------------------------------------------

    /**
     * `docs/plan-live.md` `instance`: the instance of the live state `name` this peer's login holds
     * — its scope's first key — if this peer is a device of a module with that state.
     */
    public fun instance(name: String): Instance? = client.live?.instanceOf(name, ctx)

    /**
     * `live`: the state `name` as this device sees it — what the server confirmed with this
     * device's own calls on top, ephemeral slots and all. Null before anything is known of it: a
     * call, or the snapshot the server sends on connecting.
     */
    public fun live(name: String): Value? {
        val i = instance(name) ?: return null
        return client.live?.view(i)
    }

    /**
     * `call`: a transition of the live state `name`, run over this device's view at once — so
     * [live] has moved before this returns — at its guess of the server's clock ([serverNow]), and
     * sent if linked. An ordered call is pending until the server answers, and said again on every
     * connection until it does; an ephemeral one is applied and done, and dropped while unlinked.
     * One path for both: the kind is the module's. A refusal here is the preview's, thrown as
     * `Refused`, and nothing is sent.
     */
    public fun call(name: String, transition: String, input: Map<String, Value>): Kind {
        val kind = callNow(name, transition, input)
        flush()
        return kind
    }

    // The call previewed and queued; what `call` and the throttle share.
    private fun callNow(name: String, transition: String, input: Map<String, Value>): Kind {
        val i = instance(name) ?: throw PeerError("no live state $name on this peer")
        val id = autos.newId()
        val now = serverNow()
        counter += 1
        val kind =
            try {
                client.call(ctx, i, transition, input, id, now, counter)
            } catch (e: LiveRefusal) {
                throw Refused(e.refusal)
            }
        liveMoved = true
        return kind
    }

    /**
     * `call_throttled`: [call] for an ephemeral transition a finger drives — a pointer dragged, a
     * scrubber held — at most one send per [THROTTLE_MS] for this transition, trailing edge, so the
     * final value always goes. The view moves at once whatever is sent; what is held is sent by a
     * [pump] at [deadline]. An ordered call is never held: its order is the point of it.
     */
    public fun callThrottled(name: String, transition: String, input: Map<String, Value>): Kind =
        callThrottledAt(Link.monotonicMs(), name, transition, input)

    /** [callThrottled] at a given monotonic millisecond, so that the throttle is testable. */
    public fun callThrottledAt(
        now: Long,
        name: String,
        transition: String,
        input: Map<String, Value>
    ): Kind {
        if (domain.live.kind(name, transition) == Kind.Ordered) return call(name, transition, input)
        val t = throttled.firstOrNull { it.state == name && it.transition == transition }
        if (t != null && now < t.sent + THROTTLE_MS) {
            // The view moves now; the send waits for the window.
            val w = client.live ?: throw PeerError("no live state $name on this peer")
            val i = instance(name) ?: throw PeerError("no live state $name on this peer")
            val id = autos.newId()
            val at = serverNow()
            counter += 1
            val called =
                try {
                    w.call(ctx, i, transition, input, id, at, counter)
                } catch (e: LiveRefusal) {
                    throw Refused(e.refusal)
                }
            t.waiting = input
            liveMoved = true
            return called.kind
        }
        val kind = call(name, transition, input)
        if (t != null) {
            t.sent = now
            t.waiting = null
        } else throttled.add(Throttled(name, transition, null, now))
        return kind
    }

    // What the throttle held and is now due, sent.
    private fun releaseThrottled(now: Long) {
        for (t in throttled) {
            val input = t.waiting ?: continue
            if (now < t.sent + THROTTLE_MS) continue
            t.sent = now
            t.waiting = null
            try {
                callNow(t.state, t.transition, input)
            } catch (e: PeerError) {
                // The state no longer takes it; the next value will say.
            }
        }
    }

    /**
     * `take_live_refusals`: the verdicts the server gave this device's live calls since the last
     * ask.
     */
    public fun takeLiveRefusals(): List<Rejection> =
        client.live?.takeRefused()?.map { (id, why) -> Rejection(id, why) } ?: emptyList()

    /**
     * `server_offset`: the server's clock less this peer's, in milliseconds, as the `at` stamps of
     * what it has heard say — the largest of the latest few, which is the one that waited least on
     * the way. Null before any. What an app computes a moving position with.
     */
    public val serverOffset: Long?
        get() = offsets.maxOrNull()

    /**
     * `server_now`: this peer's guess at the server's clock now — its own, moved by [serverOffset].
     */
    public fun serverNow(): Long = java.lang.System.currentTimeMillis() + (serverOffset ?: 0)

    // -- the socket ---------------------------------------------------------------

    /**
     * `connect`: dial the server's sync socket (`ws://host/sync`) and keep it up — every [pump]
     * moves frames, and a drop dials again with a backoff. Replaces any link there was. Signed out,
     * the link is kept and not dialled: a sign-in starts it.
     */
    public fun connect(url: String, dial: Dial = platformDial(timing)) {
        disconnect()
        client.denied = null
        val l = Link(url, dial, timing)
        waker?.let { l.setWaker(it.wake) }
        if (signedOut) l.stop("signed out")
        link = l
    }

    /**
     * Wake the host whenever the socket has something to report — it opened, a frame arrived, it
     * closed — instead of being polled for it (`docs/plan-reactive.md`). `wake` must only *ask* for
     * a pump: it is called from OkHttp's threads, and a burst of events calls it once until the
     * next [pump] starts. Handed to the link there is and to every one made after. A host so woken
     * pumps when woken, after anything it asks of the peer, and at [deadline] — and at no other
     * time.
     */
    public fun setWaker(wake: Wake) {
        val c = Coalesced(wake)
        link?.setWaker(c.wake)
        waker = c
    }

    /**
     * When this peer wants a [pump] with nothing having woken its host: the link's redial after a
     * close, a monotonic millisecond ([Link.monotonicMs]; `0` is now). Null — dialling, open,
     * stopped, alone — is no timer at all. The host arms one timer for it, and the open that
     * follows clears it.
     */
    public fun deadline(): Long? {
        val l = link?.deadline
        val held = throttled.filter { it.waiting != null }.minOfOrNull { it.sent + THROTTLE_MS }
        return if (l != null && held != null) minOf(l, held) else l ?: held
    }

    /**
     * Send what the engine has queued, now: a push, a live call, a verify. [mutate], [call] and
     * [verify] do it themselves; a pump does it last.
     */
    public fun flush() {
        val l = link ?: return
        if (!l.isOpen) return
        for (f in takeOutgoingFrames()) l.send(f)
    }

    /** Go offline: close the socket and stop dialling. Everything authored meanwhile is pending. */
    public fun disconnect() {
        link?.stop("offline")
        if (client.linked) client.disconnected()
    }

    /**
     * Dial again after [disconnect] or a denial — with a new token, usually. Signed out, nothing.
     */
    public fun reconnect() {
        if (signedOut) return
        client.denied = null
        val l = link ?: return
        if (l.isOpen) {
            l.stop("reconnecting")
            client.disconnected()
        }
        l.resume()
    }

    /**
     * `pump`: one turn of the link — dial if due, hand the engine what arrived, settle it once
     * (R8), send what it queued, and write whatever moved. It is the unit of work of a wake
     * (`docs/plan-reactive.md`): a host given [setWaker] calls it when woken, after asking the peer
     * for anything, and at [deadline]; one without a waker — a test, a headless driver — calls it
     * by hand, as often as it likes, and a pump with nothing to do is [Pumped.quiet]. Without a
     * link it still writes, and a peer alone needs nothing else.
     */
    public fun pump(): Pumped = pumpAt(Link.monotonicMs())

    /**
     * [pump] at a given monotonic millisecond, so that the backoff and the deadline are testable.
     */
    public fun pumpAt(now: Long): Pumped {
        waker?.answered()
        val p = Pumped()
        releaseThrottled(now)
        val l = link
        if (l != null) {
            val polled = l.pollAt(now)
            if (polled.opened) {
                client.connected()
                p.opened = true
            }
            for (f in polled.frames) {
                p.moved = true
                try {
                    placeFrame(f)
                } catch (e: Exception) {
                    badFrames++
                    p.note = "a frame from the server: ${e.message ?: e.javaClass.simpleName}"
                }
            }
            settle().firstOrNull()?.let { n ->
                p.note = "the server's state at $n is not this replica's"
            }
            polled.closed?.let { why ->
                client.disconnected()
                p.dropped = why
            }
            client.denied?.let { reason ->
                val now = link
                if (now != null && now.state != LinkState.Idle) {
                    now.stop("denied: $reason")
                    p.denied = reason
                }
            }
            val frames = takeOutgoingFrames()
            link?.let { now -> for (f in frames) now.send(f) }
        }
        val before = rejections.size
        collectRejections()
        p.rejected = rejections.size - before
        try {
            persist()
        } catch (e: Exception) {
            p.note = e.message ?: e.javaClass.simpleName
        }
        val r = client.replica
        val now = r.changes.size to r.rebuilt
        p.views = now != told
        told = now
        val moved = client.live?.takeMoved() ?: false
        p.live = liveMoved || moved
        liveMoved = false
        val s = status()
        if (s != published) {
            published = s
            p.status = s
        }
        p.deadline = deadline()
        return p
    }

    /** `status`: everything a status line or a debug screen shows. */
    public fun status(): Status {
        val joining = joining()
        val l = link
        val word =
            when {
                alone -> "alone"
                signedOut -> "signed out"
                l == null -> "offline"
                else ->
                    when (val st = l.state) {
                        is LinkState.Idle -> "idle"
                        is LinkState.Connecting,
                        is LinkState.Open ->
                            if (joining > 0) "joining"
                            else if (st is LinkState.Open) "open" else "connecting"
                        is LinkState.Waiting -> "waiting"
                    }
            }
        return Status(
            user = ctx.user,
            session = ctx.session,
            alone = alone,
            signedOut = signedOut,
            linked = client.linked,
            link = word,
            joining = joining,
            fork = fork,
            url = l?.url,
            opens = l?.opens ?: 0,
            epoch = client.epoch,
            cursor = client.replica.cursor,
            pending = pendingLen,
            held = client.held(),
            behind = client.behind(),
            diverged = client.replica.diverged.size,
            denied = client.denied,
            lastClose = l?.lastClose,
            heardFrames = heardFrames,
            badFrames = badFrames,
        )
    }

    // Intents a join re-queued that are still pending.
    private fun joining(): Int =
        if (local.isEmpty()) 0 else client.replica.pending.count { it.id in local }

    /**
     * Close: the link stopped, and whatever is not yet written, written (the reference's `Drop`).
     */
    public fun close() {
        link?.stop("closed")
        link = null
        persist()
    }

    // -- sans-io: a transport of the caller's own ---------------------------------

    public fun connected() {
        client.connected()
    }

    public fun disconnected() {
        client.disconnected()
    }

    /** `recv_frame`: a frame from the server, placed and settled. */
    public fun recvFrame(bytes: ByteArray) {
        placeFrame(bytes)
        settle()
    }

    /** Frames from the server, each placed, then one settle and one write: `pump`'s shape. */
    public fun pump(frames: List<ByteArray>) {
        for (f in frames) placeFrame(f)
        settle()
        collectRejections()
        persist()
    }

    /** What is queued for the server, as canonical bytes. */
    public fun takeOutgoingFrames(): List<ByteArray> = client.takeOutgoing().map(ClientMsg::encode)

    private fun placeFrame(bytes: ByteArray) {
        when (val got = ServerMsg.decodeFor(bytes, client.schema)) {
            is Received.Msg -> {
                val msg = got.msg
                if (msg is ServerMsg.AppliedLive ||
                    msg is ServerMsg.RefusedLive ||
                    msg is ServerMsg.SnapshotLive)
                    heardFrames++
                // `docs/plan-live.md` The server's clock, as an ordered call stamps it: a sample of
                // the offset, the latest sixteen kept.
                val order = (msg as? ServerMsg.AppliedLive)?.applied?.order
                if (order is dev.arkdb.Order.Ordered) {
                    offsets.addLast(order.at - java.lang.System.currentTimeMillis())
                    while (offsets.size > 16) offsets.removeFirst()
                }
                if (got.msg is ServerMsg.Batch || got.msg is ServerMsg.SnapshotOf) served()
                client.recv(got.msg)
            }
            is Received.Snap -> {
                served()
                client.recvSnapshot(got.snapshot)
            }
        }
    }

    private fun served() {
        thisConnection()
        checks.served = true
    }

    private fun thisConnection() {
        if (checks.epoch != client.epoch) {
            checks.epoch = client.epoch
            checks.at = null
            checks.asked.clear()
            checks.served = false
        }
    }

    /** `verify`: alone, asked of the local authority; otherwise of the server. */
    public fun verify() {
        val a = authority
        if (a == null) {
            askVerify(false)
            flush()
            return
        }
        val (n, h) = client.replica.verifyAt()
        val theirs = hashAt(a.log, n, a.store)
        if (theirs != null) checks.agreed.add(n to theirs.contentEquals(h))
        else checks.unknown.add(n)
    }

    // `Log::hash_at`, at the head: the alone log's base stands at its head
    // with its entries taken (`Log.takeEntries`), so the head is all it can say.
    private fun hashAt(log: Log, n: Seq, head: MemoryStore): ByteArray? =
        if (n == log.headSeq()) Hash.stateHash(head) else null

    private fun askVerify(auto: Boolean) {
        thisConnection()
        val before = client.out.size
        client.verifyAll()
        if (client.out.size > before) {
            checks.asked.addLast(auto)
            checks.at = client.replica.cursor
        }
    }

    /** `settle`: the client settled, verify answers sorted, and a verify asked after a page. */
    private fun settle(): List<Seq> {
        settles++
        val paging = client.more
        client.settle()
        thisConnection()
        val disagreed = ArrayList<Seq>()
        val answers = ArrayList(client.agreed)
        client.agreed.clear()
        for ((n, answer) in answers) {
            val auto = checks.asked.removeFirstOrNull() ?: false
            when {
                answer == null -> if (!auto) checks.unknown.add(n)
                else -> {
                    if (!answer) disagreed.add(n)
                    if (!auto || !answer) checks.agreed.add(n to answer)
                }
            }
        }
        val due = checks.served && !paging && checks.at != client.replica.cursor
        if (authority == null && client.linked && due) askVerify(true)
        return disagreed
    }

    // -- writing it down ----------------------------------------------------------

    /**
     * `persist`: what moved since the last write — the local history first, then the pending
     * intents, the login, and the confirmed store, each as the reference writes it.
     */
    public fun persist() {
        persistLog()
        writer.persist(client.replica, fork, ctx)
    }

    // Alone, what is pending sequenced, and the entries taken out of the
    // authority's memory into the log's next write.
    private fun commitAlone() {
        val a = authority ?: return
        localCommit(a, client.replica)
        val taken = a.log.takeEntries()
        aloneLog?.unwritten?.addAll(taken)
    }

    // The entries sequenced alone since the last write, as one page of the
    // local history; then its pages merged.
    private fun persistLog() {
        val l = aloneLog ?: return
        val to = l.unwritten.lastOrNull()?.first ?: return
        val page = java.io.ByteArrayOutputStream()
        for ((n, e, f) in l.unwritten) page.write(LogRecords.encodeRecord(n, e, f))
        l.journal.append(storage, page.toByteArray(), to)
        l.unwritten.clear()
        l.journal.merge(storage)
    }

    // The authority a peer alone is: the confirmed store as its state at
    // `head`, the ids its local history holds, and no entries.
    private fun aloneAuthority(head: Seq, ids: Map<Id, Seq>): Authority {
        val a = Authority(schema, domain.closures)
        a.log = Log(head, MemoryStore.empty(schema), ByteArray(0), null)
        a.log.ids.putAll(ids)
        a.store = client.replica.confirmed.copy()
        return a
    }

    // `resume_alone`: carry on alone from the local history on the storage.
    private fun resumeAlone() {
        val layout = Layout.alone()
        val cursor = client.replica.cursor
        val ids = HashMap<Id, Seq>()
        val ahead = ArrayList<Triple<Seq, LogEntry, FactList?>>()
        val (journal, o) =
            Journal.open(storage, layout, schema) { n, e, f ->
                ids[e.id] = n
                if (n > cursor) ahead.add(Triple(n, e, f))
            }
        val base = o.snapshot ?: throw StorageError("the local history has no snapshot")
        val fork = Fork(base.logId, base.horizon())
        val head = o.head
        var r = client.replica
        if (cursor > head || cursor < fork.cursor) {
            val st = base.baseStore
            LogRecords.read(storage, layout, schema) { n, _, f ->
                if (n <= head) st.applyChanges(f)
            }
            val pending = ArrayList(r.pending)
            r = Replica.open(r.schema, r.bodies, st, head, pending)
            client.replica = r
        } else if (ahead.isNotEmpty()) {
            r.receiveBatch(ahead)
        }
        if (r.pending.any { e -> ids[e.id]?.let { it <= head } == true }) {
            val pending = r.pending.filter { e -> ids[e.id]?.let { it > head } ?: true }
            r = Replica.open(r.schema, r.bodies, r.confirmed.copy(), r.cursor, pending)
            client.replica = r
            writer.forgetPending()
        }
        r.logId = null
        authority = aloneAuthority(head, ids)
        this.fork = fork
        alone = true
        signedOut = false
        aloneLog = AloneLog(journal, ArrayList())
    }

    // `start_alone`, server → alone: the fork where the replica stands, the
    // local history's snapshot its confirmed store, written before anything
    // else moves; what the server sent and was not applied left behind.
    private fun startAlone() {
        val r = client.replica
        r.inbox.clear()
        r.through = r.cursor
        val fork = Fork(r.logId, r.cursor)
        val st = r.confirmed.copy()
        val base = Log(fork.cursor, st, Hash.stateHash(st), fork.logId)
        val journal = Journal.create(storage, Layout.alone(), base)
        r.logId = null
        authority = aloneAuthority(fork.cursor, emptyMap())
        this.fork = fork
        alone = true
        signedOut = false
        local.clear()
        aloneLog = AloneLog(journal, ArrayList())
    }

    // `fork_back_to`, alone → server up to the connecting: the local history
    // read back from the storage, taken out of the confirmed store, and
    // re-queued as `who`'s; written down, and only then the history removed.
    private fun forkBackTo(who: Ctx) {
        persistLog()
        val fork = this.fork
        val layout = Layout.alone()
        val facts = ArrayList<FactList>()
        val requeue = ArrayList<LogEntry>()
        val o =
            LogRecords.read(storage, layout, schema) { n, e, f ->
                if (n > fork.cursor) {
                    requeue.add(e.by(who.user, who.session))
                    facts.add(f)
                }
            }
        val cursor = client.replica.cursor
        if (o.head != cursor || cursor - fork.cursor != facts.size.toLong())
            throw StorageError(
                "the local history runs from ${fork.cursor} to ${o.head} and the store is at $cursor")
        local.clear()
        requeue.forEach { local.add(it.id) }
        authored.addAll(local)
        client.replica.forkBack(facts, fork.cursor, fork.logId, requeue)
        authority = null
        aloneLog = null
        alone = false
        collectRejections()
        writer.forgetPending()
        persist()
        Journal.destroy(storage, layout)
    }

    private fun collectRejections() {
        val rs = ArrayList(client.replica.rejections)
        client.replica.rejections.clear()
        for ((id, why) in rs) {
            if (rejected.containsKey(id)) continue
            val reason = refusalText(why)
            rejected[id] = reason
            rejections.add(Rejection(id, reason))
        }
    }
}
