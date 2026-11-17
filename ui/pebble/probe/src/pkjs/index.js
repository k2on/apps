// The probe's phone script: says hello, echoes the watch's AppMessages for
// its round-trip test, and checks what the phone's JavaScript can reach
// (XMLHttpRequest and WebSocket), sending each result to the watch.

var SRC = 1; // the JS script; a native companion says hello with 2
var CMD = { HELLO: 1, ECHO: 2, TESTS: 3, RESULT: 4 };

var XHR_URL = 'https://example.com/';
var WS_URLS = ['wss://echo.websocket.org', 'wss://ws.postman-echo.com/raw'];
var WS_HOLD_MS = 30000;
var WS_PING_MS = 5000;

// AppMessages go one at a time; the next waits for the last's ack.
var queue = [];
var sending = false;

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
    if (item.tries >= 5) {
      console.log('probe: dropped a message after 5 tries: ' + JSON.stringify(e && e.error));
      queue.shift();
    }
    setTimeout(pump, 100 * item.tries);
  });
}

function result(key, text) {
  console.log('probe: ' + text);
  var msg = { CMD: CMD.RESULT };
  msg[key] = text.substring(0, 110);
  send(msg);
}

Pebble.addEventListener('ready', function () {
  console.log('probe: ready');
  send({ CMD: CMD.HELLO, SRC: SRC });
});

Pebble.addEventListener('appmessage', function (e) {
  var p = e.payload;
  if (p.CMD === CMD.ECHO && p.TARGET === SRC) {
    var n = p.ECHO_LEN;
    var data = new Array(n);
    for (var i = 0; i < n; i++) data[i] = (p.SEQ + i) & 255;
    send({ CMD: CMD.ECHO, SEQ: p.SEQ, SRC: SRC, DATA: data });
  } else if (p.CMD === CMD.TESTS) {
    runTests();
  }
});

function runTests() {
  testXhr();
  testWebSocket(0);
}

function testXhr() {
  if (typeof XMLHttpRequest === 'undefined') {
    result('XHR', 'XHR: no XMLHttpRequest');
    return;
  }
  var t0 = Date.now();
  try {
    var req = new XMLHttpRequest();
    req.open('GET', XHR_URL, true);
    req.timeout = 15000;
    req.onload = function () {
      var len = req.responseText ? req.responseText.length : 0;
      result('XHR', 'XHR: GET ' + req.status + ', ' + len + ' chars, ' + (Date.now() - t0) + ' ms');
    };
    req.onerror = function () {
      result('XHR', 'XHR: error after ' + (Date.now() - t0) + ' ms');
    };
    req.ontimeout = function () {
      result('XHR', 'XHR: timed out');
    };
    req.send();
  } catch (err) {
    result('XHR', 'XHR: threw ' + err);
  }
}

function testWebSocket(index) {
  if (typeof WebSocket === 'undefined') {
    result('WS', 'WS: no WebSocket');
    return;
  }
  var url = WS_URLS[index];
  var host = url.replace('wss://', '').split('/')[0];
  var t0 = Date.now();
  var opened = 0;
  var sent = 0;
  var echoed = 0;
  var rtt = -1;
  var pending = {};
  var done = false;
  var ws;
  var pinger;

  function finish(text) {
    if (done) return;
    done = true;
    clearInterval(pinger);
    result('WS', text);
  }

  function ping() {
    var tag = 'probe-' + sent;
    pending[tag] = Date.now();
    sent++;
    ws.send(tag);
  }

  try {
    ws = new WebSocket(url);
  } catch (err) {
    if (index + 1 < WS_URLS.length) return testWebSocket(index + 1);
    return finish('WS: threw ' + err);
  }
  ws.onopen = function () {
    opened = Date.now();
    ping();
    pinger = setInterval(function () {
      if (Date.now() - opened >= WS_HOLD_MS) {
        ws.close();
        finish('WS: ' + host + ' held ' + Math.round((Date.now() - opened) / 1000) +
               ' s open, ' + echoed + '/' + sent + ' echoed, rtt ' + rtt + ' ms');
      } else {
        ping();
      }
    }, WS_PING_MS);
  };
  ws.onmessage = function (m) {
    var at = pending[m.data];
    if (at === undefined) return; // a greeting, not an echo
    delete pending[m.data];
    echoed++;
    if (echoed === 1) {
      rtt = Date.now() - at;
      // Shown at once; the hold's result follows.
      result('WS', 'WS: ' + host + ' open in ' + (opened - t0) + ' ms, echo ' + rtt +
             ' ms, holding ' + WS_HOLD_MS / 1000 + ' s...');
    }
  };
  ws.onerror = function () {
    if (!opened && index + 1 < WS_URLS.length) {
      done = true;
      clearInterval(pinger);
      return testWebSocket(index + 1);
    }
    finish('WS: ' + host + ' error' + (opened ? ' after ' + Math.round((Date.now() - opened) / 1000) + ' s' : ' before open'));
  };
  ws.onclose = function (ev) {
    finish('WS: ' + host + ' closed ' + (ev && ev.code) + (opened ? ' after ' +
           Math.round((Date.now() - opened) / 1000) + ' s, ' + echoed + '/' + sent + ' echoed' : ' before open'));
  };
}
