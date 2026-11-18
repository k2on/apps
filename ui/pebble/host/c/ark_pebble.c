// The Pebble host's C side: a thin shim between the SDK and the Rust host
// (ark-pebble). It runs the SDK's main loop and owns what the SDK hands
// out (the window, its layers, AppMessage, timers, the connection
// service); every decision is Rust's. It calls in through `ark_on_*` and
// is called through the functions below, which Rust declares in sys.rs.
//
// The screen is the renderer's first cut: five text slots, a progress
// bar, a list (a MenuLayer of drawn rows) and an action bar whose glyphs
// are painted into bitmaps here, so the app needs no resources.
//
// Messages, both ways (the keys are package.json's messageKeys):
//   watch -> phone  READY, INBOX, ORIGIN   what the watch is, at its start
//                   EPOCH, DATA            a chunk of the frames going up
//   phone -> watch  HELLO                  say READY again
//                   LINK                   the phone's connection upstream
//                                          opened (its epoch) or closed (0)
//                   EPOCH, DATA            a chunk of the frames coming down

#include <pebble.h>
#include "pebble_process_info.h"

// AppMessage's buffers come from the app heap: 8200 bytes each at most.
// Smaller ones leave the heap to the node; a frame longer than a message
// is cut into several either way.
#ifndef ARK_INBOX
#define ARK_INBOX 2048
#endif
#ifndef ARK_OUTBOX
#define ARK_OUTBOX 1024
#endif

#define LOG(fmt, ...) APP_LOG(APP_LOG_LEVEL_INFO, fmt, ##__VA_ARGS__)

// ---- Rust -------------------------------------------------------------------

extern void ark_start(void);
extern void ark_on_button(uint8_t id);
extern void ark_on_hello(void);
extern void ark_on_link(uint32_t epoch);
extern void ark_on_chunk(uint32_t epoch, const uint8_t *data, size_t len);
extern void ark_on_sent(void);
extern void ark_on_failed(void);
extern void ark_on_timer(uint8_t kind);
extern void ark_on_phone(bool connected);
extern void ark_on_stop(void);

extern const PebbleProcessInfo __pbl_app_info;

void ark_rust_panicked(void) {
  APP_LOG(APP_LOG_LEVEL_ERROR, "ark rust panicked");
}

void ark_log(const uint8_t *msg, size_t len) {
  static char line[168];
  if (len >= sizeof(line)) len = sizeof(line) - 1;
  memcpy(line, msg, len);
  line[len] = 0;
  LOG("ark %s", line);
}

// The watch's time in milliseconds. time_ms's seconds and milliseconds can
// disagree across a second's edge (seen in the emulator), so it is held
// from going back.
uint64_t ark_now_ms(void) {
  static uint64_t last;
  time_t s;
  uint16_t ms;
  time_ms(&s, &ms);
  uint64_t now = (uint64_t)s * 1000u + ms;
  if (now < last) now = last;
  last = now;
  return now;
}

uint32_t ark_random(void) {
  return ((uint32_t)rand() << 16) ^ (uint32_t)rand();
}

// ---- Screen -----------------------------------------------------------------

enum { SLOT_STATUS, SLOT_TITLE, SLOT_SUBTITLE, SLOT_TIME, SLOT_HEADER, SLOT_COUNT };
enum { ROW_MAX = 4, TEXT_MAX = 48, ROW_HEIGHT = 34 };
enum { GLYPH_NONE, GLYPH_PLAY, GLYPH_PAUSE, GLYPH_SKIP, GLYPH_COUNT };

static Window *s_window;
static TextLayer *s_text[SLOT_COUNT];
static char s_buf[SLOT_COUNT][TEXT_MAX];
static Layer *s_progress;
static uint16_t s_permille;
static MenuLayer *s_menu;
static char s_rows[ROW_MAX][2][TEXT_MAX];
static uint16_t s_row_count;
static ActionBarLayer *s_bar;
static GBitmap *s_glyphs[GLYPH_COUNT];
static GColor s_palette[2];

static void copy_text(char *dst, const uint8_t *src, size_t len) {
  if (len >= TEXT_MAX) len = TEXT_MAX - 1;
  memcpy(dst, src, len);
  dst[len] = 0;
}

void ark_ui_text(uint8_t slot, const uint8_t *s, size_t len) {
  if (slot >= SLOT_COUNT) return;
  copy_text(s_buf[slot], s, len);
  text_layer_set_text(s_text[slot], s_buf[slot]);
}

void ark_ui_row(uint8_t i, const uint8_t *t, size_t tl, const uint8_t *s, size_t sl) {
  if (i >= ROW_MAX) return;
  copy_text(s_rows[i][0], t, tl);
  copy_text(s_rows[i][1], s, sl);
}

void ark_ui_rows(uint8_t n) {
  s_row_count = n < ROW_MAX ? n : ROW_MAX;
  menu_layer_reload_data(s_menu);
}

void ark_ui_progress(uint16_t permille) {
  s_permille = permille > 1000 ? 1000 : permille;
  layer_mark_dirty(s_progress);
}

void ark_ui_action(uint8_t button, uint8_t glyph) {
  if (button < BUTTON_ID_UP || button > BUTTON_ID_DOWN) return;
  if (glyph > GLYPH_NONE && glyph < GLYPH_COUNT) {
    action_bar_layer_set_icon(s_bar, (ButtonId)button, s_glyphs[glyph]);
  } else {
    action_bar_layer_clear_icon(s_bar, (ButtonId)button);
  }
}

// A glyph, 18 by 18 pixels: whether the pixel at (x, y) is inked. Shapes
// span rows 2 to 15; dy is the distance from their middle, in half pixels.
static bool inked(int glyph, int x, int y) {
  int dy = 2 * y - 17;
  if (dy < 0) dy = -dy;
  if (y < 2 || y > 15) return false;
  switch (glyph) {
    case GLYPH_PLAY: return x >= 4 && x <= 15 && dy * 11 <= (15 - x) * 13;
    case GLYPH_PAUSE: return (x >= 4 && x <= 7) || (x >= 10 && x <= 13);
    case GLYPH_SKIP: return (x >= 2 && x <= 11 && dy * 9 <= (11 - x) * 13) || (x >= 13 && x <= 15);
    default: return false;
  }
}

static GBitmap *paint(int glyph) {
  GBitmap *b = gbitmap_create_blank_with_palette(GSize(18, 18), GBitmapFormat1BitPalette, s_palette, false);
  if (!b) return NULL;
  uint8_t *data = gbitmap_get_data(b);
  int stride = gbitmap_get_bytes_per_row(b);
  memset(data, 0, stride * 18);
  for (int y = 0; y < 18; y++) {
    for (int x = 0; x < 18; x++) {
      if (inked(glyph, x, y)) data[y * stride + x / 8] |= 0x80 >> (x % 8);
    }
  }
  return b;
}

static void progress_update(Layer *layer, GContext *ctx) {
  GRect b = layer_get_bounds(layer);
  graphics_context_set_fill_color(ctx, PBL_IF_COLOR_ELSE(GColorDarkGray, GColorWhite));
  graphics_fill_rect(ctx, GRect(0, b.size.h / 2 - 1, b.size.w, 2), 0, GCornerNone);
  int w = b.size.w * s_permille / 1000;
  graphics_context_set_fill_color(ctx, PBL_IF_COLOR_ELSE(GColorVividCerulean, GColorWhite));
  graphics_fill_rect(ctx, GRect(0, 0, w, b.size.h), 2, GCornersAll);
}

static uint16_t menu_rows(MenuLayer *menu, uint16_t section, void *ctx) {
  return s_row_count;
}

static int16_t menu_row_height(MenuLayer *menu, MenuIndex *index, void *ctx) {
  return ROW_HEIGHT;
}

static void menu_draw_row(GContext *ctx, const Layer *cell, MenuIndex *index, void *data) {
  if (index->row >= s_row_count) return;
  GRect b = layer_get_bounds(cell);
  graphics_context_set_text_color(ctx, GColorWhite);
  graphics_draw_text(ctx, s_rows[index->row][0], fonts_get_system_font(FONT_KEY_GOTHIC_18_BOLD),
                     GRect(4, -4, b.size.w - 8, 22), GTextOverflowModeTrailingEllipsis,
                     GTextAlignmentLeft, NULL);
  graphics_context_set_text_color(ctx, PBL_IF_COLOR_ELSE(GColorLightGray, GColorWhite));
  graphics_draw_text(ctx, s_rows[index->row][1], fonts_get_system_font(FONT_KEY_GOTHIC_14),
                     GRect(4, 15, b.size.w - 8, 18), GTextOverflowModeTrailingEllipsis,
                     GTextAlignmentLeft, NULL);
}

static void click(ClickRecognizerRef r, void *ctx) {
  ark_on_button((uint8_t)click_recognizer_get_button_id(r));
}

static void click_config(void *ctx) {
  window_single_click_subscribe(BUTTON_ID_UP, click);
  window_single_click_subscribe(BUTTON_ID_SELECT, click);
  window_single_click_subscribe(BUTTON_ID_DOWN, click);
}

static TextLayer *text(Layer *root, GRect frame, const char *font, GColor color) {
  TextLayer *t = text_layer_create(frame);
  text_layer_set_background_color(t, GColorClear);
  text_layer_set_text_color(t, color);
  text_layer_set_font(t, fonts_get_system_font(font));
  text_layer_set_overflow_mode(t, GTextOverflowModeTrailingEllipsis);
  layer_add_child(root, text_layer_get_layer(t));
  return t;
}

static void window_load(Window *window) {
  Layer *root = window_get_root_layer(window);
  GRect b = layer_get_bounds(root);
  int w = b.size.w - ACTION_BAR_WIDTH;
  GColor dim = PBL_IF_COLOR_ELSE(GColorLightGray, GColorWhite);
  s_text[SLOT_STATUS] = text(root, GRect(6, 0, w - 8, 18), FONT_KEY_GOTHIC_14, dim);
  s_text[SLOT_TITLE] = text(root, GRect(6, 16, w - 8, 30), FONT_KEY_GOTHIC_24_BOLD, GColorWhite);
  s_text[SLOT_SUBTITLE] = text(root, GRect(6, 44, w - 8, 24), FONT_KEY_GOTHIC_18, dim);
  s_progress = layer_create(GRect(6, 74, w - 12, 6));
  layer_set_update_proc(s_progress, progress_update);
  layer_add_child(root, s_progress);
  s_text[SLOT_TIME] = text(root, GRect(6, 80, w - 8, 22), FONT_KEY_GOTHIC_18, GColorWhite);
  s_text[SLOT_HEADER] = text(root, GRect(6, 106, w - 8, 18), FONT_KEY_GOTHIC_14_BOLD, dim);
  s_menu = menu_layer_create(GRect(2, 124, w - 2, b.size.h - 124));
  menu_layer_set_callbacks(s_menu, NULL, (MenuLayerCallbacks){
    .get_num_rows = menu_rows,
    .get_cell_height = menu_row_height,
    .draw_row = menu_draw_row,
  });
  menu_layer_set_normal_colors(s_menu, GColorBlack, GColorWhite);
  menu_layer_set_highlight_colors(s_menu, GColorBlack, GColorWhite);
  layer_add_child(root, menu_layer_get_layer(s_menu));

  s_palette[0] = GColorClear;
  s_palette[1] = GColorWhite;
  for (int g = GLYPH_PLAY; g < GLYPH_COUNT; g++) s_glyphs[g] = paint(g);
  s_bar = action_bar_layer_create();
  action_bar_layer_set_background_color(s_bar, PBL_IF_COLOR_ELSE(GColorDarkGray, GColorWhite));
  action_bar_layer_add_to_window(s_bar, window);
  action_bar_layer_set_click_config_provider(s_bar, click_config);
}

static void window_unload(Window *window) {
  action_bar_layer_destroy(s_bar);
  for (int g = 0; g < GLYPH_COUNT; g++) {
    if (s_glyphs[g]) gbitmap_destroy(s_glyphs[g]);
  }
  menu_layer_destroy(s_menu);
  layer_destroy(s_progress);
  for (int i = 0; i < SLOT_COUNT; i++) text_layer_destroy(s_text[i]);
}

// ---- Timers -----------------------------------------------------------------

enum { TIMER_COUNT = 3 };
static AppTimer *s_timers[TIMER_COUNT];

static void timer_fired(void *data) {
  uint8_t kind = (uint8_t)(uintptr_t)data;
  s_timers[kind] = NULL;
  ark_on_timer(kind);
}

// Sets timer `kind` to fire in `ms`, or cancels it (UINT32_MAX).
void ark_timer(uint8_t kind, uint32_t ms) {
  if (kind >= TIMER_COUNT) return;
  if (s_timers[kind]) {
    app_timer_cancel(s_timers[kind]);
    s_timers[kind] = NULL;
  }
  if (ms != UINT32_MAX) s_timers[kind] = app_timer_register(ms, timer_fired, (void *)(uintptr_t)kind);
}

// ---- AppMessage and the phone -----------------------------------------------

uint32_t ark_inbox_size(void) { return ARK_INBOX; }
uint32_t ark_outbox_size(void) { return ARK_OUTBOX; }

bool ark_phone_connected(void) {
  return connection_service_peek_pebble_app_connection();
}

bool ark_send_ready(uint32_t inbox, const uint8_t *origin, size_t len) {
  DictionaryIterator *it;
  if (app_message_outbox_begin(&it) != APP_MSG_OK) return false;
  char hex[24];
  copy_text(hex, origin, len < sizeof(hex) ? len : sizeof(hex) - 1);
  dict_write_int32(it, MESSAGE_KEY_READY, 1);
  dict_write_int32(it, MESSAGE_KEY_INBOX, (int32_t)inbox);
  dict_write_cstring(it, MESSAGE_KEY_ORIGIN, hex);
  return app_message_outbox_send() == APP_MSG_OK;
}

bool ark_send_chunk(uint32_t epoch, const uint8_t *data, size_t len) {
  DictionaryIterator *it;
  if (app_message_outbox_begin(&it) != APP_MSG_OK) return false;
  dict_write_int32(it, MESSAGE_KEY_EPOCH, (int32_t)epoch);
  if (dict_write_data(it, MESSAGE_KEY_DATA, data, len) != DICT_OK) return false;
  return app_message_outbox_send() == APP_MSG_OK;
}

static void inbox_received(DictionaryIterator *it, void *ctx) {
  if (dict_find(it, MESSAGE_KEY_HELLO)) ark_on_hello();
  Tuple *link = dict_find(it, MESSAGE_KEY_LINK);
  if (link) ark_on_link((uint32_t)link->value->int32);
  Tuple *data = dict_find(it, MESSAGE_KEY_DATA);
  if (data) {
    Tuple *epoch = dict_find(it, MESSAGE_KEY_EPOCH);
    ark_on_chunk(epoch ? (uint32_t)epoch->value->int32 : 0, data->value->data, data->length);
  }
}

static void inbox_dropped(AppMessageResult reason, void *ctx) {
  LOG("ark inbox dropped: %d", (int)reason);
}

static void outbox_sent(DictionaryIterator *it, void *ctx) {
  ark_on_sent();
}

static void outbox_failed(DictionaryIterator *it, AppMessageResult reason, void *ctx) {
  LOG("ark outbox failed: %d", (int)reason);
  ark_on_failed();
}

static void phone_changed(bool connected) {
  ark_on_phone(connected);
}

// ---- Main -------------------------------------------------------------------

int main(void) {
  srand((unsigned)time(NULL) ^ (unsigned)ark_now_ms());
  uint32_t load = __pbl_app_info.load_size | ((uint32_t)__pbl_app_info.load_size_hi << 16);
  uint32_t virt = __pbl_app_info.virtual_size | ((uint32_t)__pbl_app_info.virtual_size_hi << 16);
  LOG("ark image %lu bytes, with bss %lu; heap free %lu used %lu", (unsigned long)load,
      (unsigned long)virt, (unsigned long)heap_bytes_free(), (unsigned long)heap_bytes_used());

  s_window = window_create();
  window_set_background_color(s_window, GColorBlack);
  window_set_window_handlers(s_window, (WindowHandlers){
    .load = window_load,
    .unload = window_unload,
  });
  window_stack_push(s_window, true);

  app_message_register_inbox_received(inbox_received);
  app_message_register_inbox_dropped(inbox_dropped);
  app_message_register_outbox_sent(outbox_sent);
  app_message_register_outbox_failed(outbox_failed);
  AppMessageResult r = app_message_open(ARK_INBOX, ARK_OUTBOX);
  LOG("ark appmessage %d in, %d out: %d; heap free %lu", ARK_INBOX, ARK_OUTBOX, (int)r,
      (unsigned long)heap_bytes_free());
  connection_service_subscribe((ConnectionHandlers){
    .pebble_app_connection_handler = phone_changed,
  });

  ark_start();
  LOG("ark started; heap free %lu used %lu", (unsigned long)heap_bytes_free(),
      (unsigned long)heap_bytes_used());
  app_event_loop();
  ark_on_stop();
  connection_service_unsubscribe();
  window_destroy(s_window);
}
