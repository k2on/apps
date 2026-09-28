// The page: a header that opens a peer, three panes that read it, a status
// line, and the two things the wasm cannot do for itself — the WebSocket
// and IndexedDB. No framework; the DOM is rebuilt per pane when the peer
// says its view moved, which at a demo's scale is nothing.
//
// Every URL here is relative, so the same files serve at `/` on a laptop
// and under `/apps/harken/` on GitHub Pages.

import init, { Peer, scopes } from "./harken_web.js";

// The rows as `query` returns them: an id is its 8-4-4-4-12 text, an int a
// number (harken's fit in a double).
type Track = { id: string; title: string; artist: string; album: string | null; duration_ms: number; file: string };
type Playlist = { id: string; name: string; user_id: string };
type Item = { playlist_id: string; track_id: string; pos: number };
type Status = {
  user: string;
  server: string | null;
  alone: boolean;
  linked: boolean;
  denied: string | null;
  cursors: Record<string, number>;
  pending: number;
  diverged: number;
  lastRefusal: string | null;
  module: string;
};

const DEMO: [string, string, string, number][] = [
  ["Air", "Johann Sebastian Bach", "Orchestral Suite No. 3", 312],
  ["Gigue", "Johann Sebastian Bach", "Orchestral Suite No. 3", 182],
  ["Hornpipe", "George Frideric Handel", "Water Music", 213],
  ["La Primavera: I. Allegro", "Antonio Vivaldi", "The Four Seasons", 205],
  ["Gymnopédie No. 1", "Erik Satie", "Gymnopédies", 185],
];

const FIRST_BACKOFF = 500;
const LAST_BACKOFF = 30_000;
const TICK = 1000;

// -- IndexedDB: one store, keyed `${user}@${where}/${scope}`, holding what
// `persist` hands out. Read whole before a peer opens, written after every
// step that moved something. `where` is the server's URL or `alone`: a peer
// alone sequences its own log, and those cursors mean nothing to a server,
// so the same user alone and against a server are two replicas.

const DB = "harken-web";
const STORE = "scopes";

function storeKey(user: string, server: string | null, scope: string): string {
  return `${user}@${server ?? "alone"}/${scope}`;
}

function openDb(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    const req = indexedDB.open(DB, 1);
    req.onupgradeneeded = () => req.result.createObjectStore(STORE);
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
}

function idbGet(db: IDBDatabase, key: string): Promise<Uint8Array | undefined> {
  return new Promise((resolve, reject) => {
    const req = db.transaction(STORE).objectStore(STORE).get(key);
    req.onsuccess = () => resolve(req.result as Uint8Array | undefined);
    req.onerror = () => reject(req.error);
  });
}

function idbPut(db: IDBDatabase, key: string, bytes: Uint8Array): Promise<void> {
  return new Promise((resolve, reject) => {
    const tx = db.transaction(STORE, "readwrite");
    tx.objectStore(STORE).put(bytes, key);
    tx.oncomplete = () => resolve();
    tx.onerror = () => reject(tx.error);
  });
}

// -- the DOM ---------------------------------------------------------------

function $<T extends Element>(sel: string): T {
  const e = document.querySelector<T>(sel);
  if (!e) throw new Error(`no element ${sel}`);
  return e;
}

function el<K extends keyof HTMLElementTagNameMap>(tag: K, text?: string, cls?: string): HTMLElementTagNameMap[K] {
  const e = document.createElement(tag);
  if (text !== undefined) e.textContent = text;
  if (cls) e.className = cls;
  return e;
}

function clock(ms: number): string {
  if (!ms) return "";
  const s = Math.round(ms / 1000);
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

// -- the session: one peer, its socket, its store ----------------------------

class Session {
  private peer: Peer;
  private ws: WebSocket | null = null;
  private backoff = FIRST_BACKOFF;
  private timer: number | null = null;
  private ticker: number;
  private stopped = false;

  tracks: Track[] = [];
  playlists: Playlist[] = [];
  items: Item[] = [];
  selected: string | null = null;
  // A refusal is shown beside the control that earned it, and cleared by
  // the next attempt there.
  notes: Record<"library" | "playlists" | "items", string> = { library: "", playlists: "", items: "" };

  constructor(
    private db: IDBDatabase,
    readonly user: string,
    readonly server: string | null,
    saved: Map<string, Uint8Array>,
  ) {
    this.peer = Peer.open(user, server ?? undefined, (scope: string) => saved.get(scope));
    this.ticker = window.setInterval(() => {
      this.peer.tick();
      this.settle();
    }, TICK);
    if (server) this.dial();
    this.reload();
    this.settle();
  }

  // The socket, driven from here: connected, frames both ways, closed, and
  // again after a backoff from half a second to thirty.
  private dial() {
    if (this.stopped || !this.server) return;
    const ws = new WebSocket(this.server);
    ws.binaryType = "arraybuffer";
    this.ws = ws;
    ws.onopen = () => {
      this.backoff = FIRST_BACKOFF;
      this.peer.connected();
      this.settle();
    };
    ws.onmessage = (ev: MessageEvent<ArrayBuffer>) => {
      try {
        this.peer.frame(new Uint8Array(ev.data));
      } catch (e) {
        this.notes.library = `frame: ${String(e)}`;
      }
      this.settle();
    };
    ws.onclose = () => {
      if (this.ws !== ws) return;
      this.ws = null;
      this.peer.disconnected();
      this.settle();
      if (this.stopped) return;
      this.timer = window.setTimeout(() => this.dial(), this.backoff);
      this.backoff = Math.min(this.backoff * 2, LAST_BACKOFF);
    };
    ws.onerror = () => ws.close();
  }

  // After anything happened: send what the peer queued, store what moved,
  // re-read if the view changed, and redraw the status line.
  settle() {
    if (this.ws && this.ws.readyState === WebSocket.OPEN) {
      for (const frame of this.peer.takeOutgoing() as Uint8Array[]) this.ws.send(frame);
    }
    for (const [scope, bytes] of this.peer.persist() as [string, Uint8Array][]) {
      void idbPut(this.db, storeKey(this.user, this.server, scope), bytes).catch((e) => console.error("persist", e));
    }
    if (this.peer.takeChanges()) this.reload();
    renderStatus(this.status());
  }

  status(): Status {
    return JSON.parse(this.peer.status()) as Status;
  }

  private read<T>(name: string, input: Record<string, unknown> = {}): T[] {
    return JSON.parse(this.peer.query(name, JSON.stringify(input))) as T[];
  }

  reload() {
    this.tracks = this.read<Track>("library");
    this.playlists = this.read<Playlist>("playlists");
    // A selection that lost the rebase is no selection.
    if (this.selected && !this.playlists.some((p) => p.id === this.selected)) this.selected = null;
    this.items = this.selected ? this.read<Item>("playlist_items", { playlist_id: this.selected }) : [];
    render(this);
  }

  // One mutation: the verdict lands in the pane's note; the view moved (or
  // did not) and `settle` finds out.
  mutate(pane: keyof Session["notes"], name: string, input: Record<string, unknown>) {
    this.notes[pane] = "";
    try {
      const verdict = this.peer.mutate(name, JSON.stringify(input));
      if (verdict !== undefined) this.notes[pane] = `refused: ${verdict}`;
    } catch (e) {
      this.notes[pane] = String(e);
    }
    this.settle();
    render(this);
  }

  // Alone there is no scanner, so the library starts empty; these are
  // authored by intent through `add_track`, the scanner's own mutator.
  seed() {
    for (const [title, artist, album, secs] of DEMO) {
      const file = `demo/${artist}/${title}.flac`;
      this.mutate("library", "add_track", { title, artist, album, duration_ms: secs * 1000, file });
    }
  }

  select(id: string | null) {
    this.selected = id;
    this.reload();
  }

  stop() {
    this.stopped = true;
    window.clearInterval(this.ticker);
    if (this.timer !== null) window.clearTimeout(this.timer);
    // Detached first, so its `onclose` finds it is not ours and never
    // reaches a peer that has been freed.
    const ws = this.ws;
    this.ws = null;
    ws?.close();
    this.peer.free();
  }
}

// -- drawing --------------------------------------------------------------------

function render(s: Session) {
  const byId = new Map(s.tracks.map((t) => [t.id, t]));

  const lib = $<HTMLTableSectionElement>("#library tbody");
  lib.replaceChildren(
    ...s.tracks.map((t) => {
      const tr = el("tr");
      tr.append(el("td", t.title), el("td", t.artist), el("td", t.album ?? ""), el("td", clock(t.duration_ms), "num"));
      const add = el("button", "+");
      add.title = s.selected ? "add to the selected playlist" : "select a playlist first";
      add.disabled = !s.selected;
      add.onclick = () => s.mutate("library", "add_to_playlist", { playlist_id: s.selected, track_id: t.id });
      const cell = el("td");
      cell.append(add);
      tr.append(cell);
      return tr;
    }),
  );
  $("#library .note").textContent = s.notes.library;
  $<HTMLElement>("#seed").hidden = !(s.server === null && s.tracks.length === 0);
  $("#library .count").textContent = `${s.tracks.length}`;

  const list = $<HTMLUListElement>("#playlists ul");
  list.replaceChildren(
    ...s.playlists.map((p) => {
      const li = el("li", p.name);
      if (p.id === s.selected) li.className = "selected";
      li.onclick = () => s.select(p.id === s.selected ? null : p.id);
      return li;
    }),
  );
  $("#playlists .note").textContent = s.notes.playlists;

  const chosen = s.playlists.find((p) => p.id === s.selected);
  $("#items h2").textContent = chosen ? chosen.name : "Playlist";
  const items = $<HTMLTableSectionElement>("#items tbody");
  items.replaceChildren(
    ...s.items.map((it) => {
      const t = byId.get(it.track_id);
      const tr = el("tr");
      // A track in the other scope that has not arrived is drawn as
      // unavailable, not skipped: the item is real.
      tr.append(el("td", `${it.pos}`, "num"), el("td", t ? t.title : "(unavailable)", t ? "" : "dim"), el("td", t ? t.artist : ""));
      const rm = el("button", "−");
      rm.title = "remove from this playlist";
      rm.onclick = () => s.mutate("items", "remove_from_playlist", { playlist_id: it.playlist_id, track_id: it.track_id });
      const cell = el("td");
      cell.append(rm);
      tr.append(cell);
      return tr;
    }),
  );
  $("#items .note").textContent = s.notes.items;
  $<HTMLElement>("#items .empty").hidden = !chosen || s.items.length > 0;
  $<HTMLElement>("#items .none").hidden = !!chosen;
  $<HTMLElement>("#items table").hidden = !chosen;
}

function renderStatus(st: Status) {
  const link = st.alone ? "alone" : st.denied ? `denied: ${st.denied}` : st.linked ? "linked" : "offline";
  const cursors = Object.entries(st.cursors)
    .map(([scope, seq]) => `${scope}@${seq}`)
    .join("  ");
  const parts = [`${st.user}`, link, cursors, `pending ${st.pending}`];
  if (st.diverged) parts.push(`diverged ${st.diverged}`);
  if (st.lastRefusal) parts.push(`last refusal: ${st.lastRefusal}`);
  $("#status").textContent = parts.join("  ·  ");
  document.body.dataset.link = st.alone ? "alone" : st.linked ? "linked" : "offline";
}

// -- the header --------------------------------------------------------------

async function main() {
  await init();
  const db = await openDb();
  const serverBox = $<HTMLInputElement>("#server");
  const userBox = $<HTMLInputElement>("#user");
  serverBox.value = localStorage.getItem("harken.server") ?? defaultServer();
  userBox.value = localStorage.getItem("harken.user") ?? "alice";
  let session: Session | null = null;

  const open = async (server: string | null) => {
    const user = userBox.value.trim();
    if (!user) {
      userBox.focus();
      return;
    }
    session?.stop();
    localStorage.setItem("harken.user", user);
    if (server) localStorage.setItem("harken.server", server);
    const saved = new Map<string, Uint8Array>();
    for (const scope of scopes()) {
      const bytes = await idbGet(db, storeKey(user, server, scope));
      if (bytes) saved.set(scope, bytes);
    }
    session = new Session(db, user, server, saved);
    document.body.dataset.open = "yes";
  };

  $("#connect").addEventListener("click", () => void open(serverBox.value.trim() || defaultServer()));
  $("#alone").addEventListener("click", () => void open(null));
  $("#seed button").addEventListener("click", () => session?.seed());
  $("#stop").addEventListener("click", () => {
    session?.stop();
    session = null;
    delete document.body.dataset.open;
    delete document.body.dataset.link;
    for (const sel of ["#library tbody", "#playlists ul", "#items tbody"]) $(sel).replaceChildren();
    $<HTMLElement>("#seed").hidden = true;
    $("#status").textContent = "not open";
  });
  $<HTMLFormElement>("#playlists form").addEventListener("submit", (ev) => {
    ev.preventDefault();
    const box = $<HTMLInputElement>("#new-name");
    session?.mutate("playlists", "create_playlist", { name: box.value });
    if (session && !session.notes.playlists) box.value = "";
  });
}

// A server beside the page, if the page is served by one; the dev server's
// address otherwise.
function defaultServer(): string {
  const { protocol, host } = location;
  if (protocol === "http:" || protocol === "https:") {
    if (!host.endsWith("github.io")) return `${protocol === "https:" ? "wss" : "ws"}://${host}/sync`;
  }
  return "ws://127.0.0.1:8787/sync";
}

void main().catch((e) => {
  $("#status").textContent = `could not start: ${String(e)}`;
});
