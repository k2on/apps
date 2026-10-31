// The household's budget as the screens see it: money's views over a peer —
// the Rust kernel through the bridge, a replica of the server's or the
// demo's household — kept in step with it, and every change the screens
// make, as one of the domain's functions.
//
// **Everything that touches the views runs on one thread** (`run`): the
// first read, every splice, every mutation and the read after it. The peer
// says when anything moved (its listener, on a thread of the kernel's); a
// drain then runs there once, asks each view what it did, splices the
// patches into the lists it holds, and publishes one [Ledger] — an
// immutable snapshot the screens draw from (`state`, a StateFlow). A peer
// at rest runs nothing: there is no clock here.
//
// **The lists are spliced, not re-read**, as harken's are: each view is read
// once and then asked what it did (`Held`). The month's three views are
// opened again for another month; a register is held while a screen shows
// it and let go after the last one leaves.
//
// **A refusal is said, not thrown.** The domain's own sentence for why a
// change was not taken — refused here, or by the server later — is a note
// (`notes`), which the screens show as a snackbar. A form asks first
// ([check]) and says the sentence under its field.
package dev.money.android

import dev.arkdb.ffi.Id
import dev.arkdb.ffi.Listener
import dev.arkdb.ffi.Peer
import dev.arkdb.ffi.Refusal
import dev.arkdb.ffi.Update
import dev.arkdb.ffi.View
import dev.arkdb.ffi.splice
import dev.money.android.gen.AddCategory
import dev.money.android.gen.AddGroup
import dev.money.android.gen.AddTransaction
import dev.money.android.gen.Assign
import dev.money.android.gen.Auto
import dev.money.android.gen.AutoAssign
import dev.money.android.gen.Categorize
import dev.money.android.gen.DeleteCategory
import dev.money.android.gen.EditTransaction
import dev.money.android.gen.HideCategory
import dev.money.android.gen.IgnoreLinked
import dev.money.android.gen.LinkAccount
import dev.money.android.gen.Money
import dev.money.android.gen.Month
import dev.money.android.gen.MoveCategory
import dev.money.android.gen.MoveGroup
import dev.money.android.gen.MoveMoney
import dev.money.android.gen.OpenAccount
import dev.money.android.gen.Reconcile
import dev.money.android.gen.Register
import dev.money.android.gen.RenameCategory
import dev.money.android.gen.SetCleared
import dev.money.android.gen.SetTarget
import dev.money.android.gen.Txns
import dev.money.ffi.DemoBank
import dev.money.ffi.moneyAddMonths
import java.security.MessageDigest
import java.util.concurrent.CopyOnWriteArrayList
import java.util.concurrent.Executor
import java.util.concurrent.ExecutorService
import java.util.concurrent.Executors
import java.util.concurrent.RejectedExecutionException
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.flow.MutableSharedFlow
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.SharedFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asSharedFlow
import kotlinx.coroutines.flow.asStateFlow
import uniffi.money_domain.AccountLine
import uniffi.money_domain.BankLine
import uniffi.money_domain.BudgetLine
import uniffi.money_domain.GroupLine
import uniffi.money_domain.LinkedLine
import uniffi.money_domain.PayeeLine
import uniffi.money_domain.SpendingLine
import uniffi.money_domain.Summary
import uniffi.money_domain.TxnLine

/** The link to the server, in what a screen says of it. */
data class Link(
    val linked: Boolean = false,
    /** `online`, `connecting`, `waiting`, … — or `alone` for the demo. */
    val word: String = "",
    /** Changes made here the server has not answered yet. */
    val pending: Int = 0,
)

/** Everything the screens draw, at one moment. */
data class Ledger(
    val today: String = "",
    /** The month the budget shows, `2026-10`. */
    val month: String = "",
    val summary: Summary? = null,
    val lines: List<BudgetLine> = emptyList(),
    val groups: List<GroupLine> = emptyList(),
    val accounts: List<AccountLine> = emptyList(),
    val inbox: List<TxnLine> = emptyList(),
    /** Each held register by [key]. */
    val registers: Map<String, List<TxnLine>> = emptyMap(),
    val payees: List<PayeeLine> = emptyList(),
    val spending: List<SpendingLine> = emptyList(),
    val banks: List<BankLine> = emptyList(),
    val linked: List<LinkedLine> = emptyList(),
    val link: Link = Link(),
    /** Who is signed in, as a name to show. */
    val who: String = "",
    val demo: Boolean = false,
    /** The server's address; empty for the demo. */
    val server: String = "",
    /** A sync of the bank is under way (pull to refresh). */
    val syncing: Boolean = false,
    /** The views have been read once: the first screen has what it draws. */
    val ready: Boolean = false,
    /** Why the server turned this login away, when it did. */
    val denied: String? = null,
) {
    fun line(category: Id): BudgetLine? = lines.firstOrNull { it.categoryId == category }

    fun account(id: Id): AccountLine? = accounts.firstOrNull { it.id == id }

    fun register(account: Id?): List<TxnLine> = registers[key(account)] ?: emptyList()

    /** The month's categories, by group in the budget's order, the hidden left out unless `hidden`. */
    fun byGroup(hidden: Boolean = false): List<Pair<GroupLine, List<BudgetLine>>> =
        groups
            .filter { hidden || !it.hidden }
            .sortedBy { it.pos }
            .map { g -> g to lines.filter { it.groupId == g.id && (hidden || !it.hidden) }.sortedBy { it.pos } }

    companion object {
        /** A register's key: an account's id, or `all`. */
        fun key(account: Id?): String = account?.hex ?: "all"
    }
}

/** A view read once and then spliced: the rows a screen holds. */
private class Held<T>(private val view: View<T>) : AutoCloseable {
    val rows: ArrayList<T> = ArrayList(view.rows())

    /** What the view did since, spliced in; whether anything moved. */
    fun settle(): Boolean =
        when (val u = view.take()) {
            is Update.Unchanged -> false
            is Update.Patched -> {
                rows.splice(u.changes)
                true
            }
            is Update.Reset -> {
                rows.clear()
                rows.addAll(u.rows)
                true
            }
        }

    override fun close() = view.close()
}

/** A transaction being entered or corrected: what the form holds. */
data class TxnForm(
    /** The one corrected; none for a new one. */
    val editing: Id? = null,
    val account: Id? = null,
    val date: String = "",
    val payee: String = "",
    val category: Id? = null,
    /** The other account, for a transfer. */
    val transferTo: Id? = null,
    val memo: String = "",
    /** Positive in, negative out, in cents. */
    val amount: Long = 0,
    val cleared: Boolean = false,
    /** Who entered the one corrected. */
    val by: String = "",
    /**
     * What the bank called the one corrected, when it was imported and the bank's words say more than
     * the payee does: `CHIPOTLE 1234 SAN FRANCISCO CA` for Chipotle. Shown, never sent.
     */
    val bankSaid: String = "",
)

class Book
private constructor(
    private val peer: Peer,
    private val today: String,
    val demo: Boolean,
    val server: String,
    private val run: Executor,
    private val owned: ExecutorService?,
    private val listen: Boolean,
) : AutoCloseable {
    /** The domain's functions, typed. */
    val money: Money = Money(peer)

    private val bank: DemoBank? = if (demo) DemoBank() else null
    private val _state = MutableStateFlow(Ledger(today = today, month = today.take(7), demo = demo, server = server))
    private val _notes = MutableSharedFlow<String>(extraBufferCapacity = 16)
    @Volatile private var closed = false
    private val draining = AtomicBoolean(false)

    /** What the screens draw. */
    val state: StateFlow<Ledger> = _state.asStateFlow()

    /** Sentences to show once each: a refusal, what a sync brought. */
    val notes: SharedFlow<String> = _notes.asSharedFlow()

    /** Every note said, in order: the diagnostics' and the tests'. */
    val said: MutableList<String> = CopyOnWriteArrayList()

    // Held on `run`'s thread only.
    private var month: String = today.take(7)
    private var accounts: Held<AccountLine>? = null
    private var groups: Held<GroupLine>? = null
    private var lines: Held<BudgetLine>? = null
    private var summary: Held<Summary>? = null
    private var spending: Held<SpendingLine>? = null
    private var inbox: Held<TxnLine>? = null
    private var payees: Held<PayeeLine>? = null
    private var banks: Held<BankLine>? = null
    private var linked: Held<LinkedLine>? = null
    private val registers = HashMap<String, Pair<Held<TxnLine>, Int>>()
    private var syncing = false

    private fun start() {
        accounts = Held(money.accounts())
        groups = Held(money.groups())
        openMonth()
        inbox = Held(money.inbox())
        payees = Held(money.payees())
        banks = Held(money.banks())
        linked = Held(money.linked())
        if (listen) {
            peer.setListener(
                object : Listener {
                    override fun changed() = drainSoon()
                })
        }
        drain()
    }

    private fun openMonth() {
        listOf(lines, summary, spending).forEach { it?.close() }
        val m = Month(month)
        lines = Held(money.month(m))
        summary = Held(money.summary(m))
        spending = Held(money.spending(m))
    }

    private fun post(f: () -> Unit) {
        if (closed) return
        try {
            run.execute {
                if (closed) return@execute
                f()
                drain()
            }
        } catch (e: RejectedExecutionException) {
            // Closed: nothing to do.
        }
    }

    private fun drainSoon() {
        if (closed || draining.getAndSet(true)) return
        try {
            run.execute { drain() }
        } catch (e: RejectedExecutionException) {
            // Closed.
        }
    }

    // On `run`'s thread: every view settled, the server's refusals said, one Ledger published.
    private fun drain() {
        draining.set(false)
        if (closed) return
        for (h in listOf(accounts, groups, lines, summary, spending, inbox, payees, banks, linked)) h?.settle()
        for ((h, _) in registers.values) h.settle()
        for (r in peer.takeRejections()) say("The server didn't take a change: ${r.why}")
        val s = peer.status()
        // The demo is Alex's, as the demo window is, with no session row to say so.
        val who = peer.signedIn()?.let { it.name.ifEmpty { it.user } } ?: if (demo) "alex" else ""
        _state.value =
            Ledger(
                today = today,
                month = month,
                summary = summary?.rows?.firstOrNull(),
                lines = lines?.rows?.toList().orEmpty(),
                groups = groups?.rows?.toList().orEmpty(),
                accounts = accounts?.rows?.toList().orEmpty(),
                inbox = inbox?.rows?.toList().orEmpty(),
                registers = registers.mapValues { it.value.first.rows.toList() },
                payees = payees?.rows?.toList().orEmpty(),
                spending = spending?.rows?.toList().orEmpty(),
                banks = banks?.rows?.toList().orEmpty(),
                linked = linked?.rows?.toList().orEmpty(),
                link = Link(s.linked, if (s.alone) "alone" else s.link, s.pending.toInt()),
                who = who.replaceFirstChar { it.uppercase() },
                demo = demo,
                server = server,
                syncing = syncing,
                ready = true,
                denied = s.denied,
            )
    }

    private fun say(note: String) {
        said.add(note)
        _notes.tryEmit(note)
    }

    // A change by the domain's function: its refusal said in its sentence.
    private fun act(what: String, f: (Money) -> Unit) = post {
        try {
            f(money)
        } catch (e: Refusal) {
            say("Couldn't $what: ${why(e)}")
        }
    }

    /**
     * The domain's sentence for the first thing wrong with `input` to the function `name`, or null:
     * what a form says under its field before anything is sent.
     */
    fun check(name: String, input: String): String? = peer.check(name, input)

    // * The month

    /** The budget shows `month` (`2026-10`). */
    fun showMonth(m: String) = post {
        if (m != month) {
            month = m
            openMonth()
        }
    }

    /** `n` months later (or earlier); zero is this month. */
    fun stepMonth(n: Int) = post {
        month = if (n == 0) today.take(7) else moneyAddMonths(month, n.toLong())
        openMonth()
    }

    fun assign(category: Id, amount: Long) =
        act("assign it") { it.assign(Assign(month, category, amount)) }

    /** Last month's amounts, each target's shortfall, or nothing — for `only` these, or every one. */
    fun autoAssign(how: Auto, only: List<Id> = emptyList()) =
        act("assign the month") { it.autoAssign(AutoAssign(month, how, only)) }

    /** `amount` from one category to another; none is Ready to Assign. */
    fun moveMoney(from: Id?, to: Id?, amount: Long) =
        act("move the money") { it.moveMoney(MoveMoney(month, from, to, amount)) }

    /** An overspent category made whole from `from` (none: Ready to Assign). */
    fun cover(category: Id, from: Id?) = post {
        val short = -(lines?.rows?.firstOrNull { it.categoryId == category }?.available ?: 0)
        if (short <= 0) return@post
        try {
            money.moveMoney(MoveMoney(month, from, category, short))
        } catch (e: Refusal) {
            say("Couldn't cover it: ${why(e)}")
        }
    }

    fun setTarget(category: Id, target: Long) =
        act("set the target") { it.setTarget(SetTarget(category, target.coerceAtLeast(0))) }

    fun addCategory(group: Id, name: String) = act("add the category") { it.addCategory(AddCategory(group, name)) }

    fun addGroup(name: String) = act("add the group") { it.addGroup(AddGroup(name)) }

    fun renameCategory(category: Id, name: String) =
        act("rename it") { it.renameCategory(RenameCategory(category, name)) }

    fun hideCategory(category: Id, hidden: Boolean) =
        act("hide it") { it.hideCategory(HideCategory(category, hidden)) }

    fun deleteCategory(category: Id) =
        act("delete the category") { it.deleteCategory(DeleteCategory(category, null)) }

    /** A category into `group`, in front of `before` (one of that group's) or at its end. */
    fun moveCategory(category: Id, group: Id, before: Id?) =
        act("move the category") { it.moveCategory(MoveCategory(category, group, before)) }

    /** A group in front of `before`, or after every other. */
    fun moveGroup(group: Id, before: Id?) = act("move the group") { it.moveGroup(MoveGroup(group, before)) }

    // * Transactions

    /** A new transaction, or the one corrected. */
    fun save(f: TxnForm) =
        act("save the transaction") { m ->
            val editing = f.editing
            if (editing != null) {
                m.editTransaction(EditTransaction(editing, f.date, f.payee, f.category, f.memo, f.amount))
            } else {
                val account = f.account ?: throw Refusal.Refused("which account?")
                m.addTransaction(
                    AddTransaction(
                        account,
                        f.date,
                        if (f.transferTo != null) "" else f.payee,
                        if (f.transferTo != null) null else f.category,
                        f.memo,
                        f.amount,
                        f.cleared,
                        f.transferTo))
            }
        }

    fun delete(ids: List<Id>) = act("delete it") { it.deleteTransactions(Txns(ids)) }

    fun approve(ids: List<Id>) = act("approve it") { it.approve(Txns(ids)) }

    fun categorize(ids: List<Id>, category: Id?) = act("file it") { it.categorize(Categorize(ids, category)) }

    fun setCleared(id: Id, cleared: Boolean) = act("clear it") { it.setCleared(SetCleared(id, cleared)) }

    /** The account agreed with the bank at `balance`, today; a difference becomes an adjustment. */
    fun reconcile(account: Id, balance: Long) =
        act("reconcile") {
            it.reconcile(Reconcile(account, balance, today))
            say("Reconciled")
        }

    // * Accounts and the bank

    fun openAccount(name: String, kind: String, onBudget: Boolean, balance: Long) =
        act("add the account") { it.openAccount(OpenAccount(name, kind, onBudget, balance, today)) }

    /** The bank's account `plaid` feeds `account` from now on. */
    fun link(plaid: String, account: Id) = act("link it") { it.link(LinkAccount(plaid, account)) }

    fun ignore(plaid: String, ignored: Boolean) = act("change it") { it.ignore(IgnoreLinked(plaid, ignored)) }

    /**
     * Ask the bank for anything new. The demo's bank answers at once; a server's importer is asked
     * (`banks.sync`) and its import arrives as any change does, and the link is dialled now.
     */
    fun sync() {
        post {
            syncing = true
        }
        post {
            try {
                val b = bank
                if (b != null) {
                    say(b.sync(peer, today).note)
                } else {
                    money.sync()
                    peer.dialNow()
                    say("Asked the bank for anything new")
                }
            } catch (e: Refusal) {
                say("Couldn't sync: ${why(e)}")
            }
            syncing = false
        }
    }

    /** Hold `account`'s register (none: every account's) while a screen shows it. */
    fun holdRegister(account: Id?) = post {
        val k = Ledger.key(account)
        val had = registers[k]
        registers[k] =
            if (had != null) had.first to had.second + 1 else Held(money.register(Register(account))) to 1
    }

    fun releaseRegister(account: Id?) = post {
        val k = Ledger.key(account)
        val had = registers[k] ?: return@post
        if (had.second <= 1) {
            registers.remove(k)
            had.first.close()
        } else registers[k] = had.first to had.second - 1
    }

    /** Dial the server now: the app came to the front. */
    fun dialNow() {
        if (!demo) post { peer.dialNow() }
    }

    /** Signing out: the server told, the replica kept for the next sign-in. */
    fun logout() = post { if (!demo) peer.logout() }

    /** The denial the server gave this login, if it gave one. */
    fun denied(): String? = peer.status().denied

    override fun close() {
        if (closed) return
        val shut = {
            closed = true
            peer.setListener(null)
            for (h in listOf(accounts, groups, lines, summary, spending, inbox, payees, banks, linked)) h?.close()
            for ((h, _) in registers.values) h.close()
            registers.clear()
            peer.close()
        }
        try {
            run.execute { shut() }
        } catch (e: RejectedExecutionException) {
            closed = true
        }
        owned?.shutdown()
    }

    companion object {
        /** The budget's own category, where money that comes in goes: the same id on every peer. */
        val READY_TO_ASSIGN: Id = Id(fixed("ready to assign"))

        private fun fixed(what: String): String =
            MessageDigest.getInstance("SHA-256")
                .digest("money $what".toByteArray(Charsets.UTF_8))
                .take(16)
                .joinToString("") { "%02x".format(it) }

        /** The domain's sentence in a refusal. */
        fun why(e: Refusal): String = (e as? Refusal.Refused)?.why ?: e.message ?: "no reason given"

        /**
         * A book over `peer` on a thread of its own, told by the peer when anything moves. Returns at
         * once; the views are read on that thread first.
         */
        fun open(peer: Peer, today: String, demo: Boolean, server: String): Book {
            val pool = Executors.newSingleThreadExecutor { r -> Thread(r, "money-book").apply { isDaemon = true } }
            return Book(peer, today, demo, server, pool, pool, listen = true).also { b -> pool.execute { b.start() } }
        }

        /**
         * A book whose every read and change happens at once on the caller's thread, not told by the
         * peer: a test's, which sees each change's effect as soon as it returns.
         */
        fun inline(peer: Peer, today: String, demo: Boolean = true, server: String = ""): Book =
            Book(peer, today, demo, server, Executor { it.run() }, null, listen = false).also { it.start() }
    }
}
