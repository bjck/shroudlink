//! Windows that server mods open on this client (`"mod":"ui"` messages), drawn
//! with the game's own widgets (`native`) inside the game's UI pass.
//!
//! Mod windows are interactive only while a screen with a mouse cursor is open
//! (inventory / chest screens); otherwise a small status line sits on the HUD.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use chestsort_common::log;
use serde_json::Value;
use shroudlink::ui::{Event, HudRow, MenuEntry, Node, ToClient, UiEvent, Window};

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use crate::native::{Api, Frame, Layer, UiStr, LAYER_HUD, LAYER_POPUP, STYLE_TAB_TEXT, STYLE_TITLE, STYLE_TOP_TAB};

pub struct OpenWindow {
    pub owner: String,
    pub window: Window,
}

#[derive(Default)]
pub struct UiState {
    pub windows: Vec<OpenWindow>,
    pub menu: Vec<(String, MenuEntry)>,
    /// rows mods add to the chest screen's backpack panel: (owner, id, nodes)
    sections: Vec<(String, String, Vec<Node>)>,
    /// HUD messages and when they arrived
    notes: Vec<(String, Instant)>,
    /// HUD panels of mods, drawn in this order
    huds: Vec<Hud>,
    /// The HUD panel being dragged: its key and its offsets when the drag began
    drag: Option<(String, (f32, f32))>,
    last_screen: u8,
}

#[derive(Clone)]
struct Hud {
    owner: String,
    id: String,
    title: String,
    rows: Vec<HudRow>,
    /// where it was drawn last (UI units), for test automation
    rect: [f32; 4],
}

impl Hud {
    fn key(&self) -> String {
        format!("{}/{}", self.owner, self.id)
    }
}

pub static UI: Mutex<UiState> = Mutex::new(UiState { windows: Vec::new(), menu: Vec::new(), sections: Vec::new(), notes: Vec::new(), huds: Vec::new(), drag: None, last_screen: 0 });

/// What the UI shows, for test automation.
pub fn snapshot() -> Value {
    let ui = UI.lock().unwrap();
    serde_json::json!({
        "screen": ui.last_screen,
        "page_open": mods_page_open(),
        "menu": ui.menu.iter().map(|(o, e)| serde_json::json!({ "owner": o, "id": e.id, "label": e.label })).collect::<Vec<_>>(),
        "windows": ui.windows.iter().map(|w| serde_json::json!({ "owner": w.owner, "id": w.window.id, "title": w.window.title })).collect::<Vec<_>>(),
        "sections": ui.sections.iter().map(|(o, i, _)| format!("{o}/{i}")).collect::<Vec<_>>(),
        "notes": ui.notes.iter().map(|(t, _)| t.clone()).collect::<Vec<_>>(),
        "huds": ui.huds.iter().map(|h| serde_json::json!({ "key": h.key(), "title": h.title, "rect": h.rect, "rows": h.rows.iter().map(|r| format!("{} {}", r.label, r.value)).collect::<Vec<_>>() })).collect::<Vec<_>>(),
        "search": SEARCH.lock().unwrap().clone(),
        "search_open": SEARCH_OPEN.load(Ordering::Relaxed),
    })
}

/// How long a notification stays, and how many are shown at once.
const NOTE_SECONDS: u64 = 6;
const NOTES_MAX: usize = 4;
/// Width of mod sections in the backpack panel (the search field's row).
const SECTION_WIDTH: f32 = 530.0;

/// HUD panels: width, background (plain blue panel, see-through), bars, and the default
/// distance from the bottom right corner (above the game's own HUD there).
const HUD_WIDTH: f32 = 340.0;
const HUD_PADDING: [f32; 4] = [14.0, 8.0, 14.0, 10.0];
const HUD_STYLE: u32 = 5;
const HUD_ALPHA: f32 = 0.55;
const HUD_BAR_HEIGHT: f32 = 10.0;
/// Style 0 draws a thin gold line with round ends (4 to 7 are blue strips that spill
/// out to the left, 8 to 10 gold ornaments and frames).
const HUD_BAR_STYLE: u32 = 0;
const HUD_BAR_ALPHA: f32 = 1.0;
/// Labels longer than this are cut, so they do not run into the value.
const HUD_LABEL_CHARS: usize = 16;
const HUD_OFFSET: (f32, f32) = (40.0, 260.0);
/// Where the player dragged HUD panels to: `{"meter/dps": [right, bottom]}` in UI units.
const HUD_FILE: &str = "shroudlink_hud.json";

/// Gameplay without a menu; every other screen (journal 0x04, inventories, ...) shows a cursor.
const SCREEN_GAMEPLAY: u8 = 0x01;


const SLOT_SIZE: f32 = 64.0;
const SLOT_GAP: f32 = 4.0;
const PADDING: [f32; 4] = [24.0, 18.0, 24.0, 18.0];
const SPACING: f32 = 8.0;
/// Approximate heights in UI units, used to size panels (frames without a height fill their parent).
const TEXT_HEIGHT: f32 = 30.0;
const TITLE_HEIGHT: f32 = 40.0;
const SEPARATOR_HEIGHT: f32 = 10.0;
/// The MODS page below the player menu tab row.
const PAGE_WIDTH: f32 = 1800.0;
const PAGE_HEIGHT: f32 = 880.0;
const PAGE_TOP: f32 = 140.0;
const PAGE_LIST_WIDTH: f32 = 340.0;
/// Background of the MODS page: the plain blue panel.
const PAGE_STYLE: u32 = 5;
/// Entries per column of a list node.
const LIST_ROWS: usize = 6;

/// UI frames drawn so far; the MODS tab records the frame it was drawn in, so the
/// page is only shown while the player menu is actually on screen.
static FRAME: AtomicU64 = AtomicU64::new(0);
static TAB_FRAME: AtomicU64 = AtomicU64::new(u64::MAX);
static PAGE_OPEN: AtomicBool = AtomicBool::new(false);
/// Key of the focused text field (0: none); while set the keyboard goes to it.
static FOCUS: AtomicU64 = AtomicU64::new(0);
const TEXT_INPUT_WIDTH: f32 = 520.0;
const TEXT_INPUT_MAX: usize = 64;
/// Background of text fields: blue panel with a fine edge.
const TEXT_INPUT_STYLE: u32 = 3;
/// Game tab index when the page was opened; choosing another game tab closes the page.
static PAGE_GAME_TAB: AtomicU64 = AtomicU64::new(u64::MAX);
/// Index into the mod menu of the entry shown on the page.
static PAGE_SELECTED: AtomicU64 = AtomicU64::new(0);

/// Development hot reload: the page state survives in the process environment.
const PAGE_VAR: &str = "SHROUDLINK_PAGE";

pub fn restore_page() {
    if let Some((open, selected)) = std::env::var(PAGE_VAR).ok().and_then(|v| {
        let (o, s) = v.split_once(':')?;
        Some((o == "1", s.parse::<u64>().ok()?))
    }) {
        PAGE_OPEN.store(open, Ordering::Relaxed);
        PAGE_SELECTED.store(selected, Ordering::Relaxed);
    }
}

fn save_page() {
    std::env::set_var(PAGE_VAR, format!("{}:{}", mods_page_open() as u8, PAGE_SELECTED.load(Ordering::Relaxed)));
}

/// Item search of inventory and chest screens (lower case); slots that do not
/// match are drawn faded while it is not empty.
static SEARCH: Mutex<String> = Mutex::new(String::new());
const SEARCH_KEY: &str = "shroudlinkItemSearch";
/// Screens of chests and other storages (the chest screen dispatches on 0x16, 0x1c and 0x29).
const CHEST_SCREENS: [u8; 3] = [0x16, 0x1c, 0x29];
const SEARCH_ROW_WIDTH: f32 = 210.0;
/// The open search field below the title, and the room the slot grid makes for it.
const SEARCH_FIELD_WIDTH: f32 = 470.0;

/// Position of the search inside the backpack panel's header (UI units from its top left).
const SEARCH_LEFT: f32 = 60.0;
const SEARCH_TOP: f32 = 44.0;
/// Panel whose header gets the search (0: backpack on the chest screen).
const SEARCH_PANEL: u8 = 0;
/// The search field is shown (otherwise only the "Search" label).
static SEARCH_OPEN: AtomicBool = AtomicBool::new(false);
/// A chest screen is shown: only there the search greys out slots.
static SEARCH_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Runs `f` with the active item search, if any.
pub fn with_search<R>(f: impl FnOnce(&str) -> R) -> Option<R> {
    if !SEARCH_ACTIVE.load(Ordering::Relaxed) {
        return None;
    }
    let query = SEARCH.lock().unwrap();
    (!query.trim().is_empty()).then(|| f(&query.to_lowercase()))
}

pub fn mods_page_open() -> bool {
    PAGE_OPEN.load(Ordering::Relaxed)
}

/// Draws the MODS tab inside the player menu tab row (called from the tab widget hook).
pub fn draw_mods_tab(api: &Api) {
    TAB_FRAME.store(FRAME.load(Ordering::Relaxed), Ordering::Relaxed);
    let label = if mods_page_open() { "» Mods «" } else { "Mods" };
    if unsafe { api.text_button(label, STYLE_TOP_TAB, shroudlink::string_id(b"shroudlinkModsTab")) } {
        let open = !mods_page_open();
        PAGE_OPEN.store(open, Ordering::Relaxed);
        save_page();
        PAGE_GAME_TAB.store(unsafe { api.player_tab() }.unwrap_or(u64::MAX), Ordering::Relaxed);
        if open {
            if let Some((owner, entry)) = UI.lock().unwrap().menu.get(PAGE_SELECTED.load(Ordering::Relaxed) as usize).cloned() {
                crate::link::send(UiEvent { window: String::new(), node: entry.id, event: Event::Menu }.to_message(&owner));
            }
        }
    }
}

fn handle(message: &Value) {
    let mut m = message.clone();
    if let Some(o) = m.as_object_mut() {
        o.remove("mod");
    }
    let op: ToClient = match serde_json::from_value(m) {
        Ok(op) => op,
        Err(e) => {
            log!("ui: unsupported message ({e})");
            return;
        }
    };
    let mut ui = UI.lock().unwrap();
    match op {
        ToClient::Open { owner, window } => {
            log!("ui: {owner} opens '{}' ({} nodes, {} bytes)", window.id, window.content.len(), message.to_string().len());
            ui.windows.retain(|w| !(w.owner == owner && w.window.id == window.id));
            ui.windows.push(OpenWindow { owner, window });
        }
        ToClient::Close { owner, id } => ui.windows.retain(|w| !(w.owner == owner && w.window.id == id)),
        ToClient::Entries { owner, entries } => {
            ui.menu.retain(|(o, _)| *o != owner);
            ui.menu.extend(entries.into_iter().map(|e| (owner.clone(), e)));
        }
        // inventory building blocks: the slot module keeps them (its own lock)
        ToClient::SlotKeys { owner, keys } => {
            drop(ui);
            crate::slots::set_keys(owner, keys);
        }
        ToClient::Marks { owner, marks } => {
            drop(ui);
            crate::slots::set_marks(owner, marks);
        }
        ToClient::Store { owner, data } => {
            drop(ui);
            crate::slots::store(&owner, data);
        }
        ToClient::Section { owner, id, nodes } => {
            ui.sections.retain(|(o, i, _)| !(*o == owner && *i == id));
            if !nodes.is_empty() {
                ui.sections.push((owner, id, nodes));
            }
        }
        ToClient::Hud { owner, id, title, rows } => {
            let at = ui.huds.iter().position(|h| h.owner == owner && h.id == id);
            match (at, rows.is_empty()) {
                (Some(i), true) => drop(ui.huds.remove(i)),
                (Some(i), false) => (ui.huds[i].title, ui.huds[i].rows) = (title, rows),
                (None, false) => {
                    log!("ui: {owner} shows HUD panel {id}");
                    ui.huds.push(Hud { owner, id, title, rows, rect: [0.0; 4] });
                }
                (None, true) => {}
            }
        }
        ToClient::Notify { owner, text } => {
            log!("ui: {owner} notifies \"{text}\"");
            ui.notes.push((text, Instant::now()));
            let excess = ui.notes.len().saturating_sub(NOTES_MAX);
            ui.notes.drain(..excess);
        }
    }
}

pub fn install() {
    restore_page();
    crate::link::on_message("ui", handle);
}

/// Called every client frame on the game thread.
pub fn tick() {}

static DRAWS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Collected while drawing, sent after the UI lock is released.
struct Pending {
    events: Vec<(String, UiEvent)>,
    closed: Vec<(String, String)>,
}

/// Per-frame drawing state: hands out frame keys. The game identifies frames by
/// key across frames, so every frame needs its own, stable key; drawing in the
/// same order each frame keeps the counter stable.
struct Draw<'a> {
    api: &'a Api,
    next_key: u64,
}

impl Draw<'_> {
    /// Key for fixed scaffolding frames, drawn in the same order every frame.
    fn key(&mut self, name: &'static str) -> UiStr {
        self.next_key += 1;
        UiStr::keyed(name, self.next_key)
    }
}

/// Key derived from what a frame shows (owner/window/node path or id, plus an index),
/// so it stays the same when other content changes. The game keeps hover and press
/// state per key: a key that moves to another button makes that button fire.
fn ident(name: &'static str, scope: &str, index: u64) -> UiStr {
    UiStr::keyed(name, shroudlink::string_id(scope.as_bytes()) ^ index.wrapping_mul(0x2545_f491_4f6c_dd1d))
}

/// Draws mod UI after the game's own UI for this frame.
pub fn draw(api: &Api) {
    let screen = unsafe { api.screen() };
    let mut pending = Pending { events: Vec::new(), closed: Vec::new() };
    {
        let mut ui = UI.lock().unwrap();
        if screen != ui.last_screen {
            log!("ui: screen {:#04x} -> {screen:#04x}", ui.last_screen);
            ui.last_screen = screen;
        }
        if screen == 0 {
            return;
        }
        let frame = FRAME.fetch_add(1, Ordering::Relaxed);
        let menu_visible = TAB_FRAME.load(Ordering::Relaxed) == frame;
        if mods_page_open() && (!menu_visible || unsafe { api.player_tab() } != Some(PAGE_GAME_TAB.load(Ordering::Relaxed))) {
            // player menu closed or another game tab chosen
            PAGE_OPEN.store(false, Ordering::Relaxed);
            save_page();
        }
        let page = mods_page_open() && menu_visible;
        // mod windows live on the MODS page only; they never float over other screens
        if DRAWS.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 3600 == 0 {
            log!("ui: screen {screen:#04x} page={page} menu={} open={}", ui.menu.len(), ui.windows.len());
        }
        let mut d = Draw { api, next_key: 0 };
        // item search only where inventories are shown: chest screens and the backpack tab
        // the chest screen embeds the item search in its backpack panel (after_panel_header)
        let search = !page && CHEST_SCREENS.contains(&screen);
        crate::input::arm_search_hotkey(search);
        // slot keys (chestsort's L) act on any menu screen (not while playing, not on the MODS page)
        crate::input::allow_slot_keys(!page && screen != SCREEN_GAMEPLAY);
        // the search term stays across chests (looking for something means opening many)
        SEARCH_ACTIVE.store(search, Ordering::Relaxed);
        if !page && !search && FOCUS.swap(0, Ordering::Relaxed) != 0 {
            crate::input::set_focus(false);
        }
        ui.notes.retain(|(_, at)| at.elapsed() < Duration::from_secs(NOTE_SECONDS));
        unsafe {
            if page {
                draw_page(&mut d, &mut ui, &mut pending);
            } else {
                draw_notes(&mut d, &ui.notes);
                draw_huds(&mut d, &mut ui, screen != SCREEN_GAMEPLAY);
            }
        }
        for (owner, id) in &pending.closed {
            ui.windows.retain(|w| !(&w.owner == owner && &w.window.id == id));
        }
    }
    // an L press is meant for the slot under the cursor in the frame it happened
    crate::input::clear_slot_key();
    crate::automation::end_frame();
    for (owner, event) in pending.events {
        log!("ui: event {owner}/{}/{} {:?}", event.window, event.node, event.event);
        crate::link::send(event.to_message(&owner));
    }
}

/// The chest screen draws an inventory panel header (title row); index 0 is the backpack.
/// The item search is embedded right below it, inside the panel's own layout, so the
/// game moves the slot grid down instead of anything covering it.
pub fn after_panel_header(api: &Api, index: u8) {
    if index != SEARCH_PANEL {
        return;
    }
    let key = ident(SEARCH_KEY, "client", 0);
    if crate::input::search_requested() {
        SEARCH_OPEN.store(true, Ordering::Relaxed);
        FOCUS.store(key.id(), Ordering::Relaxed);
        crate::input::set_focus(true);
    }
    if SEARCH_OPEN.load(Ordering::Relaxed) {
        return; // the field is drawn below the title (before_backpack_grid)
    }
    unsafe {
        // the panel container overlays this frame: a full-size wrapper with padding puts the
        // label into the title line, left of the centred title
        let mut wrapper = Frame::new();
        api.open_column(&mut wrapper, &ident("shroudlinkSearchWrapper", "client", 0), 0.0);
        api.set_alignment(&wrapper, 0x0101);
        api.set_padding(&wrapper, [SEARCH_LEFT, SEARCH_TOP, 0.0, 0.0]);
        let mut row = Frame::new();
        api.open_row(&mut row, &ident("shroudlinkSearchRow", "client", 0), 8.0);
        api.set_size(&row, SEARCH_ROW_WIDTH, TEXT_HEIGHT + 12.0);
        if api.text_button("Search ( / )", STYLE_TAB_TEXT, ident("shroudlinkSearchOpen", "client", 0).id()) {
            SEARCH_OPEN.store(true, Ordering::Relaxed);
            FOCUS.store(key.id(), Ordering::Relaxed);
            crate::input::set_focus(true);
        }
        api.close_frame(&mut row);
        api.close_frame(&mut wrapper);
    }
}

/// HUD messages from mods, top centre, newest last.
unsafe fn draw_notes(d: &mut Draw, notes: &[(String, Instant)]) {
    if notes.is_empty() {
        return;
    }
    let api = d.api;
    let mut layer = Layer::new();
    let root = api.begin_layer(&mut layer, &UiStr::name("shroudlinkNotes"), LAYER_HUD);
    let mut root_frame = Frame::new();
    api.open_root(&mut root_frame, root);
    let mut column = Frame::new();
    api.open_column(&mut column, &d.key("shroudlinkNotes"), 4.0);
    api.set_alignment(&column, 0x0102);
    api.set_padding(&column, [0.0, 160.0, 0.0, 0.0]);
    for (text, _) in notes {
        api.text(text, STYLE_TITLE);
    }
    api.close_frame(&mut column);
    api.close_frame(&mut root_frame);
    api.end_layer(&mut layer);
}

static HUD_POSITIONS: Mutex<Option<serde_json::Map<String, Value>>> = Mutex::new(None);

fn hud_offset(key: &str) -> (f32, f32) {
    let mut positions = HUD_POSITIONS.lock().unwrap();
    let positions = positions.get_or_insert_with(|| {
        std::fs::read_to_string(chestsort_common::game_dir().join(HUD_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
    });
    positions.get(key).and_then(|v| Some((v.get(0)?.as_f64()? as f32, v.get(1)?.as_f64()? as f32))).unwrap_or(HUD_OFFSET)
}

fn set_hud_offset(key: &str, (right, bottom): (f32, f32), save: bool) {
    let mut positions = HUD_POSITIONS.lock().unwrap();
    let positions = positions.get_or_insert_with(Default::default);
    positions.insert(key.to_owned(), serde_json::json!([right.round(), bottom.round()]));
    if save {
        let _ = std::fs::write(chestsort_common::game_dir().join(HUD_FILE), serde_json::to_string_pretty(positions).unwrap_or_default());
    }
}

/// Mod panels on the HUD, bottom right. While a menu shows the cursor they sit above it
/// and can be dragged anywhere; the addon keeps where.
unsafe fn draw_huds(d: &mut Draw, ui: &mut UiState, menu: bool) {
    let api = d.api;
    let inner = HUD_WIDTH - HUD_PADDING[0] - HUD_PADDING[2];
    for hud in ui.huds.clone() {
        let key = hud.key();
        let (right, bottom) = hud_offset(&key);
        let mut layer = Layer::new();
        let root = api.begin_layer(&mut layer, &ident("shroudlinkHud", &key, 0), if menu { LAYER_POPUP } else { LAYER_HUD });
        let mut root_frame = Frame::new();
        api.open_root(&mut root_frame, root);
        let mut column = Frame::new();
        api.open_column(&mut column, &ident("shroudlinkHudColumn", &key, 0), 0.0);
        api.set_alignment(&column, 0x0303);
        api.set_padding(&column, [0.0, 0.0, right, bottom]);
        let height = HUD_PADDING[1] + HUD_PADDING[3] + TEXT_HEIGHT + hud.rows.iter().map(|r| TEXT_HEIGHT + if r.fraction > 0.0 { HUD_BAR_HEIGHT } else { 0.0 }).sum::<f32>();
        let mut panel = Frame::new();
        api.open_column(&mut panel, &ident("shroudlinkHudPanel", &key, 0), 0.0);
        api.set_size(&panel, HUD_WIDTH, height);
        api.set_padding(&panel, HUD_PADDING);
        api.background(&mut panel, HUD_STYLE, HUD_ALPHA);
        api.text(&hud.title, STYLE_TAB_TEXT);
        for (i, row) in hud.rows.iter().enumerate() {
            let i = i as u64;
            let mut line = Frame::new();
            api.open_row(&mut line, &ident("shroudlinkHudLine", &key, i), 0.0);
            api.set_size(&line, inner, TEXT_HEIGHT);
            let mut label = Frame::new();
            api.open_column(&mut label, &ident("shroudlinkHudLabel", &key, i), 0.0);
            api.set_size(&label, inner * 0.62, TEXT_HEIGHT);
            api.text(&clip(&row.label, HUD_LABEL_CHARS), STYLE_TITLE);
            api.close_frame(&mut label);
            let mut value = Frame::new();
            api.open_column(&mut value, &ident("shroudlinkHudValue", &key, i), 0.0);
            api.set_size(&value, inner * 0.38, TEXT_HEIGHT);
            // right-aligned: a column fitting the text, at the end of the fixed one
            let mut fit = Frame::new();
            api.open_column(&mut fit, &ident("shroudlinkHudValueText", &key, i), 0.0);
            api.set_alignment(&fit, 0x0103);
            api.text(&row.value, STYLE_TITLE);
            api.close_frame(&mut fit);
            api.close_frame(&mut value);
            api.close_frame(&mut line);
            if row.fraction > 0.0 {
                let mut bar = Frame::new();
                api.open_row(&mut bar, &ident("shroudlinkHudBar", &key, i), 0.0);
                api.set_size(&bar, (inner * row.fraction.min(1.0)).max(2.0), HUD_BAR_HEIGHT);
                api.background(&mut bar, HUD_BAR_STYLE, HUD_BAR_ALPHA);
                api.close_frame(&mut bar);
            }
        }
        let rect = api.rect(&panel);
        api.close_frame(&mut panel);
        api.close_frame(&mut column);
        api.close_frame(&mut root_frame);
        api.end_layer(&mut layer);
        if let Some(h) = ui.huds.iter_mut().find(|h| h.key() == key) {
            h.rect = rect;
        }
        drag_hud(ui, &key, rect, (right, bottom), menu);
    }
}

/// `text` cut to `max` characters, with an ellipsis.
fn clip(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_owned();
    }
    let mut cut: String = text.chars().take(max - 1).collect();
    cut.push('…');
    cut
}

/// Drags a HUD panel while the button is held after a press inside it (menus only).
fn drag_hud(ui: &mut UiState, key: &str, rect: [f32; 4], offset: (f32, f32), menu: bool) {
    let inside = |(x, y): (f32, f32)| x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3];
    match crate::native::mouse_held().filter(|_| menu) {
        Some((at, pos)) => {
            if ui.drag.is_none() && inside(at) {
                ui.drag = Some((key.to_owned(), offset));
            }
            if let Some((_, (right, bottom))) = ui.drag.as_ref().filter(|(k, _)| k == key) {
                set_hud_offset(key, ((right - (pos.0 - at.0)).max(0.0), (bottom - (pos.1 - at.1)).max(0.0)), false);
            }
        }
        None => {
            if ui.drag.as_ref().is_some_and(|(k, _)| k == key) {
                ui.drag = None;
                let offset = hud_offset(key);
                set_hud_offset(key, offset, true);
                log!("ui: HUD panel {key} moved to {offset:?}");
            }
        }
    }
}

/// Called inside the backpack panel's column right before its slot grid: the open search
/// field and the mods' sections sit below the title, and the game moves the grid down to
/// make room.
pub fn before_backpack_grid(api: &Api) {
    draw_search_field(api);
    let mut d = Draw { api, next_key: 0 };
    let mut pending = Pending { events: Vec::new(), closed: Vec::new() };
    {
        let mut ui = UI.lock().unwrap();
        for (owner, id, nodes) in ui.sections.iter_mut() {
            unsafe {
                let height: f32 = nodes.iter().map(node_height).sum::<f32>() + SPACING * nodes.len() as f32;
                let mut column = Frame::new();
                api.open_column(&mut column, &ident("shroudlinkSection", &format!("{owner}/{id}"), 0), SPACING);
                api.set_size(&column, SECTION_WIDTH, height);
                for (i, node) in nodes.iter_mut().enumerate() {
                    draw_node(&mut d, owner, id, &i.to_string(), node, SECTION_WIDTH, &mut pending);
                }
                api.close_frame(&mut column);
            }
        }
    }
    for (owner, event) in pending.events {
        log!("ui: section event {owner}/{}/{} {:?}", event.window, event.node, event.event);
        crate::link::send(event.to_message(&owner));
    }
}

fn draw_search_field(api: &Api) {
    if !SEARCH_OPEN.load(Ordering::Relaxed) {
        return;
    }
    let mut d = Draw { api, next_key: 0 };
    let key = ident(SEARCH_KEY, "client", 0);
    unsafe {
        let mut row = Frame::new();
        api.open_row(&mut row, &ident("shroudlinkSearchField", "client", 0), 12.0);
        api.set_size(&row, SEARCH_FIELD_WIDTH + 60.0, TEXT_HEIGHT + 20.0);
        api.set_padding(&row, [0.0, 4.0, 0.0, 4.0]);
        let mut query = SEARCH.lock().unwrap();
        let was_focused = FOCUS.load(Ordering::Relaxed) == key.id();
        text_field(&mut d, &key, &mut query, "Item name", SEARCH_FIELD_WIDTH);
        if api.text_button("X", STYLE_TAB_TEXT, ident("shroudlinkSearchClear", "client", 0).id()) {
            query.clear();
            collapse_search();
        }
        // Esc on an empty field leaves it: back to the label in the title line
        if was_focused && FOCUS.load(Ordering::Relaxed) != key.id() && query.is_empty() {
            collapse_search();
        }
        drop(query);
        api.close_frame(&mut row);
    }
}

fn collapse_search() {
    SEARCH_OPEN.store(false, Ordering::Relaxed);
    FOCUS.store(0, Ordering::Relaxed);
    crate::input::set_focus(false);
}

/// Average glyph width of the tab text style (capitals), in UI units.
const GLYPH_WIDTH: f32 = 12.0;

/// `text` shortened from the front with "…" to fit `width` UI units.
fn fit_end(text: &str, width: f32) -> String {
    let max = ((width / GLYPH_WIDTH) as usize).max(4);
    let count = text.chars().count();
    if count <= max {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(count - (max - 1)).collect();
    format!("…{tail}")
}

/// A text field editing `value` in place; returns true when Enter was pressed.
unsafe fn text_field(d: &mut Draw, key: &UiStr, value: &mut String, hint: &str, width: f32) -> bool {
    let api = d.api;
    let focused = FOCUS.load(Ordering::Relaxed) == key.id();
    let mut field = Frame::new();
    api.open_column(&mut field, key, 0.0);
    api.set_size(&field, width, TEXT_HEIGHT + 12.0);
    api.set_padding(&field, [10.0, 6.0, 10.0, 6.0]);
    api.background(&mut field, TEXT_INPUT_STYLE, 1.0);
    let shown = if focused {
        format!("{value}|")
    } else if value.is_empty() {
        format!("{hint}…")
    } else {
        value.clone()
    };
    // the game's text widget does not clip: show only the end of long input
    api.text(&fit_end(&shown, width - 32.0), STYLE_TAB_TEXT);
    let rect = api.rect(&field);
    api.close_frame(&mut field);
    let mut submitted = false;
    if crate::native::mouse_click_in(rect) {
        FOCUS.store(key.id(), Ordering::Relaxed);
        crate::input::set_focus(true);
    } else if focused {
        if crate::native::mouse_clicked() {
            // a click somewhere else ends the editing
            FOCUS.store(0, Ordering::Relaxed);
            crate::input::set_focus(false);
        }
        for k in crate::input::take_keys() {
            match k {
                crate::input::Key::Char(c) if value.chars().count() < TEXT_INPUT_MAX => value.push(c),
                crate::input::Key::Char(_) => {}
                crate::input::Key::Backspace => {
                    value.pop();
                }
                crate::input::Key::Enter => {
                    submitted = true;
                    FOCUS.store(0, Ordering::Relaxed);
                    crate::input::set_focus(false);
                }
                // first Esc clears the text, the second one leaves the field
                crate::input::Key::Escape if !value.is_empty() => value.clear(),
                crate::input::Key::Escape => {
                    FOCUS.store(0, Ordering::Relaxed);
                    crate::input::set_focus(false);
                }
            }
        }
    }
    submitted
}


/// The MODS page: mod entries on the left, the selected mod's windows on the right.
unsafe fn draw_page(d: &mut Draw, ui: &mut UiState, pending: &mut Pending) {
    let api = d.api;
    let mut layer = Layer::new();
    let root = api.begin_layer(&mut layer, &UiStr::name("shroudlinkPage"), LAYER_POPUP);
    let mut root_frame = Frame::new();
    api.open_root(&mut root_frame, root);
    let mut column = Frame::new();
    api.open_column(&mut column, &d.key("shroudlinkPageColumn"), 0.0);
    api.set_alignment(&column, 0x0102);
    api.set_padding(&column, [0.0, PAGE_TOP, 0.0, 0.0]);

    let mut page = Frame::new();
    api.open_row(&mut page, &d.key("shroudlinkPageRow"), 24.0);
    api.set_size(&page, PAGE_WIDTH, PAGE_HEIGHT);
    api.set_padding(&page, PADDING);
    api.background(&mut page, PAGE_STYLE, 1.0);


    // left: one entry per mod menu item
    let mut list = Frame::new();
    api.open_column(&mut list, &d.key("shroudlinkPageList"), 6.0);
    api.set_size(&list, PAGE_LIST_WIDTH, PAGE_HEIGHT - PADDING[1] - PADDING[3]);
    api.text("Server mods", STYLE_TITLE);
    let selected = PAGE_SELECTED.load(Ordering::Relaxed) as usize;
    for (i, (owner, entry)) in ui.menu.iter().enumerate() {
        let label = if i == selected { format!("» {}", entry.label) } else { entry.label.clone() };
        if api.text_button(&label, STYLE_TAB_TEXT, ident("shroudlinkModEntry", &format!("{owner}/{}", entry.id), 0).id()) {
            PAGE_SELECTED.store(i as u64, Ordering::Relaxed);
            save_page();
            pending.events.push((owner.clone(), UiEvent { window: String::new(), node: entry.id.clone(), event: Event::Menu }));
        }
    }
    if ui.menu.is_empty() {
        let text = if crate::link::connected() { "This server has no mods with a menu." } else { "Not connected to a shroudlink server." };
        api.text(text, STYLE_TAB_TEXT);
    }
    api.close_frame(&mut list);

    // right: the windows of the selected mod
    let owner = ui.menu.get(selected).map(|(o, _)| o.clone());
    let width = PAGE_WIDTH - PAGE_LIST_WIDTH - PADDING[0] - PADDING[2] - 24.0;
    let mut content = Frame::new();
    api.open_column(&mut content, &d.key("shroudlinkPageContent"), SPACING);
    api.set_size(&content, width, PAGE_HEIGHT - PADDING[1] - PADDING[3]);
    for w in ui.windows.iter_mut().filter(|w| Some(&w.owner) == owner.as_ref()) {
        api.text(&w.window.title, STYLE_TITLE);
        let window_id = w.window.id.clone();
        for (i, node) in w.window.content.iter_mut().enumerate() {
            draw_node(d, &w.owner, &window_id, &i.to_string(), node, width, pending);
        }
    }
    api.close_frame(&mut content);

    api.close_frame(&mut page);
    api.close_frame(&mut column);
    api.close_frame(&mut root_frame);
    api.end_layer(&mut layer);
}

fn node_height(node: &Node) -> f32 {
    match node {
        Node::Text { .. } | Node::Button { .. } | Node::Toggle { .. } | Node::TextInput { .. } => TEXT_HEIGHT,
        Node::Heading { .. } => TITLE_HEIGHT,
        Node::Separator => SEPARATOR_HEIGHT,
        Node::Row { children } => children.iter().map(node_height).fold(TEXT_HEIGHT, f32::max),
        Node::List { entries, .. } => entries.len().min(LIST_ROWS) as f32 * (TEXT_HEIGHT + 2.0),
        Node::ItemGrid { columns, items, .. } => {
            let rows = items.len().div_ceil((*columns).max(1) as usize);
            // plus a line for the name of the item under the cursor
            rows as f32 * (SLOT_SIZE + SLOT_GAP) + TEXT_HEIGHT
        }
    }
}


unsafe fn draw_node(d: &mut Draw, owner: &str, window: &str, path: &str, node: &mut Node, width: f32, pending: &mut Pending) {
    let api = d.api;
    let scope = format!("{owner}/{window}/{path}");
    let height = node_height(node);
    let mut event = |node: &str, event: Event| {
        pending.events.push((owner.to_owned(), UiEvent { window: window.to_owned(), node: node.to_owned(), event }));
    };
    match node {
        Node::Text { text } => api.text(text, STYLE_TAB_TEXT),
        Node::Heading { text } => api.text(text, STYLE_TITLE),
        Node::Separator => {
            let mut f = Frame::new();
            api.open_column(&mut f, &ident("shroudlinkSeparator", &scope, 0), 0.0);
            api.set_size(&f, width, SEPARATOR_HEIGHT);
            api.close_frame(&mut f);
        }
        Node::Row { children } => {
            let mut row = Frame::new();
            api.open_row(&mut row, &ident("shroudlinkRow", &scope, 0), 16.0);
            api.set_size(&row, width, height);
            for (i, child) in children.iter_mut().enumerate() {
                draw_node(d, owner, window, &format!("{path}.{i}"), child, width, pending);
            }
            api.close_frame(&mut row);
        }
        Node::Button { id, label } => {
            if api.text_button(&format!("[ {label} ]"), STYLE_TAB_TEXT, ident("shroudlinkButton", &format!("{owner}/{window}/{id}"), 0).id()) {
                event(id, Event::Click { index: None });
            }
        }
        Node::Toggle { id, label, value } => {
            // the game font has no check box glyphs
            let mark = if *value { "[x]" } else { "[ ]" };
            if api.text_button(&format!("{mark} {label}"), STYLE_TAB_TEXT, ident("shroudlinkToggle", &format!("{owner}/{window}/{id}"), 0).id()) {
                *value = !*value;
                event(id, Event::Change { value: Value::Bool(*value) });
            }
        }
        Node::TextInput { id, value, hint } => {
            let key = ident("shroudlinkTextInput", &format!("{owner}/{window}/{id}"), 0);
            if text_field(d, &key, value, hint, TEXT_INPUT_WIDTH.min(width)) {
                event(id, Event::Submit { value: value.clone() });
            }
        }
        Node::List { id, entries } => {
            // long lists flow into columns of LIST_ROWS entries
            let columns = entries.len().div_ceil(LIST_ROWS).max(1);
            let mut list = Frame::new();
            api.open_row(&mut list, &ident("shroudlinkList", &format!("{owner}/{window}/{id}"), 0), 24.0);
            api.set_size(&list, width, height);
            for (c, chunk) in entries.chunks(LIST_ROWS).enumerate() {
                let mut col = Frame::new();
                api.open_column(&mut col, &ident("shroudlinkListColumn", &format!("{owner}/{window}/{id}"), c as u64), 2.0);
                api.set_size(&col, width / columns as f32 - 24.0, height);
                for (r, entry) in chunk.iter().enumerate() {
                    if api.text_button(entry, STYLE_TAB_TEXT, ident("shroudlinkListEntry", &format!("{owner}/{window}/{id}"), (c * LIST_ROWS + r) as u64).id()) {
                        event(id, Event::Click { index: Some((c * LIST_ROWS + r) as u32) });
                    }
                }
                api.close_frame(&mut col);
            }
            api.close_frame(&mut list);
        }
        Node::ItemGrid { id, columns, items } => {
            let columns = (*columns).max(1) as usize;
            let mut grid = Frame::new();
            api.open_column(&mut grid, &ident("shroudlinkGrid", &format!("{owner}/{window}/{id}"), 0), SLOT_GAP);
            api.set_size(&grid, width, height);
            let mut hovered = None;
            for (r, chunk) in items.chunks(columns).enumerate() {
                let mut row = Frame::new();
                api.open_row(&mut row, &ident("shroudlinkSlotRow", &format!("{owner}/{window}/{id}"), r as u64), SLOT_GAP);
                api.set_size(&row, width, SLOT_SIZE);
                for (c, item) in chunk.iter().enumerate() {
                    let (clicked, over) = api.item_slot(item.id, item.count, SLOT_SIZE);
                    if clicked {
                        event(id, Event::Click { index: Some((r * columns + c) as u32) });
                    }
                    if over {
                        hovered = Some(item);
                    }
                }
                api.close_frame(&mut row);
            }
            // the name of the item under the cursor, in the game's language
            let name = hovered.map(|item| {
                let name = api.item_display_name(item.id).unwrap_or_else(|| format!("item {}", item.id));
                if item.count > 1 { format!("{name} ×{}", item.count) } else { name }
            });
            api.text(&name.unwrap_or_default(), STYLE_TAB_TEXT);
            api.close_frame(&mut grid);
        }
    }
}
