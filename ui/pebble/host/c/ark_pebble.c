// The Pebble host's C side: a thin shim between the SDK and the Rust host
// (ark-pebble). It runs the SDK's main loop and owns what the SDK hands
// out (the window, its layers, AppMessage, timers, the connection
// service); every decision is Rust's. It calls in through `ark_on_*` and
// is called through the functions below, which Rust declares in sys.rs.
//
// The screen is arkui's Pebble toolkit: the native layers (text, a meter,
// a glyph's bitmap, a list, the action bar) a screen generated from a
// `#[view]` is made of, made and set by id from Rust (ui/docs/pebble.md).
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
//
// arkui's Pebble toolkit (ui/docs/pebble.md): the layers a screen generated
// from a `#[view]` is made of, each made by id at the frame Rust worked out
// and given its value by id after. Rust (ark-pebble's `ui`) keeps what each
// last showed and calls in only for what changed. Glyphs come painted, as
// bits, and are made 1-bit bitmaps here in the colour asked.

enum { ARK_LAYERS = 16, TEXT_MAX = 48, ROW_MAX = 6, BITMAP_MAX = 12 };
enum { KIND_NONE, KIND_TEXT, KIND_METER, KIND_RECT, KIND_IMAGE, KIND_LIST };

// A frame and a list's look as Rust hands them (sys.rs's `Frame`, `CList`).
typedef struct { int16_t x, y, w, h; } ArkFrame;
typedef struct {
  uint8_t title_size; bool title_bold;
  uint8_t sub_size; bool sub_bold;
  uint8_t header_size; bool header_bold;
  uint8_t ink, quiet, ground;
  int16_t inset, title_line, row_height, header_height, lift;
} ArkListStyle;

typedef struct {
  uint8_t fill, track;
  int16_t track_h, corner;
  uint16_t permille;
} Meter;

typedef struct {
  uint8_t color;
  int16_t corner;
} Rect;

typedef struct {
  ArkListStyle style;
  char header[TEXT_MAX];
  char rows[ROW_MAX][2][TEXT_MAX];
  uint16_t count;
} List;

static Window *s_window;
static uint8_t s_kind[ARK_LAYERS];
static Layer *s_layer[ARK_LAYERS];
static TextLayer *s_text[ARK_LAYERS];
static BitmapLayer *s_image[ARK_LAYERS];
static MenuLayer *s_menu[ARK_LAYERS];
static List *s_list[ARK_LAYERS];
static char s_words[ARK_LAYERS][TEXT_MAX];
static ActionBarLayer *s_bar;
static GBitmap *s_bitmaps[BITMAP_MAX];
static GColor s_palettes[BITMAP_MAX][2];
static uint8_t s_bitmap_count;

static GColor color(uint8_t argb) {
  return (GColor){ .argb = argb };
}

static GRect rect(const ArkFrame *f) {
  return GRect(f->x, f->y, f->w, f->h);
}

static void copy_text(char *dst, const uint8_t *src, size_t len) {
  if (len >= TEXT_MAX) len = TEXT_MAX - 1;
  memcpy(dst, src, len);
  dst[len] = 0;
}

// Gothic at a size the theme's tokens name (ui/theme.toml's pebble-type-*).
static GFont gothic(uint8_t size, bool bold) {
  const char *key;
  switch (size) {
    case 9: key = FONT_KEY_GOTHIC_09; break;
    case 14: key = bold ? FONT_KEY_GOTHIC_14_BOLD : FONT_KEY_GOTHIC_14; break;
    case 24: key = bold ? FONT_KEY_GOTHIC_24_BOLD : FONT_KEY_GOTHIC_24; break;
    case 28: key = bold ? FONT_KEY_GOTHIC_28_BOLD : FONT_KEY_GOTHIC_28; break;
    default: key = bold ? FONT_KEY_GOTHIC_18_BOLD : FONT_KEY_GOTHIC_18; break;
  }
  return fonts_get_system_font(key);
}

static bool fresh(uint8_t id) {
  return id < ARK_LAYERS && s_kind[id] == KIND_NONE;
}

static void place(uint8_t id, uint8_t kind, Layer *layer) {
  s_kind[id] = kind;
  s_layer[id] = layer;
  layer_add_child(window_get_root_layer(s_window), layer);
}

void ark_ui_window(uint8_t ground) {
  window_set_background_color(s_window, color(ground));
}

void ark_ui_bounds(int16_t *w, int16_t *h) {
  GRect b = layer_get_bounds(window_get_root_layer(s_window));
  *w = b.size.w;
  *h = b.size.h;
}

static void click(ClickRecognizerRef r, void *ctx) {
  ark_on_button((uint8_t)click_recognizer_get_button_id(r));
}

static void click_config(void *ctx) {
  window_single_click_subscribe(BUTTON_ID_UP, click);
  window_single_click_subscribe(BUTTON_ID_SELECT, click);
  window_single_click_subscribe(BUTTON_ID_DOWN, click);
}

int16_t ark_ui_action_bar(uint8_t ground) {
  if (!s_bar) {
    s_bar = action_bar_layer_create();
    action_bar_layer_set_background_color(s_bar, color(ground));
    action_bar_layer_add_to_window(s_bar, s_window);
    action_bar_layer_set_click_config_provider(s_bar, click_config);
  }
  return ACTION_BAR_WIDTH;
}

void ark_ui_text(uint8_t id, const ArkFrame *f, uint8_t size, bool bold, uint8_t ink, uint8_t align) {
  if (!fresh(id)) return;
  TextLayer *t = text_layer_create(rect(f));
  text_layer_set_background_color(t, GColorClear);
  text_layer_set_text_color(t, color(ink));
  text_layer_set_font(t, gothic(size, bold));
  text_layer_set_overflow_mode(t, GTextOverflowModeTrailingEllipsis);
  text_layer_set_text_alignment(t, align == 1 ? GTextAlignmentCenter : align == 2 ? GTextAlignmentRight : GTextAlignmentLeft);
  s_words[id][0] = 0;
  text_layer_set_text(t, s_words[id]);
  s_text[id] = t;
  place(id, KIND_TEXT, text_layer_get_layer(t));
}

static void meter_update(Layer *layer, GContext *ctx) {
  Meter *m = layer_get_data(layer);
  GRect b = layer_get_bounds(layer);
  graphics_context_set_fill_color(ctx, color(m->track));
  graphics_fill_rect(ctx, GRect(0, (b.size.h - m->track_h) / 2, b.size.w, m->track_h), 0, GCornerNone);
  graphics_context_set_fill_color(ctx, color(m->fill));
  graphics_fill_rect(ctx, GRect(0, 0, b.size.w * m->permille / 1000, b.size.h), m->corner, GCornersAll);
}

void ark_ui_meter(uint8_t id, const ArkFrame *f, uint8_t fill, uint8_t track, int16_t track_h, int16_t corner) {
  if (!fresh(id)) return;
  Layer *l = layer_create_with_data(rect(f), sizeof(Meter));
  *(Meter *)layer_get_data(l) = (Meter){ .fill = fill, .track = track, .track_h = track_h, .corner = corner };
  layer_set_update_proc(l, meter_update);
  place(id, KIND_METER, l);
}

static void rect_update(Layer *layer, GContext *ctx) {
  Rect *r = layer_get_data(layer);
  graphics_context_set_fill_color(ctx, color(r->color));
  graphics_fill_rect(ctx, layer_get_bounds(layer), r->corner, GCornersAll);
}

void ark_ui_rect(uint8_t id, const ArkFrame *f, uint8_t c, int16_t corner) {
  if (!fresh(id)) return;
  Layer *l = layer_create_with_data(rect(f), sizeof(Rect));
  *(Rect *)layer_get_data(l) = (Rect){ .color = c, .corner = corner };
  layer_set_update_proc(l, rect_update);
  place(id, KIND_RECT, l);
}

void ark_ui_image(uint8_t id, const ArkFrame *f) {
  if (!fresh(id)) return;
  BitmapLayer *b = bitmap_layer_create(rect(f));
  bitmap_layer_set_compositing_mode(b, GCompOpSet);
  bitmap_layer_set_alignment(b, GAlignCenter);
  s_image[id] = b;
  place(id, KIND_IMAGE, bitmap_layer_get_layer(b));
}

static uint16_t list_sections(MenuLayer *menu, void *ctx) {
  return 1;
}

static uint16_t list_rows(MenuLayer *menu, uint16_t section, void *ctx) {
  return ((List *)ctx)->count;
}

static int16_t list_row_height(MenuLayer *menu, MenuIndex *index, void *ctx) {
  return ((List *)ctx)->style.row_height;
}

static int16_t list_header_height(MenuLayer *menu, uint16_t section, void *ctx) {
  List *l = ctx;
  return l->header[0] ? l->style.header_height : 0;
}

static void list_draw_header(GContext *ctx, const Layer *cell, uint16_t section, void *data) {
  List *l = data;
  GRect b = layer_get_bounds(cell);
  const ArkListStyle *s = &l->style;
  graphics_context_set_text_color(ctx, color(s->quiet));
  graphics_draw_text(ctx, l->header, gothic(s->header_size, s->header_bold),
                     GRect(s->inset, -s->lift, b.size.w - 2 * s->inset, b.size.h + s->lift),
                     GTextOverflowModeTrailingEllipsis, GTextAlignmentLeft, NULL);
}

static void list_draw_row(GContext *ctx, const Layer *cell, MenuIndex *index, void *data) {
  List *l = data;
  if (index->row >= l->count) return;
  GRect b = layer_get_bounds(cell);
  const ArkListStyle *s = &l->style;
  int16_t w = b.size.w - 2 * s->inset;
  graphics_context_set_text_color(ctx, color(s->ink));
  graphics_draw_text(ctx, l->rows[index->row][0], gothic(s->title_size, s->title_bold),
                     GRect(s->inset, -s->lift, w, s->title_line), GTextOverflowModeTrailingEllipsis,
                     GTextAlignmentLeft, NULL);
  graphics_context_set_text_color(ctx, color(s->quiet));
  graphics_draw_text(ctx, l->rows[index->row][1], gothic(s->sub_size, s->sub_bold),
                     GRect(s->inset, s->title_line - 2 * s->lift, w, b.size.h - s->title_line + 2 * s->lift),
                     GTextOverflowModeTrailingEllipsis, GTextAlignmentLeft, NULL);
}

void ark_ui_list(uint8_t id, const ArkFrame *f, const ArkListStyle *style) {
  if (!fresh(id)) return;
  List *l = malloc(sizeof(List));
  if (!l) return;
  memset(l, 0, sizeof(List));
  l->style = *style;
  MenuLayer *m = menu_layer_create(rect(f));
  menu_layer_set_callbacks(m, l, (MenuLayerCallbacks){
    .get_num_sections = list_sections,
    .get_num_rows = list_rows,
    .get_cell_height = list_row_height,
    .get_header_height = list_header_height,
    .draw_header = list_draw_header,
    .draw_row = list_draw_row,
  });
  // Nothing is selected on a list the action bar's buttons are beside.
  menu_layer_set_normal_colors(m, color(style->ground), color(style->ink));
  menu_layer_set_highlight_colors(m, color(style->ground), color(style->ink));
  s_menu[id] = m;
  s_list[id] = l;
  place(id, KIND_LIST, menu_layer_get_layer(m));
}

// A glyph's bits (rows of whole bytes, the leftmost pixel the highest bit)
// as a 1-bit bitmap, clear where it is not inked and `ink` where it is.
uint8_t ark_ui_bitmap(int16_t side, const uint8_t *bits, size_t len, uint8_t ink) {
  if (s_bitmap_count >= BITMAP_MAX) return 0xff;
  uint8_t i = s_bitmap_count;
  s_palettes[i][0] = GColorClear;
  s_palettes[i][1] = color(ink);
  GBitmap *b = gbitmap_create_blank_with_palette(GSize(side, side), GBitmapFormat1BitPalette, s_palettes[i], false);
  if (!b) return 0xff;
  uint8_t *data = gbitmap_get_data(b);
  int stride = gbitmap_get_bytes_per_row(b);
  int row = (side + 7) / 8;
  memset(data, 0, stride * side);
  for (int y = 0; y < side && (size_t)((y + 1) * row) <= len; y++) memcpy(data + y * stride, bits + y * row, row);
  s_bitmaps[i] = b;
  s_bitmap_count++;
  return i;
}

void ark_ui_hide(uint8_t id, bool hidden) {
  if (id < ARK_LAYERS && s_layer[id]) layer_set_hidden(s_layer[id], hidden);
}

void ark_ui_set_text(uint8_t id, const uint8_t *s, size_t len) {
  if (id >= ARK_LAYERS || s_kind[id] != KIND_TEXT) return;
  copy_text(s_words[id], s, len);
  text_layer_set_text(s_text[id], s_words[id]);
}

void ark_ui_set_meter(uint8_t id, uint16_t permille) {
  if (id >= ARK_LAYERS || s_kind[id] != KIND_METER) return;
  ((Meter *)layer_get_data(s_layer[id]))->permille = permille > 1000 ? 1000 : permille;
  layer_mark_dirty(s_layer[id]);
}

void ark_ui_set_image(uint8_t id, uint8_t bitmap) {
  if (id >= ARK_LAYERS || s_kind[id] != KIND_IMAGE) return;
  bitmap_layer_set_bitmap(s_image[id], bitmap < s_bitmap_count ? s_bitmaps[bitmap] : NULL);
}

static List *list_of(uint8_t id) {
  if (id >= ARK_LAYERS || s_kind[id] != KIND_LIST) return NULL;
  return s_list[id];
}

void ark_ui_set_header(uint8_t id, const uint8_t *s, size_t len) {
  List *l = list_of(id);
  if (l) copy_text(l->header, s, len);
}

void ark_ui_set_row(uint8_t id, uint8_t i, const uint8_t *t, size_t tl, const uint8_t *s, size_t sl) {
  List *l = list_of(id);
  if (!l || i >= ROW_MAX) return;
  copy_text(l->rows[i][0], t, tl);
  copy_text(l->rows[i][1], s, sl);
}

void ark_ui_set_rows(uint8_t id, uint8_t n) {
  List *l = list_of(id);
  if (!l) return;
  l->count = n < ROW_MAX ? n : ROW_MAX;
  menu_layer_reload_data(s_menu[id]);
}

void ark_ui_set_action(uint8_t button, uint8_t bitmap) {
  if (!s_bar || button < BUTTON_ID_UP || button > BUTTON_ID_DOWN) return;
  if (bitmap < s_bitmap_count) {
    action_bar_layer_set_icon(s_bar, (ButtonId)button, s_bitmaps[bitmap]);
  } else {
    action_bar_layer_clear_icon(s_bar, (ButtonId)button);
  }
}

static void window_load(Window *window) {
  // Buttons reach Rust whether or not the screen has an action bar (whose
  // own provider replaces this one).
  window_set_click_config_provider(window, click_config);
}

static void window_unload(Window *window) {
  if (s_bar) action_bar_layer_destroy(s_bar);
  for (int i = 0; i < ARK_LAYERS; i++) {
    switch (s_kind[i]) {
      case KIND_TEXT: text_layer_destroy(s_text[i]); break;
      case KIND_IMAGE: bitmap_layer_destroy(s_image[i]); break;
      case KIND_LIST:
        menu_layer_destroy(s_menu[i]);
        free(s_list[i]);
        break;
      case KIND_METER:
      case KIND_RECT: layer_destroy(s_layer[i]); break;
      default: break;
    }
    s_kind[i] = KIND_NONE;
  }
  for (int i = 0; i < s_bitmap_count; i++) gbitmap_destroy(s_bitmaps[i]);
  s_bitmap_count = 0;
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
