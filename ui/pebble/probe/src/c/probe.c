// The probe: measures what an app gets on this watch and shows it, a page at
// a time (Up and Down page, Select runs the AppMessage test again). Every
// result is also logged, so `pebble logs` has them.
//
// 1. Rust: a no_std static library over the app heap (rust/).
// 2. Storage: the largest single persist value, the total that fits, write
//    and read times; every key it wrote is deleted again.
// 3. AppMessage: the inbox and outbox maxima, then round trips of
//    full-size messages through each phone side that says hello.
// 4. The phone script's WebSocket and XHR checks, sent back to show.
// 5. The watch's model, firmware and screen.

#include <pebble.h>
#include "pebble_process_info.h"

#define LOG(fmt, ...) APP_LOG(APP_LOG_LEVEL_INFO, "probe " fmt, ##__VA_ARGS__)

// ---- Rust -------------------------------------------------------------------

typedef struct {
  uint32_t ok, map_len, vec_sum, aligned_ok;
  uint32_t free_before, free_peak, free_after;
  uint32_t used_before, used_peak, used_after;
} RustReport;

extern void probe_rust_run(RustReport *out);

void probe_rust_panicked(void) {
  APP_LOG(APP_LOG_LEVEL_ERROR, "probe rust panicked");
}

// The image header the SDK writes; its sizes are filled in after linking.
extern const PebbleProcessInfo __pbl_app_info;

// ---- Pages ------------------------------------------------------------------

enum { PAGE_DEVICE, PAGE_RUST, PAGE_STORAGE, PAGE_MSG, PAGE_PHONE, PAGE_COUNT };
static const char *const s_titles[PAGE_COUNT] = {
  "Device", "Rust", "Storage", "AppMessage", "Phone script",
};
static char s_pages[PAGE_COUNT][400];
static int s_page;

static Window *s_window;
static TextLayer *s_title_layer;
static TextLayer *s_body_layer;
static char s_title[32];

static void show(void) {
  snprintf(s_title, sizeof(s_title), "%d/%d %s", s_page + 1, PAGE_COUNT, s_titles[s_page]);
  text_layer_set_text(s_title_layer, s_title);
  text_layer_set_text(s_body_layer, s_pages[s_page]);
}

static void refresh(int page) {
  if (s_window && page == s_page) {
    show();
  }
}

// ---- Time -------------------------------------------------------------------

static uint32_t now_ms(void) {
  time_t s;
  uint16_t ms;
  time_ms(&s, &ms);
  return (uint32_t)s * 1000u + ms;
}

// time_ms's seconds and milliseconds can disagree across a second's edge
// (seen in the emulator), so an interval can come out negative: such
// readings are counted and taken as zero.
static uint32_t s_clock_steps;

static uint32_t since(uint32_t t0) {
  int32_t d = (int32_t)(now_ms() - t0);
  if (d < 0) {
    s_clock_steps++;
    return 0;
  }
  return (uint32_t)d;
}

// ---- Device -----------------------------------------------------------------

#if defined(PBL_PLATFORM_EMERY)
#define PLATFORM_NAME "emery"
#elif defined(PBL_PLATFORM_GABBRO)
#define PLATFORM_NAME "gabbro"
#elif defined(PBL_PLATFORM_FLINT)
#define PLATFORM_NAME "flint"
#else
#define PLATFORM_NAME "other"
#endif

static const char *model_name(WatchInfoModel m) {
  switch (m) {
    case WATCH_INFO_MODEL_PEBBLE_ORIGINAL: return "Pebble";
    case WATCH_INFO_MODEL_PEBBLE_STEEL: return "Pebble Steel";
    case WATCH_INFO_MODEL_PEBBLE_TIME: return "Pebble Time";
    case WATCH_INFO_MODEL_PEBBLE_TIME_STEEL: return "Time Steel";
    case WATCH_INFO_MODEL_PEBBLE_TIME_ROUND_14: return "Time Round 14";
    case WATCH_INFO_MODEL_PEBBLE_TIME_ROUND_20: return "Time Round 20";
    case WATCH_INFO_MODEL_PEBBLE_2_HR: return "Pebble 2 HR";
    case WATCH_INFO_MODEL_PEBBLE_2_SE: return "Pebble 2 SE";
    case WATCH_INFO_MODEL_PEBBLE_TIME_2: return "Pebble Time 2";
    case WATCH_INFO_MODEL_COREDEVICES_P2D: return "Core P2D";
    case WATCH_INFO_MODEL_COREDEVICES_PT2: return "Core PT2";
    case WATCH_INFO_MODEL_COREDEVICES_PR2: return "Core PR2";
    default: return "unknown";
  }
}

static void measure_device(GRect bounds) {
  WatchInfoModel model = watch_info_get_model();
  WatchInfoVersion fw = watch_info_get_firmware_version();
  snprintf(s_pages[PAGE_DEVICE], sizeof(s_pages[0]),
           "%s (model %d)\nfirmware %d.%d.%d\nscreen %dx%d\ncolor id %d\nbuilt for %s\napp API %d.%d",
           model_name(model), (int)model, fw.major, fw.minor, fw.patch,
           bounds.size.w, bounds.size.h, (int)watch_info_get_color(),
           PLATFORM_NAME,
           __pbl_app_info.sdk_version.major, __pbl_app_info.sdk_version.minor);
  LOG("device model=%s (%d)", model_name(model), (int)model);
  LOG("device fw=%d.%d.%d screen=%dx%d", fw.major, fw.minor, fw.patch, bounds.size.w,
      bounds.size.h);
}

// ---- Rust -------------------------------------------------------------------

static void measure_rust(void) {
  RustReport r = {0};
  probe_rust_run(&r);
  uint32_t load = __pbl_app_info.load_size | ((uint32_t)__pbl_app_info.load_size_hi << 16);
  uint32_t virt = __pbl_app_info.virtual_size | ((uint32_t)__pbl_app_info.virtual_size_hi << 16);
  snprintf(s_pages[PAGE_RUST], sizeof(s_pages[0]),
           "%s\nmap %lu keys, sum %lu\nheap free %lu\n  peak %lu, after %lu\nheap used %lu\n  peak %lu, after %lu\nimage %lu B\nimage+bss %lu B",
           r.ok ? "rust ok" : "rust FAILED", (unsigned long)r.map_len,
           (unsigned long)r.vec_sum, (unsigned long)r.free_before, (unsigned long)r.free_peak,
           (unsigned long)r.free_after, (unsigned long)r.used_before, (unsigned long)r.used_peak,
           (unsigned long)r.used_after, (unsigned long)load, (unsigned long)virt);
  LOG("rust %s map_len=%lu vec_sum=%lu aligned=%lu", r.ok ? "ok" : "FAILED",
      (unsigned long)r.map_len, (unsigned long)r.vec_sum, (unsigned long)r.aligned_ok);
  LOG("rust heap free before=%lu peak=%lu after=%lu", (unsigned long)r.free_before,
      (unsigned long)r.free_peak, (unsigned long)r.free_after);
  LOG("rust heap used before=%lu peak=%lu after=%lu", (unsigned long)r.used_before,
      (unsigned long)r.used_peak, (unsigned long)r.used_after);
  LOG("image load_size=%lu virtual_size=%lu", (unsigned long)load, (unsigned long)virt);
}

// ---- Storage ----------------------------------------------------------------
//
// A state machine on a timer, a few writes per tick, so the app never holds
// the event loop long.

#define KEY_BASE 0x5000u
#define KEY_SPAN 4096u           // the keys the probe may use, all deleted
#define SINGLE_CAP 65536         // the largest single value tried
#define FILL_CAP_BYTES (1u << 20) // stop filling at 1 MiB and say so
#define PER_TICK 4

typedef enum {
  ST_CLEAN, ST_GROW, ST_SEARCH, ST_FILL, ST_READ, ST_DELETE, ST_DONE,
} StorageStep;

static struct {
  StorageStep step;
  uint32_t key;
  uint8_t *buf;
  int buf_len;
  int ok_size, bad_size;  // the single-value search's bounds
  int heap_limited;
  int fill_size;          // the value size being filled with
  uint32_t fill_keys, fill_bytes;
  // Each phase is timed whole, start to end, timer gaps included, so a
  // step of the clock moves the average by little.
  uint32_t phase_t0;
  uint32_t write_ms, write_count;  // every write the fill tried
  uint32_t read_ms, read_count;
  uint32_t delete_ms, delete_count;
  status_t first_error;
  int capped;
  uint32_t max_size;
} st;

static void storage_tick(void *data);
static void run_appmessage_tests(void);
static bool s_storage_done;

static bool ensure_buf(int n) {
  if (st.buf_len >= n) return true;
  free(st.buf);
  st.buf = malloc(n);
  st.buf_len = st.buf ? n : 0;
  if (st.buf) {
    for (int i = 0; i < n; i++) st.buf[i] = (uint8_t)(i * 7 + 3);
  }
  return st.buf != NULL;
}

// Writes n bytes at key and checks they come back. Leaves the key written.
static bool try_write(uint32_t key, int n, status_t *rc_out) {
  if (!ensure_buf(n)) {
    st.heap_limited = n;
    return false;
  }
  status_t rc = persist_write_data(key, st.buf, n);
  if (rc_out) *rc_out = rc;
  if (rc != n || persist_get_size(key) != n) return false;
  return true;
}

static bool single_value_ok(int n) {
  status_t rc = 0;
  bool ok = try_write(KEY_BASE, n, &rc);
  if (ok) {
    uint8_t *back = malloc(n);
    if (back) {
      ok = persist_read_data(KEY_BASE, back, n) == n && memcmp(back, st.buf, n) == 0;
      free(back);
    }
  }
  persist_delete(KEY_BASE);
  LOG("storage single %d bytes: rc=%ld %s", n, (long)rc, ok ? "ok" : "fail");
  return ok;
}

static void storage_report(void) {
  uint32_t avg_w = st.write_count ? (st.write_ms * 1000u / st.write_count) : 0;
  uint32_t avg_r = st.read_count ? (st.read_ms * 1000u / st.read_count) : 0;
  uint32_t avg_d = st.delete_count ? (st.delete_ms * 1000u / st.delete_count) : 0;
  snprintf(s_pages[PAGE_STORAGE], sizeof(s_pages[0]),
           "max value %d B%s\ntotal %s%lu B\n  in %lu keys\nmax_size() %lu B\nwrite %lu.%03lu ms/op\nread %lu.%03lu ms/op\ndelete %lu.%03lu ms/op\nfile API: none\n(SDK 4.44 headers)",
           st.ok_size, st.heap_limited ? " (heap)" : "", st.capped ? ">=" : "",
           (unsigned long)st.fill_bytes, (unsigned long)st.fill_keys,
           (unsigned long)st.max_size, (unsigned long)(avg_w / 1000), (unsigned long)(avg_w % 1000),
           (unsigned long)(avg_r / 1000), (unsigned long)(avg_r % 1000),
           (unsigned long)(avg_d / 1000), (unsigned long)(avg_d % 1000));
  LOG("storage max_value=%d heap_limited=%d", st.ok_size, st.heap_limited);
  LOG("storage total=%s%lu bytes in keys=%lu", st.capped ? ">=" : "",
      (unsigned long)st.fill_bytes, (unsigned long)st.fill_keys);
  LOG("storage first_error=%ld last_fill_size=%d", (long)st.first_error, st.fill_size);
  LOG("storage persist_get_max_size=%lu", (unsigned long)st.max_size);
  LOG("storage us/op write=%lu read=%lu delete=%lu", (unsigned long)avg_w,
      (unsigned long)avg_r, (unsigned long)avg_d);
  LOG("storage ops write=%lu read=%lu delete=%lu", (unsigned long)st.write_count,
      (unsigned long)st.read_count, (unsigned long)st.delete_count);
  LOG("storage file API in SDK headers: none (persist_* only)");
  LOG("clock steps so far: %lu", (unsigned long)s_clock_steps);
  refresh(PAGE_STORAGE);
}

static void storage_tick(void *data) {
  switch (st.step) {
    case ST_CLEAN: {
      // Leftovers of a run that did not finish, 256 keys a tick.
      if (st.key < KEY_BASE) st.key = KEY_BASE;
      for (int i = 0; i < 256 && st.key < KEY_BASE + KEY_SPAN; i++, st.key++) {
        if (persist_exists(st.key)) persist_delete(st.key);
      }
      if (st.key < KEY_BASE + KEY_SPAN) break;
      st.max_size = persist_get_max_size();
      st.ok_size = 0;
      st.bad_size = 0;
      st.step = single_value_ok(1) ? ST_GROW : ST_DONE;
      if (st.step == ST_GROW) st.ok_size = 1;
      break;
    }
    case ST_GROW: {
      int n = st.ok_size < 64 ? 64 : st.ok_size * 2;
      if (n > SINGLE_CAP) {
        st.step = ST_FILL;
      } else if (single_value_ok(n)) {
        st.ok_size = n;
      } else {
        st.bad_size = n;
        st.step = ST_SEARCH;
      }
      break;
    }
    case ST_SEARCH: {
      if (st.bad_size - st.ok_size <= 1) {
        st.step = ST_FILL;
        break;
      }
      int mid = st.ok_size + (st.bad_size - st.ok_size) / 2;
      if (single_value_ok(mid)) st.ok_size = mid;
      else st.bad_size = mid;
      break;
    }
    case ST_FILL: {
      if (st.fill_size == 0) {
        st.fill_size = st.ok_size;
        st.key = KEY_BASE;
        st.phase_t0 = now_ms();
      }
      for (int i = 0; i < PER_TICK; i++) {
        if (st.key >= KEY_BASE + KEY_SPAN || st.fill_bytes >= FILL_CAP_BYTES) {
          st.capped = 1;
          st.step = ST_READ;
          break;
        }
        status_t rc = 0;
        bool ok = try_write(st.key, st.fill_size, &rc);
        st.write_count++;
        if (ok) {
          st.fill_bytes += st.fill_size;
          st.fill_keys++;
          st.key++;
          if (st.fill_keys % 64 == 0) {
            snprintf(s_pages[PAGE_STORAGE], sizeof(s_pages[0]),
                     "max value %d B\nfilling...\n%lu keys\n%lu B", st.ok_size,
                     (unsigned long)st.fill_keys, (unsigned long)st.fill_bytes);
            refresh(PAGE_STORAGE);
          }
        } else {
          if (!st.first_error) st.first_error = rc;
          persist_delete(st.key);
          // Squeeze what is left with smaller values.
          st.fill_size /= 2;
          if (st.fill_size < 8) {
            st.step = ST_READ;
            break;
          }
        }
      }
      break;
    }
    case ST_READ: {
      st.write_ms = since(st.phase_t0);
      int n = st.ok_size;
      uint8_t *back = malloc(n);
      uint32_t reads = st.fill_keys < 256 ? st.fill_keys : 256;
      if (back) {
        uint32_t t0 = now_ms();
        for (uint32_t k = 0; k < reads; k++) persist_read_data(KEY_BASE + k, back, n);
        st.read_ms += since(t0);
        st.read_count += reads;
        free(back);
      }
      st.key = KEY_BASE;
      st.step = ST_DELETE;
      st.phase_t0 = now_ms();
      break;
    }
    case ST_DELETE: {
      uint32_t end = KEY_BASE + st.fill_keys;
      for (int i = 0; i < PER_TICK * 4 && st.key < end; i++, st.key++) {
        persist_delete(st.key);
        st.delete_count++;
      }
      if (st.key >= end) {
        st.delete_ms = since(st.phase_t0);
        int left = 0;
        for (uint32_t k = KEY_BASE; k < KEY_BASE + st.fill_keys + 1; k++) left += persist_exists(k);
        LOG("storage cleanup: %d keys left", left);
        st.step = ST_DONE;
      }
      break;
    }
    case ST_DONE:
      break;
  }
  if (st.step == ST_DONE) {
    free(st.buf);
    st.buf = NULL;
    st.buf_len = 0;
    storage_report();
    s_storage_done = true;
    run_appmessage_tests();
    return;
  }
  app_timer_register(1, storage_tick, NULL);
}

// ---- AppMessage -------------------------------------------------------------
//
// Commands, in CMD, both ways:
//   1 HELLO   phone -> watch  SRC: 1 the JS script, 2 a native companion
//   2 ECHO    watch -> phone  SEQ, TARGET, ECHO_LEN, DATA (outbox-sized)
//             phone -> watch  SEQ, SRC, DATA (ECHO_LEN bytes)
//   3 TESTS   watch -> phone  run the WebSocket and XHR checks
//   4 RESULT  phone -> watch  XHR and/or WS: lines to show

enum { CMD_HELLO = 1, CMD_ECHO = 2, CMD_TESTS = 3, CMD_RESULT = 4 };
enum { SRC_JS = 1, SRC_NATIVE = 2, SRC_COUNT = 3 };
#define ROUNDS 20
#define ECHO_TIMEOUT_MS 10000

static uint32_t s_inbox_max, s_outbox_max;
static bool s_hello[SRC_COUNT];
static bool s_tested[SRC_COUNT];
static struct {
  int target;  // 0: idle
  int seq;
  uint32_t sent_at, started_at;
  uint32_t rtt_sum, rtt_min, rtt_max;
  int done, failed, bad_clock;
  uint32_t up, down;
  int out_len, back_len;
  AppTimer *timeout;
} am;
static char s_lines[SRC_COUNT][120];
static char s_xhr[120] = "XHR: waiting";
static char s_ws[160] = "WS: waiting";
static bool s_tests_sent;

// The payload sizes that fill a dictionary of four tuples (three of
// four-byte integers and one byte array) to the buffer's maximum:
// 1 count byte, then 7 header bytes and the value per tuple.
static int fill_len(uint32_t buffer) {
  return (int)buffer - 1 - 4 * 7 - 3 * 4;
}

static void msg_page(void) {
  snprintf(s_pages[PAGE_MSG], sizeof(s_pages[0]),
           "inbox max %lu B\noutbox max %lu B\n%s\n%s",
           (unsigned long)s_inbox_max, (unsigned long)s_outbox_max,
           s_lines[SRC_JS][0] ? s_lines[SRC_JS] : "js: no hello yet",
           s_lines[SRC_NATIVE][0] ? s_lines[SRC_NATIVE] : "native: no hello");
  refresh(PAGE_MSG);
}

static void phone_page(void) {
  snprintf(s_pages[PAGE_PHONE], sizeof(s_pages[0]), "%s\n%s", s_xhr, s_ws);
  refresh(PAGE_PHONE);
}

static void send_echo(void *data);

static void echo_finish(void) {
  int src = am.target;
  uint32_t total = since(am.started_at);
  // The average is the run's wall time over its rounds; min and max are
  // single rounds, which a step of the clock can move by a second.
  int timed = am.done - am.bad_clock;
  uint32_t avg = am.done ? total / ROUNDS : 0;
  if (timed <= 0) am.rtt_min = 0;
  uint32_t bps = total ? (uint64_t)(am.up + am.down) * 1000u / total : 0;
  snprintf(s_lines[src], sizeof(s_lines[0]),
           "%s: %d/%d ok\n %dB up %dB back\n rtt avg %lu ms\n  min %lu max %lu\n %lu B/s both ways",
           src == SRC_JS ? "js" : "native", am.done, ROUNDS, am.out_len, am.back_len,
           (unsigned long)avg, (unsigned long)am.rtt_min, (unsigned long)am.rtt_max,
           (unsigned long)bps);
  const char *who = src == SRC_JS ? "js" : "native";
  LOG("appmessage %s rounds=%d ok=%d failed=%d", who, ROUNDS, am.done, am.failed);
  LOG("appmessage %s bytes up=%d back=%d per round", who, am.out_len, am.back_len);
  LOG("appmessage %s rtt ms avg=%lu min=%lu max=%lu", who, (unsigned long)avg,
      (unsigned long)am.rtt_min, (unsigned long)am.rtt_max);
  LOG("appmessage %s rounds with a clock step: %d", who, am.bad_clock);
  LOG("appmessage %s total_ms=%lu bytes_per_s=%lu", who, (unsigned long)total,
      (unsigned long)bps);
  s_tested[src] = true;
  am.target = 0;
  msg_page();
  run_appmessage_tests();
}

static void echo_timeout(void *data) {
  am.timeout = NULL;
  LOG("appmessage echo %d timed out", am.seq);
  am.failed++;
  am.seq++;
  if (am.seq >= ROUNDS) echo_finish();
  else send_echo(NULL);
}

static void send_echo(void *data) {
  DictionaryIterator *it;
  AppMessageResult r = app_message_outbox_begin(&it);
  if (r != APP_MSG_OK) {
    app_timer_register(20, send_echo, NULL);  // the outbox is still busy
    return;
  }
  dict_write_int32(it, MESSAGE_KEY_CMD, CMD_ECHO);
  dict_write_int32(it, MESSAGE_KEY_SEQ, am.seq);
  dict_write_int32(it, MESSAGE_KEY_TARGET, am.target);
  dict_write_int32(it, MESSAGE_KEY_ECHO_LEN, am.back_len);
  // The outbox keeps some of its buffer for itself: fill what is left.
  int room = (int)((uint8_t *)it->end - (uint8_t *)it->cursor) - 7;
  if (am.out_len > room) {
    LOG("appmessage outbox room for data: %d", room);
    am.out_len = room;
  }
  uint8_t *payload = malloc(am.out_len);
  if (!payload) {
    LOG("appmessage no heap for %d bytes", am.out_len);
    return;
  }
  for (int i = 0; i < am.out_len; i++) payload[i] = (uint8_t)(am.seq + i);
  if (dict_write_data(it, MESSAGE_KEY_DATA, payload, am.out_len) != DICT_OK) {
    LOG("appmessage dict_write_data %d failed", am.out_len);
  }
  free(payload);
  am.sent_at = now_ms();
  r = app_message_outbox_send();
  if (r != APP_MSG_OK) LOG("appmessage send %d: %d", am.seq, (int)r);
  am.timeout = app_timer_register(ECHO_TIMEOUT_MS, echo_timeout, NULL);
}

static void start_echo(int src) {
  memset(&am, 0, sizeof(am));
  am.target = src;
  am.rtt_min = UINT32_MAX;
  am.out_len = fill_len(s_outbox_max);
  // What comes back is one tuple fewer than what goes out (no ECHO_LEN);
  // keep it to the same four-tuple bound.
  am.back_len = fill_len(s_inbox_max);
  am.started_at = now_ms();
  snprintf(s_lines[src], sizeof(s_lines[0]), "%s: testing...", src == SRC_JS ? "js" : "native");
  msg_page();
  send_echo(NULL);
}

static void send_tests_request(void *data) {
  DictionaryIterator *it;
  if (app_message_outbox_begin(&it) != APP_MSG_OK) {
    app_timer_register(50, send_tests_request, NULL);
    return;
  }
  dict_write_int32(it, MESSAGE_KEY_CMD, CMD_TESTS);
  app_message_outbox_send();
  s_tests_sent = true;
  LOG("phone tests requested");
}

// Runs whatever is due: an echo test per source that said hello, then the
// phone script's checks.
static void run_appmessage_tests(void) {
  if (!s_storage_done || am.target) return;
  for (int src = SRC_JS; src < SRC_COUNT; src++) {
    if (s_hello[src] && !s_tested[src]) {
      start_echo(src);
      return;
    }
  }
  if (s_hello[SRC_JS] && !s_tests_sent) {
    app_timer_register(10, send_tests_request, NULL);
  }
}

static void inbox_received(DictionaryIterator *it, void *ctx) {
  Tuple *cmd = dict_find(it, MESSAGE_KEY_CMD);
  if (!cmd) return;
  Tuple *src_t = dict_find(it, MESSAGE_KEY_SRC);
  int src = src_t ? (int)src_t->value->int32 : 0;
  switch ((int)cmd->value->int32) {
    case CMD_HELLO:
      if (src > 0 && src < SRC_COUNT) {
        LOG("hello from %s", src == SRC_JS ? "js" : "native");
        s_hello[src] = true;
        run_appmessage_tests();
      }
      break;
    case CMD_ECHO: {
      Tuple *seq = dict_find(it, MESSAGE_KEY_SEQ);
      Tuple *data = dict_find(it, MESSAGE_KEY_DATA);
      if (!seq || src != am.target || (int)seq->value->int32 != am.seq) break;
      int32_t rtt = (int32_t)(now_ms() - am.sent_at);
      if (am.timeout) {
        app_timer_cancel(am.timeout);
        am.timeout = NULL;
      }
      am.done++;
      if (rtt < 0) {
        am.bad_clock++;
        s_clock_steps++;
      } else {
        am.rtt_sum += rtt;
        if ((uint32_t)rtt < am.rtt_min) am.rtt_min = rtt;
        if ((uint32_t)rtt > am.rtt_max) am.rtt_max = rtt;
      }
      am.up += am.out_len;
      am.down += data ? data->length : 0;
      if (!data || data->length != am.back_len) {
        LOG("appmessage echo %d came back with %d bytes", am.seq, data ? data->length : -1);
      }
      am.seq++;
      if (am.seq >= ROUNDS) echo_finish();
      else app_timer_register(1, send_echo, NULL);
      break;
    }
    case CMD_RESULT: {
      Tuple *xhr = dict_find(it, MESSAGE_KEY_XHR);
      Tuple *ws = dict_find(it, MESSAGE_KEY_WS);
      if (xhr && xhr->type == TUPLE_CSTRING) {
        snprintf(s_xhr, sizeof(s_xhr), "%s", xhr->value->cstring);
        LOG("phone %s", s_xhr);
      }
      if (ws && ws->type == TUPLE_CSTRING) {
        snprintf(s_ws, sizeof(s_ws), "%s", ws->value->cstring);
        LOG("phone %s", s_ws);
      }
      phone_page();
      break;
    }
  }
}

static void inbox_dropped(AppMessageResult reason, void *ctx) {
  LOG("appmessage inbox dropped: %d", (int)reason);
}

static void outbox_failed(DictionaryIterator *it, AppMessageResult reason, void *ctx) {
  LOG("appmessage outbox failed: %d", (int)reason);
}

static void open_appmessage(void) {
  s_inbox_max = app_message_inbox_size_maximum();
  s_outbox_max = app_message_outbox_size_maximum();
  LOG("appmessage inbox_max=%lu outbox_max=%lu", (unsigned long)s_inbox_max,
      (unsigned long)s_outbox_max);
  LOG("appmessage heap_free_before_open=%lu", (unsigned long)heap_bytes_free());
  app_message_register_inbox_received(inbox_received);
  app_message_register_inbox_dropped(inbox_dropped);
  app_message_register_outbox_failed(outbox_failed);
  AppMessageResult r = app_message_open(s_inbox_max, s_outbox_max);
  LOG("appmessage open: %d, heap_free_after_open=%lu", (int)r,
      (unsigned long)heap_bytes_free());
  msg_page();
  phone_page();
}

// ---- Window and buttons -----------------------------------------------------

static void up_click(ClickRecognizerRef r, void *ctx) {
  s_page = (s_page + PAGE_COUNT - 1) % PAGE_COUNT;
  show();
}

static void down_click(ClickRecognizerRef r, void *ctx) {
  s_page = (s_page + 1) % PAGE_COUNT;
  show();
}

static void select_click(ClickRecognizerRef r, void *ctx) {
  // Runs the AppMessage tests again, and the phone's checks.
  if (am.target) return;
  for (int src = 0; src < SRC_COUNT; src++) s_tested[src] = false;
  s_tests_sent = false;
  run_appmessage_tests();
}

static void click_config(void *ctx) {
  window_single_click_subscribe(BUTTON_ID_UP, up_click);
  window_single_click_subscribe(BUTTON_ID_DOWN, down_click);
  window_single_click_subscribe(BUTTON_ID_SELECT, select_click);
}

static void window_load(Window *window) {
  Layer *root = window_get_root_layer(window);
  GRect b = layer_get_bounds(root);
  s_title_layer = text_layer_create(GRect(4, 0, b.size.w - 8, 26));
  text_layer_set_font(s_title_layer, fonts_get_system_font(FONT_KEY_GOTHIC_18_BOLD));
  s_body_layer = text_layer_create(GRect(4, 24, b.size.w - 8, b.size.h - 24));
  text_layer_set_font(s_body_layer, fonts_get_system_font(FONT_KEY_GOTHIC_18));
  text_layer_set_overflow_mode(s_body_layer, GTextOverflowModeWordWrap);
  layer_add_child(root, text_layer_get_layer(s_title_layer));
  layer_add_child(root, text_layer_get_layer(s_body_layer));
  measure_device(b);
  show();
}

static void window_unload(Window *window) {
  text_layer_destroy(s_title_layer);
  text_layer_destroy(s_body_layer);
}

int main(void) {
  for (int i = 0; i < PAGE_COUNT; i++) snprintf(s_pages[i], sizeof(s_pages[0]), "measuring...");
  LOG("start, heap free %lu used %lu", (unsigned long)heap_bytes_free(),
      (unsigned long)heap_bytes_used());
  // Rust first, so its heap numbers are not moved by AppMessage's buffers.
  measure_rust();
  s_window = window_create();
  window_set_click_config_provider(s_window, click_config);
  window_set_window_handlers(s_window, (WindowHandlers){
    .load = window_load,
    .unload = window_unload,
  });
  window_stack_push(s_window, true);
  open_appmessage();
  app_timer_register(50, storage_tick, NULL);
  app_event_loop();
  // Left mid-run: delete what this run wrote (the next run also sweeps).
  if (!s_storage_done) {
    for (uint32_t k = KEY_BASE; k <= st.key && k < KEY_BASE + KEY_SPAN; k++) persist_delete(k);
  }
  window_destroy(s_window);
}
