// The relay: the watch's upstream link, piped through the phone to a
// WebSocket. It reads no frame: the watch's frames go up as the bytes they
// are, and the upstream's come down cut to the watch's inbox. Only the
// server judges entries; the phone holds nothing. This is the fallback of
// docs/north-star.md, "Upstreams" (harken's Android app is the upstream
// proper, answering the watch from its own view).
//
// Each WebSocket the script opens is a connection upstream, numbered by an
// epoch it tells the watch (LINK); every chunk either way carries it, so
// nothing of a connection that has ended is read as part of the next. When
// the socket closes the watch is told (LINK 0) and the script connects
// again a few seconds later. The watch says what it is (READY: its inbox
// size and origin) at its start; the script asks again (HELLO) at its own.
//
// The upstream's address is the localStorage key `upstream`, by default the
// emulator's host: ws://127.0.0.1:9010/.

var UPSTREAM = localStorage.getItem('upstream') || 'ws://127.0.0.1:9010/';
var RECONNECT_MS = 3000;
// The dictionary's own bytes in a message: a count, two tuple headers, the
// epoch, and some slack.
var OVERHEAD = 32;

var ws = null;
var epoch = 0;
var nextEpoch = (Date.now() % 1000000) + 1;
var inbox = 0;
var origin = '';
var reconnect = null;

// AppMessages go one at a time; the next waits for the last's ack.
var queue = [];
var sending = false;

function log(text) {
  console.log('relay: ' + text);
}

function send(msg) {
  queue.push({ msg: msg, tries: 0 });
  pump();
}

function pump() {
  if (sending || queue.length === 0) return;
  sending = true;
  var item = queue[0];
  Pebble.sendAppMessage(item.msg, function () {
    queue.shift();
    sending = false;
    pump();
  }, function (e) {
    sending = false;
    item.tries++;
    if (item.tries >= 8) {
      queue.shift();
      // A chunk lost breaks the stream: start the connection again.
      if (item.msg.DATA !== undefined && ws) {
        log('a chunk was refused 8 times; connecting again');
        ws.close();
      }
    }
    setTimeout(pump, 100 * item.tries);
  });
}

// Drops the chunks still queued for the watch: their connection ended.
function dropChunks() {
  var head = sending ? queue.slice(0, 1) : [];
  queue = head.concat(queue.slice(head.length).filter(function (item) {
    return item.msg.DATA === undefined;
  }));
}

function connect() {
  if (reconnect) {
    clearTimeout(reconnect);
    reconnect = null;
  }
  if (ws) {
    var old = ws;
    ws = null;
    old.close();
  }
  epoch = 0;
  dropChunks();
  var url = UPSTREAM + '?origin=' + origin;
  var sock;
  try {
    sock = new WebSocket(url);
  } catch (err) {
    log('could not open ' + url + ': ' + err);
    later();
    return;
  }
  sock.binaryType = 'arraybuffer';
  ws = sock;
  sock.onopen = function () {
    if (ws !== sock) return;
    epoch = nextEpoch++;
    log('connected to ' + UPSTREAM + ', epoch ' + epoch);
    send({ LINK: epoch });
  };
  sock.onmessage = function (ev) {
    if (ws !== sock || typeof ev.data === 'string') return;
    var bytes = new Uint8Array(ev.data);
    var size = Math.max(16, inbox - OVERHEAD);
    for (var i = 0; i < bytes.length; i += size) {
      send({ EPOCH: epoch, DATA: Array.prototype.slice.call(bytes.subarray(i, i + size)) });
    }
  };
  sock.onclose = function () {
    if (ws !== sock) return;
    ws = null;
    epoch = 0;
    dropChunks();
    log('upstream closed');
    send({ LINK: 0 });
    later();
  };
  sock.onerror = function () {
    log('upstream error');
  };
}

function later() {
  if (!reconnect && inbox) {
    reconnect = setTimeout(function () {
      reconnect = null;
      connect();
    }, RECONNECT_MS);
  }
}

Pebble.addEventListener('ready', function () {
  log('ready');
  send({ HELLO: 1 });
});

Pebble.addEventListener('appmessage', function (e) {
  var p = e.payload;
  if (p.READY) {
    // The watch (re)started: a fresh connection, so the upstream's record
    // of it starts from the watch's hello.
    inbox = p.INBOX;
    origin = p.ORIGIN || '';
    log('watch ' + origin + ', inbox ' + inbox);
    connect();
    return;
  }
  if (p.DATA !== undefined && p.EPOCH === epoch && ws && ws.readyState === 1) {
    ws.send(new Uint8Array(p.DATA).buffer);
  }
});
