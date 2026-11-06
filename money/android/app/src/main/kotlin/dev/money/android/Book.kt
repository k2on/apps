// The household's budget as the screens see it: money's views over a peer —
// the Rust kernel through the bridge, a replica of the server's or the
// demo's household — kept in step with it, and every change the screens
// make, as one of the domain's functions.
//
// **The views are the core's** (`Session`, money/ffi): it holds each one by
// count, keeps its rows spliced on the kernel's thread, says which moved
// (`Changed`), and a page is what the domain says it is made of — the
// budget for a month is its lines, summary and spending. The book holds
// what every screen draws from as long as it is open, and the month's
// budget for the month on screen; a screen holds the rest while it shows
// them (`watched.watch(view) { read }`, arkui's): a register, a receipt's
// items.
//
// **Everything else runs on one thread** (`run`): every mutation, and the
// drain after it or after the core said something moved, which reads again
// the views whose version moved and publishes one [Ledger] — an immutable
// snapshot the screens draw from (`state`, a StateFlow). A peer at rest
// runs nothing: there is no clock here.
//
// **A refusal is said, not thrown.** The domain's own sentence for why a
// change was not taken — refused here, or by the server later — is a note
// (`notes`), which the screens show as a snackbar. A form asks first
// ([check]) and says the sentence under its field.
package dev.money.android

import dev.arkdb.ffi.Id
import dev.arkdb.ffi.J
import dev.arkdb.ffi.Listener
import dev.arkdb.ffi.Peer
import dev.arkdb.ffi.Refusal
import dev.money.android.gen.AddCategory
import dev.money.android.gen.AddPhoto
import dev.money.android.gen.FileRef
import dev.money.android.gen.ItemLine
import dev.money.android.gen.MatchReceipt
import dev.money.android.gen.PhotoLine
import dev.money.android.gen.ReceiptLine
import dev.money.android.gen.WriteReceipt
import dev.money.android.gen.AddGroup
import dev.money.android.gen.AddTransaction
import dev.money.android.gen.Assign
import dev.money.android.gen.Auto
import dev.money.android.gen.AutoAssign
import dev.money.android.gen.BankName
import dev.money.android.gen.Categorize
import dev.money.android.gen.DeleteCategory
import dev.money.android.gen.EditTransaction
import dev.money.android.gen.HideCategory
import dev.money.android.gen.IgnoreLinked
import dev.money.android.gen.LinkAccount
import dev.money.android.gen.Money
import dev.money.android.gen.MoveCategory
import dev.money.android.gen.MoveGroup
import dev.money.android.gen.MoveMoney
import dev.money.android.gen.OpenLinked
import dev.money.android.gen.OpenAccount
import dev.money.android.gen.Reconcile
import dev.money.android.gen.RenameCategory
import dev.money.android.gen.SetCleared
import dev.money.android.gen.SetTarget
import dev.money.android.gen.Txns
import dev.money.ffi.Changed
import dev.money.ffi.DemoBank
import dev.money.ffi.Holding
import dev.money.ffi.MoneyView
import dev.money.ffi.Session
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
import uniffi.money_domain.AskLine
import uniffi.money_domain.AskState
import uniffi.money_domain.BankLine
import uniffi.money_domain.BudgetLine
import uniffi.money_domain.GroupLine
import uniffi.money_domain.LinkedLine
import uniffi.money_domain.PayeeLine
import uniffi.money_domain.SpendingLine
import uniffi.money_domain.Summary
import uniffi.money_domain.TxnLine
import dev.arkui.Feedback
import dev.arkui.Watched
import kotlinx.coroutines.flow.update

/** What the hand last felt of the book's changes, and how many there have been (`Book.felt`). */
data class Felt(val count: Int, val kind: Feedback)

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
    val payees: List<PayeeLine> = emptyList(),
    val spending: List<SpendingLine> = emptyList(),
    val banks: List<BankLine> = emptyList(),
    val linked: List<LinkedLine> = emptyList(),
    /** This person's asks of the server's importer (`banks.asks`), the newest first. */
    val asks: List<AskLine> = emptyList(),
    val link: Link = Link(),
    /** Who is signed in, as a name to show. */
    val who: String = "",
    /** Their picture, as this phone loads it ([Faces.url]); empty for none, and for the demo. */
    val picture: String = "",
    val demo: Boolean = false,
    /** The server's address; empty for the demo. */
    val server: String = "",
    /** A sync of the bank is under way (pull to refresh). */
    val syncing: Boolean = false,
    /** The views have been read once: the first screen has what it draws. */
    val ready: Boolean = false,
    /** Why the server turned this login away, when it did. */
    val denied: String? = null,
    /** Every receipt photo, the newest first, and where reading it stands. */
    val photos: List<PhotoLine> = emptyList(),
    /** Every receipt, the newest first. */
    val receipts: List<ReceiptLine> = emptyList(),
) {
    fun line(category: Id): BudgetLine? = lines.firstOrNull { it.categoryId == category }

    fun account(id: Id): AccountLine? = accounts.firstOrNull { it.id == id }

    /**
     * The receipts that paid for `txn` (`receipt` the one its line names) — whole, or by one of their
     * charges — and those read from its photos.
     */
    fun receiptsOf(txn: Id, receipt: String): List<ReceiptLine> {
        val read = photos.filter { it.txnId == txn && it.receiptId.isNotEmpty() }.map { it.receiptId }.toSet()
        return receipts.filter { it.txnId == txn || (receipt.isNotEmpty() && it.id == receipt) || it.id in read }
    }

    /** The photos taken for `txn`, the newest first. */
    fun photosOf(txn: Id): List<PhotoLine> = photos.filter { it.txnId == txn }

    /** The photo a receipt was read from, when it was. */
    fun photoOf(receipt: String): PhotoLine? = photos.firstOrNull { it.receiptId == receipt }

    /** The receipts nothing pays for yet, for somebody to say which transaction did. */
    val unmatched: List<ReceiptLine>
        get() = receipts.filter { it.txnId == null && it.charges == 0L }

    /** The month's categories, by group in the budget's order, the hidden left out unless `hidden`. */
    fun byGroup(hidden: Boolean = false): List<Pair<GroupLine, List<BudgetLine>>> =
        groups
            .filter { hidden || !it.hidden }
            .sortedBy { it.pos }
            .map { g -> g to lines.filter { it.groupId == g.id && (hidden || !it.hidden) }.sortedBy { it.pos } }

    /** The cards' payment categories this month. */
    val cards: List<BudgetLine>
        get() = lines.filter { isPayment(it) }

    /** What the cards owe beyond what is set aside for them. */
    val cardsShort: Long
        get() = cards.filter { !it.hidden }.sumOf { it.underfunded }

    /** What wants somebody in Settings: a bank whose login ran out, or an account waiting for a choice. */
    val banksWanting: Int
        get() = banks.sumOf { it.waiting.toInt() + if (it.needsLogin) 1 else 0 }

    companion object {
        /**
         * Whether `l` is a card's payment category: one of the cards' own group, which is the domain's
         * and changes as the cards do — never renamed, deleted, moved out, or a transaction's category.
         */
        fun isPayment(l: BudgetLine): Boolean = l.card != null || l.groupId == Book.CARD_GROUP
    }
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
    /** The receipt its line says paid for the one corrected (`TxnLine.receipt`), blank for none. */
    val receipt: String = "",
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

    /** The views, held and kept by the core. */
    val session: Session = Session(peer)

    /** The views as a screen holds them while it shows them (arkui's `watch`). */
    val watched: Watched<MoneyView> = Watched({ session.hold(it) })

    private val bank: DemoBank? = if (demo) DemoBank() else null
    private val _state = MutableStateFlow(Ledger(today = today, month = today.take(7), demo = demo, server = server))
    private val _notes = MutableSharedFlow<String>(extraBufferCapacity = 16)
    @Volatile private var closed = false
    private val draining = AtomicBoolean(false)

    /** What the screens draw. */
    val state: StateFlow<Ledger> = _state.asStateFlow()

    /** Sentences to show once each: a refusal, what a sync brought. */
    val notes: SharedFlow<String> = _notes.asSharedFlow()

    /**
     * The last change the hand should feel, and how many there have been: bumped once the domain
     * has taken a change the iPhone answers with `.sensoryFeedback` — saved, approved, assigned,
     * moved, reconciled, a photo kept (a success); cleared (a selection) — and never for one it
     * refused. The app's root performs it (`SensoryFeedback`) as the change is composed.
     */
    val felt: StateFlow<Felt> get() = _felt
    private val _felt = MutableStateFlow(Felt(0, Feedback.Success))

    /** Every note said, in order: the diagnostics' and the tests'. */
    val said: MutableList<String> = CopyOnWriteArrayList()

    private val _pages = MutableSharedFlow<String>(extraBufferCapacity = 4)

    /** Plaid's pages to open, each once: the answer to a connection asked for here (Banks.kt). */
    val pages: SharedFlow<String> = _pages.asSharedFlow()

    /** Every page asked to be opened, in order: the tests'. */
    val opened: MutableList<String> = CopyOnWriteArrayList()

    // Held on `run`'s thread only.
    private var month: String = today.take(7)
    /** What every screen draws from, held while the book is open. */
    private val always = ArrayList<Holding>()
    /** The month on screen's budget. */
    private var budget: Holding? = null
    /** Each view's version when it was last read into the ledger. */
    private val seen = HashMap<MoneyView, ULong>()
    private var syncing = false

    /**
     * A page asked for here and not yet opened: the newest ask there was before it (none: there was
     * none), so the drain knows the server's answer to this one when it arrives.
     */
    private var awaiting: Awaiting? = null

    private class Awaiting(val after: Id?)

    private fun start() {
        for (v in ALWAYS) always.add(session.hold(v))
        budget = session.hold(MoneyView.Budget(month))
        if (listen) {
            peer.setListener(
                object : Listener {
                    override fun changed() = drainSoon()
                })
            session.setListener(
                object : Changed {
                    override fun moved(views: List<MoneyView>) {
                        watched.moved(views)
                        drainSoon()
                    }
                })
        }
        drain()
    }

    private fun showBudget() {
        val was = budget
        budget = session.hold(MoneyView.Budget(month))
        was?.close()
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

    // On `run`'s thread: what moved read again, the server's refusals said, one Ledger published.
    private fun drain() {
        draining.set(false)
        if (closed) return
        for (r in peer.takeRejections()) say("The server didn't take a change: ${r.why}")
        val was = _state.value
        val b = MoneyView.Budget(month)
        val got = if (fresh(b) || was.month != month) session.budget(month) else null
        val asks = if (fresh(MoneyView.Asks)) session.asks() else was.asks
        awaiting?.let { w ->
            when (val f = follow(asks, w.after)) {
                is Follow.Open -> {
                    awaiting = null
                    browse(f.url)
                }
                Follow.Over -> awaiting = null
                Follow.Wait -> {}
            }
        }
        val s = peer.status()
        // The demo is Alex's, as the demo window is, with no session row to say so.
        val signed = peer.signedIn()
        val who = signed?.let { it.name.ifEmpty { it.user } } ?: if (demo) "alex" else ""
        _state.value =
            Ledger(
                today = today,
                month = month,
                summary = if (got != null) got.summary else was.summary,
                lines = got?.lines ?: was.lines,
                groups = if (fresh(MoneyView.Groups)) session.groups() else was.groups,
                accounts = if (fresh(MoneyView.Accounts)) session.accounts() else was.accounts,
                inbox = if (fresh(MoneyView.Inbox)) session.inbox() else was.inbox,
                payees = if (fresh(MoneyView.Payees)) session.payees() else was.payees,
                spending = got?.spending ?: was.spending,
                banks = if (fresh(MoneyView.Banks)) session.banks() else was.banks,
                linked = if (fresh(MoneyView.Linked)) session.linked() else was.linked,
                asks = asks,
                link = Link(s.linked, if (s.alone) "alone" else s.link, s.pending.toInt()),
                who = who.replaceFirstChar { it.uppercase() },
                picture = signed?.let { Faces.url(server, it.user, it.picture) }.orEmpty(),
                demo = demo,
                server = server,
                syncing = syncing,
                ready = true,
                denied = s.denied,
                photos = if (fresh(MoneyView.Photos)) session.photos().map { PhotoLine.of(J.parse(it)) } else was.photos,
                receipts = if (fresh(MoneyView.Receipts)) session.receipts().map { ReceiptLine.of(J.parse(it)) } else was.receipts,
            )
    }

    // Whether `v` moved since it was last read here; it counts as read from now.
    private fun fresh(v: MoneyView): Boolean {
        val now = session.version(v)
        return seen.put(v, now) != now
    }

    /** A sentence for the snackbar, from a screen. */
    fun tell(note: String) = say(note)

    private fun say(note: String) {
        said.add(note)
        _notes.tryEmit(note)
    }

    // A change by the domain's function: its refusal said in its sentence, and what the hand feels
    // of it, if anything, once it is taken.
    private fun act(what: String, feel: Feedback? = null, f: (Money) -> Unit) = post {
        try {
            f(money)
            if (feel != null) _felt.update { Felt(it.count + 1, feel) }
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
            showBudget()
        }
    }

    /** `n` months later (or earlier); zero is this month. */
    fun stepMonth(n: Int) = post {
        month = if (n == 0) today.take(7) else moneyAddMonths(month, n.toLong())
        showBudget()
    }

    fun assign(category: Id, amount: Long) =
        act("assign it", Feedback.Success) { it.assign(Assign(month, category, amount)) }

    /** Last month's amounts, each target's shortfall, or nothing — for `only` these, or every one. */
    fun autoAssign(how: Auto, only: List<Id> = emptyList()) =
        act("assign the month", Feedback.Success) { it.autoAssign(AutoAssign(month, how, only)) }

    /** `amount` from one category to another; none is Ready to Assign. */
    fun moveMoney(from: Id?, to: Id?, amount: Long) =
        act("move the money", Feedback.Success) { it.moveMoney(MoveMoney(month, from, to, amount)) }

    /** An overspent category made whole from `from` (none: Ready to Assign). */
    fun cover(category: Id, from: Id?) = post {
        val short = -(session.budget(month).lines.firstOrNull { it.categoryId == category }?.available ?: 0)
        if (short <= 0) return@post
        try {
            money.moveMoney(MoveMoney(month, from, category, short))
        } catch (e: Refusal) {
            say("Couldn't cover it: ${why(e)}")
        }
    }

    /**
     * A card's payment category given what the card owes beyond what it has: its debt covered, from
     * Ready to Assign, as the desktop's "Assign $X to cover the debt" does.
     */
    fun coverCard(category: Id) = post {
        val l = session.budget(month).lines.firstOrNull { it.categoryId == category } ?: return@post
        if (l.underfunded <= 0) return@post
        try {
            money.assign(Assign(month, category, l.assigned + l.underfunded))
        } catch (e: Refusal) {
            say("Couldn't cover the card: ${why(e)}")
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

    /** A new transaction, or the one corrected; one corrected into a transfer becomes one (`transfer_to`). */
    fun save(f: TxnForm) =
        act("save the transaction", Feedback.Success) { m ->
            val editing = f.editing
            if (editing != null) {
                m.editTransaction(
                    EditTransaction(editing, f.date, if (f.transferTo != null) "" else f.payee, f.category, f.memo, f.amount, f.transferTo))
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

    fun approve(ids: List<Id>) = act("approve it", Feedback.Success) { it.approve(Txns(ids)) }

    fun categorize(ids: List<Id>, category: Id?) = act("file it") { it.categorize(Categorize(ids, category)) }

    fun setCleared(id: Id, cleared: Boolean) =
        act("clear it", Feedback.Selection) { it.setCleared(SetCleared(id, cleared)) }

    /** The account agreed with the bank at `balance`, today; a difference becomes an adjustment. */
    fun reconcile(account: Id, balance: Long) =
        act("reconcile", Feedback.Success) {
            it.reconcile(Reconcile(account, balance, today))
            say("Reconciled")
        }

    // * Accounts and the bank

    fun openAccount(name: String, kind: String, onBudget: Boolean, balance: Long) =
        act("add the account") { it.openAccount(OpenAccount(name, kind, onBudget, balance, today)) }

    /** The bank's account `plaid` feeds `account` from now on. */
    fun link(plaid: String, account: Id) = act("link it") { it.link(LinkAccount(plaid, account)) }

    fun ignore(plaid: String, ignored: Boolean) = act("change it") { it.ignore(IgnoreLinked(plaid, ignored)) }

    /** The bank's account `plaid` given an account of its own, of its kind (a card a credit account). */
    fun openLinked(plaid: String) =
        act("open an account for it") {
            it.open(OpenLinked(plaid, ""))
            say("Opened. Its history arrives with the bank's next import")
        }

    /** Ask the server to connect a bank: its page opens when the server answers with it ([pages]). */
    fun connect() = askPage("connect a bank") { it.connect() }

    /** Ask the server for the page to log in to `bank` again. */
    fun reconnect(bank: String) = askPage("reconnect the bank") { it.reconnect(BankName(bank)) }

    /** Ask the server to forget `bank`. Its accounts here, and what is in them, stay. */
    fun disconnect(bank: String) = post {
        if (demo) return@post say("The demo has no server to disconnect a bank from")
        try {
            money.disconnect(BankName(bank))
            say("Asked the server to disconnect the bank")
        } catch (e: Refusal) {
            say("Couldn't disconnect the bank: ${why(e)}")
        }
    }

    // An ask whose answer is a page to open: the newest ask before it kept, so the drain knows its answer.
    private fun askPage(what: String, f: (Money) -> Unit) = post {
        if (demo) return@post say("The demo has no server to connect a bank through")
        val after = session.asks().firstOrNull()?.id
        try {
            f(money)
            awaiting = Awaiting(after)
            say("Asked the server for Plaid's page")
        } catch (e: Refusal) {
            say("Couldn't $what: ${why(e)}")
        }
    }

    /** Plaid's page opened: when the server's answer arrives, or again from Settings. */
    fun browse(url: String) {
        opened.add(url)
        _pages.tryEmit(url)
    }

    /**
     * A change by the domain's function as it is, said as `what` when refused: what the server's
     * importer does, which a test stands in for (the demo holds its role).
     */
    internal fun change(what: String, f: (Money) -> Unit) = act(what, f = f)

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

    // * Receipts

    /** A receipt's items, as far as a screen holds them (`MoneyView.Items`). */
    fun itemsOf(receipt: String): List<ItemLine> = session.items(receipt).map { ItemLine.of(J.parse(it)) }

    /**
     * A photo of a receipt, for `txn` when it was taken for one: kept on this phone at once, sent to the
     * server now or on its next connection, where it is read.
     */
    fun addPhoto(jpeg: ByteArray, txn: Id?, media: String = "image/jpeg") =
        act("keep the photo", Feedback.Success) { m ->
            val hash = peer.putFile(jpeg, media)
            m.addPhoto(AddPhoto(FileRef(hash, jpeg.size.toLong(), media), txn))
            say(if (state.value.link.linked) "Photo kept: reading it" else "Photo kept: it's read once you're online")
        }

    /** A receipt typed by hand, or one already read put right. */
    fun writeReceipt(w: WriteReceipt) = act("save the receipt") { it.writeReceipt(w) }

    /** `receipt` paid for by `txn`, or (none) by nothing. */
    fun pay(receipt: String, txn: Id?) =
        act(if (txn == null) "take the receipt off" else "attach the receipt") { it.pay(MatchReceipt(receipt, txn)) }

    /**
     * A photo's bytes when this phone holds them; when not, fetched from the server, and the state moves
     * when they arrive. Safe from any thread: the peer guards itself.
     */
    fun photo(f: FileRef): ByteArray? = if (closed) null else peer.file(f.hash, f.size, f.media)

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
            session.setListener(null)
            for (h in always) h.close()
            always.clear()
            budget?.close()
            budget = null
            session.close()
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
        /** What every screen draws from: held as long as the book is open. */
        private val ALWAYS =
            listOf(
                MoneyView.Accounts,
                MoneyView.Groups,
                MoneyView.Inbox,
                MoneyView.Payees,
                MoneyView.Banks,
                MoneyView.Linked,
                MoneyView.Asks,
                MoneyView.Photos,
                MoneyView.Receipts,
            )

        /** The budget's own category, where money that comes in goes: the same id on every peer. */
        val READY_TO_ASSIGN: Id = Id(fixed("ready to assign"))

        /**
         * The cards' own group, "Credit Card Payments", first in the budget: a card's payment category
         * for each card on the budget, and nothing else (`money/domain/src/cards.rs`).
         */
        val CARD_GROUP: Id = Id(fixed("credit card payments"))

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
