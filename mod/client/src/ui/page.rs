//! The MODS page's windows, drawn as panels: buttons and list rows on the game's blue
//! panel styles (lighter under the cursor), headings and separators over the game's gold
//! divider, text wrapped to the page, and a scrolled view with a scroll bar and the mouse
//! wheel, as a mod's windows can be longer than the page.
//!
//! A window is cut into pieces (a line of text, a grid row, a list row, a button …) of
//! known height; only the pieces inside the view are drawn, so nothing is squeezed by the
//! page's fixed height. Boxes are solid fills in our own colours (`Api::fill`), as the
//! game's panel styles reach past their frames and fade at the edges; the gold divider is the game's style 0.

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::Value;
use shroudlink::ui::{Event, Node, UiEvent};

use crate::native::{mouse_click_in, mouse_held, mouse_in, Frame, UiStr, STYLE_TAB_TEXT, STYLE_TITLE};

/// Colours, 0xAABBGGRR: navy boxes with a gold rim, like the game's own panels.
pub const BOX: u32 = 0xd8402a1e;
pub const BOX_HOVER: u32 = 0xf0704a38;
pub const BOX_SELECTED: u32 = 0xf0683f28;
/// An ON switch: muted gold (the game shows these colours brighter than written).
const BOX_ON: u32 = 0xe0184058;
/// Toasts: nearly opaque, they show over the game world.
pub const BOX_TOAST: u32 = 0xf0402a1e;
pub const RIM: u32 = 0xe02c7aa8;
pub const RIM_HOVER: u32 = 0xff48b0e8;
const ROW: u32 = 0x40301e14;
const ROW_HOVER: u32 = 0xc0704a38;
const TRACK: u32 = 0x80301e14;
const THUMB: u32 = 0xe02c7aa8;
/// The gold divider line under headings and as separator.
const DIVIDER_STYLE: u32 = 0;
pub unsafe fn boxed(api: &crate::native::Api, frame: &mut Frame, fill: u32, rim: Option<u32>) {
    let r = api.rect(frame);
    match rim {
        Some(rim) => {
            api.fill(frame, r, rim);
            api.fill(frame, [r[0] + 2.0, r[1] + 2.0, (r[2] - 4.0).max(0.0), (r[3] - 4.0).max(0.0)], fill);
        }
        None => api.fill(frame, r, fill),
    }
}

const LINE: f32 = 30.0;
const TITLE: f32 = 40.0;
const DIVIDER: f32 = 14.0;
const BUTTON: f32 = 40.0;
const LIST_ROW: f32 = 38.0;
const SLOT: f32 = 64.0;
const SLOT_GAP: f32 = 4.0;
const INPUT: f32 = 42.0;
const GAP: f32 = 8.0;
const BUTTON_PAD: f32 = 18.0;
/// Width of a character of the page's text style, for wrapping (UI units, about).
const GLYPH: f32 = 13.7;
/// Height of a line of the page's text style.
const TEXT_H: f32 = 24.0;
const SCROLL_WIDTH: f32 = 14.0;
/// UI units one wheel notch scrolls.
const WHEEL_STEP: f32 = 3.0 * LINE;

/// Scroll position of each mod's page, by owner.
static SCROLL: Mutex<Option<HashMap<String, f32>>> = Mutex::new(None);

fn key(name: &'static str, scope: &str, index: u64) -> UiStr {
    super::ident(name, scope, index)
}

/// `text` in lines of at most `width` UI units, broken at spaces.
pub fn wrap(text: &str, width: f32) -> Vec<String> {
    let max = ((width / GLYPH) as usize).max(8);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        let fits = line.is_empty() || line.chars().count() + 1 + word.chars().count() <= max;
        if !fits {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        // a word longer than a line is cut
        let mut word = word.to_string();
        while word.chars().count() > max {
            let head: String = word.chars().take(max).collect();
            lines.push(head);
            word = word.chars().skip(max).collect();
        }
        line.push_str(&word);
    }
    if !line.is_empty() || lines.is_empty() {
        lines.push(line);
    }
    lines
}

/// Height of a node drawn inside a row (one line, nothing wraps there).
fn row_child_height(node: &Node) -> f32 {
    match node {
        Node::Button { .. } | Node::Toggle { .. } => BUTTON,
        Node::TextInput { .. } => INPUT,
        Node::Heading { .. } => TITLE,
        _ => LINE,
    }
}

fn label_width(label: &str) -> f32 {
    label.chars().count() as f32 * GLYPH + 2.0 * BUTTON_PAD
}

/// A panel with a centred label that lights up under the cursor; true when clicked.
pub unsafe fn button(d: &mut super::Draw, key: &UiStr, label: &str, width: f32, height: f32, selected: bool) -> bool {
    tinted_button(d, key, label, width, height, selected, BOX_SELECTED)
}

/// An ON/OFF switch.
pub unsafe fn switch(d: &mut super::Draw, key: &UiStr, on: bool, width: f32, height: f32) -> bool {
    tinted_button(d, key, if on { "ON" } else { "OFF" }, width, height, on, BOX_ON)
}

unsafe fn tinted_button(d: &mut super::Draw, key: &UiStr, label: &str, width: f32, height: f32, selected: bool, selected_fill: u32) -> bool {
    let api = d.api;
    let mut f = Frame::new();
    api.open_column(&mut f, key, 0.0);
    api.set_size(&f, width, height);
    // the label in the middle (alignment places a frame in its parent, not its children)
    let text_w = label.chars().count() as f32 * GLYPH;
    api.set_padding(&f, [((width - text_w) / 2.0).max(6.0), ((height - TEXT_H) / 2.0).max(0.0), 0.0, 0.0]);
    let rect = api.rect(&f);
    let over = mouse_in(rect);
    let (fill, rim) = match (selected, over) {
        (true, false) => (selected_fill, RIM),
        (true, true) => (selected_fill, RIM_HOVER),
        (false, true) => (BOX_HOVER, RIM_HOVER),
        (false, false) => (BOX, RIM),
    };
    boxed(api, &mut f, fill, Some(rim));
    api.text(label, STYLE_TAB_TEXT);
    api.close_frame(&mut f);
    mouse_click_in(rect)
}

/// A row panel with left-aligned text, for lists; true when clicked.
unsafe fn list_row(d: &mut super::Draw, key: &UiStr, text: &str, width: f32) -> bool {
    let api = d.api;
    let mut f = Frame::new();
    api.open_row(&mut f, key, 0.0);
    api.set_size(&f, width, LIST_ROW - 4.0);
    api.set_padding(&f, [14.0, (LIST_ROW - 4.0 - TEXT_H) / 2.0, 14.0, 0.0]);
    let rect = api.rect(&f);
    // a faint band that lights up under the cursor: clickable, but still a list
    boxed(api, &mut f, if mouse_in(rect) { ROW_HOVER } else { ROW }, None);
    api.text(&super::fit_end(text, width - 40.0), STYLE_TAB_TEXT);
    api.close_frame(&mut f);
    mouse_click_in(rect)
}

/// The gold divider, `width` wide.
unsafe fn divider(d: &mut super::Draw, key: &UiStr, width: f32) {
    let api = d.api;
    let mut f = Frame::new();
    api.open_column(&mut f, key, 0.0);
    api.set_size(&f, width, DIVIDER);
    api.background(&mut f, DIVIDER_STYLE, 1.0);
    api.close_frame(&mut f);
}

/// Draws one node at full width or inside a row.
unsafe fn draw_inline(d: &mut super::Draw, owner: &str, window: &str, node: &mut Node, events: &mut Vec<(String, UiEvent)>) {
    let api = d.api;
    let scope = format!("{owner}/{window}");
    let mut event = |node: &str, event: Event| events.push((owner.to_owned(), UiEvent { window: window.to_owned(), node: node.to_owned(), event }));
    match node {
        Node::Text { text } => api.text(text, STYLE_TAB_TEXT),
        Node::Heading { text } => api.text(text, STYLE_TITLE),
        Node::Button { id, label } => {
            let k = key("shroudlinkPageButton", &format!("{scope}/{id}"), 0);
            if button(d, &k, label, label_width(label), BUTTON, false) {
                event(id, Event::Click { index: None });
            }
        }
        Node::Toggle { id, label, value } => {
            // a switch: ON / OFF pill, then the label
            let mut row = Frame::new();
            api.open_row(&mut row, &key("shroudlinkPageToggleRow", &format!("{scope}/{id}"), 0), 12.0);
            api.set_alignment(&row, 0x0001);
            let k = key("shroudlinkPageToggle", &format!("{scope}/{id}"), 0);
            let clicked = switch(d, &k, *value, 84.0, BUTTON);
            api.text(label, STYLE_TAB_TEXT);
            api.close_frame(&mut row);
            if clicked {
                *value = !*value;
                event(id, Event::Change { value: Value::Bool(*value) });
            }
        }
        Node::TextInput { id, value, hint } => {
            let k = key("shroudlinkTextInput", &format!("{scope}/{id}"), 0);
            let before = value.clone();
            let submitted = super::text_field(d, &k, value, hint, super::TEXT_INPUT_WIDTH);
            // every edit too: a button next to the field (Order) needs what was typed
            // without Enter; mods that do not know `change` for a field ignore it
            if *value != before {
                event(id, Event::Change { value: Value::String(value.clone()) });
            }
            if submitted {
                event(id, Event::Submit { value: value.clone() });
            }
        }
        // full-width kinds inside a row are drawn on one line
        Node::Separator => api.text("|", STYLE_TAB_TEXT),
        Node::Row { children } => {
            for child in children.iter_mut() {
                draw_inline(d, owner, window, child, events);
            }
        }
        Node::List { entries, .. } => api.text(&entries.join(" / "), STYLE_TAB_TEXT),
        Node::ItemGrid { items, .. } => api.text(&format!("{} items", items.len()), STYLE_TAB_TEXT),
    }
}

/// The view a page shows: from `offset` down, `height` UI units; pieces are drawn when
/// they fit in completely.
struct View {
    offset: f32,
    height: f32,
    /// where the next piece starts, from the top of the content
    y: f32,
    drawn: bool,
}

impl View {
    /// Takes the next piece of `h` units; true when it is to be drawn.
    fn take(&mut self, h: f32) -> bool {
        let top = self.y;
        self.y += h + GAP;
        let show = top + 0.5 >= self.offset && top + h <= self.offset + self.height;
        self.drawn |= show;
        show
    }
}

/// Draws a window's nodes as pieces into the view.
unsafe fn draw_pieces(d: &mut super::Draw, owner: &str, window: &str, title: &str, nodes: &mut [Node], width: f32, view: &mut View, events: &mut Vec<(String, UiEvent)>) {
    let api = d.api;
    let scope = format!("{owner}/{window}");
    if view.take(TITLE) {
        api.text(title, STYLE_TITLE);
    }
    if view.take(DIVIDER) {
        divider(d, &key("shroudlinkPageTitleLine", &scope, 0), width);
    }
    for (n, node) in nodes.iter_mut().enumerate() {
        let path = format!("{scope}/{n}");
        match node {
            Node::Text { text } => {
                for line in wrap(text, width) {
                    if view.take(LINE) {
                        api.text(&line, STYLE_TAB_TEXT);
                    }
                }
            }
            Node::Heading { text } => {
                if view.take(TITLE) {
                    api.text(text, STYLE_TITLE);
                }
                if view.take(DIVIDER) {
                    divider(d, &key("shroudlinkPageHeadingLine", &path, 0), width * 0.5);
                }
            }
            Node::Separator => {
                if view.take(DIVIDER) {
                    divider(d, &key("shroudlinkPageSeparator", &path, 0), width);
                }
            }
            Node::List { id, entries } => {
                for (i, entry) in entries.iter().enumerate() {
                    if view.take(LIST_ROW - 4.0) && list_row(d, &key("shroudlinkPageListRow", &format!("{scope}/{id}"), i as u64), entry, width) {
                        events.push((owner.to_owned(), UiEvent { window: window.to_owned(), node: id.clone(), event: Event::Click { index: Some(i as u32) } }));
                    }
                }
            }
            Node::ItemGrid { id, columns, items } => {
                let columns = (*columns).max(1) as usize;
                let mut hovered = None;
                let mut any = false;
                for (r, chunk) in items.chunks(columns).enumerate() {
                    if !view.take(SLOT) {
                        continue;
                    }
                    any = true;
                    let mut row = Frame::new();
                    api.open_row(&mut row, &key("shroudlinkPageSlotRow", &format!("{scope}/{id}"), r as u64), SLOT_GAP);
                    api.set_size(&row, width, SLOT);
                    let grid_right = api.rect(&row)[0] + columns as f32 * (SLOT + SLOT_GAP);
                    for (c, item) in chunk.iter().enumerate() {
                        let (clicked, over) = api.item_slot(item.id, item.count, SLOT);
                        if clicked {
                            events.push((owner.to_owned(), UiEvent { window: window.to_owned(), node: id.clone(), event: Event::Click { index: Some((r * columns + c) as u32) } }));
                        }
                        if over {
                            hovered = Some(*item);
                            api.item_tip_beside(grid_right);
                        }
                    }
                    api.close_frame(&mut row);
                }
                // the name of the item under the cursor, where the game's tooltip is missing
                if !api.has_item_tips() && view.take(LINE) && any {
                    let name = hovered.map(|item| {
                        let name = api.item_display_name(item.id).unwrap_or_else(|| format!("item {}", item.id));
                        if item.count > 1 { format!("{name} ×{}", item.count) } else { name }
                    });
                    api.text(&name.unwrap_or_default(), STYLE_TAB_TEXT);
                }
            }
            Node::Row { children } => {
                let h = children.iter().map(row_child_height).fold(LINE, f32::max);
                if view.take(h) {
                    let mut row = Frame::new();
                    api.open_row(&mut row, &key("shroudlinkPageRow", &path, 0), 16.0);
                    api.set_size(&row, width, h);
                    api.set_alignment(&row, 0x0001);
                    for child in children.iter_mut() {
                        draw_inline(d, owner, window, child, events);
                    }
                    api.close_frame(&mut row);
                }
            }
            Node::Button { .. } | Node::Toggle { .. } | Node::TextInput { .. } => {
                if view.take(row_child_height(node)) {
                    draw_inline(d, owner, window, node, events);
                }
            }
        }
    }
    // a little air between windows
    view.y += GAP * 2.0;
}

/// The selected mod's windows in a scrolled view of `width` x `height`, with a scroll bar
/// when they are taller. Events go to `events`.
pub unsafe fn draw_windows(d: &mut super::Draw, owner: &str, windows: &mut [&mut super::OpenWindow], width: f32, height: f32, events: &mut Vec<(String, UiEvent)>) {
    let api = d.api;
    let mut scroll = SCROLL.lock().unwrap();
    let offsets = scroll.get_or_insert_with(HashMap::new);
    let mut offset = offsets.get(owner).copied().unwrap_or(0.0);

    let mut area = Frame::new();
    api.open_row(&mut area, &d.key("shroudlinkPageArea"), 10.0);
    api.set_size(&area, width, height);
    let area_rect = api.rect(&area);

    let inner = width - SCROLL_WIDTH - 10.0;
    let mut content = Frame::new();
    api.open_column(&mut content, &d.key("shroudlinkPageScrolled"), GAP);
    api.set_size(&content, inner, height);
    let mut view = View { offset, height, y: 0.0, drawn: false };
    api.begin_item_tips();
    for w in windows.iter_mut() {
        let (id, title) = (w.window.id.clone(), w.window.title.clone());
        draw_pieces(d, owner, &id, &title, &mut w.window.content, inner, &mut view, events);
    }
    api.close_frame(&mut content);
    let total = view.y;

    // the scroll bar: a track, and a thumb as tall as the share that shows
    let max = (total - height).max(0.0);
    let mut track = Frame::new();
    api.open_column(&mut track, &d.key("shroudlinkPageTrack"), 0.0);
    api.set_size(&track, SCROLL_WIDTH, height);
    let track_rect = api.rect(&track);
    if max > 0.0 {
        boxed(api, &mut track, TRACK, None);
        let thumb_h = (height * height / total).max(40.0);
        let thumb_top = (height - thumb_h) * offset / max;
        api.set_padding(&track, [0.0, thumb_top, 0.0, 0.0]);
        let mut thumb = Frame::new();
        api.open_column(&mut thumb, &d.key("shroudlinkPageThumb"), 0.0);
        api.set_size(&thumb, SCROLL_WIDTH, thumb_h);
        boxed(api, &mut thumb, THUMB, None);
        api.close_frame(&mut thumb);
        // drag anywhere on the track: the thumb follows the cursor
        if let Some(((px, py), (_, y))) = mouse_held() {
            if mouse_in(track_rect) || (px >= track_rect[0] - 8.0 && px < track_rect[0] + track_rect[2] + 8.0 && py >= track_rect[1] && py < track_rect[1] + track_rect[3]) {
                offset = ((y - track_rect[1] - thumb_h / 2.0) / (height - thumb_h)).clamp(0.0, 1.0) * max;
            }
        }
    }
    api.close_frame(&mut track);
    api.close_frame(&mut area);

    // the wheel scrolls while the cursor is over the page
    let wheel = crate::input::take_wheel();
    if wheel != 0 && mouse_in(area_rect) {
        offset -= wheel as f32 / 120.0 * WHEEL_STEP;
    }
    offsets.insert(owner.to_owned(), offset.clamp(0.0, max));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_wraps_at_spaces_and_cuts_long_words() {
        let lines = wrap("one two three four five six", GLYPH * 9.0);
        assert_eq!(lines, vec!["one two", "three", "four five", "six"]);
        assert_eq!(wrap("", 100.0), vec![""]);
        assert_eq!(wrap("abcdefghijklmnopqrstuvwxyz", GLYPH * 10.0), vec!["abcdefghij", "klmnopqrst", "uvwxyz"]);
    }

    #[test]
    fn the_view_shows_what_fits() {
        let mut v = View { offset: 38.0, height: 100.0, y: 0.0, drawn: false };
        // pieces start at 0, 38, 76, 114 (each 30 high, 8 apart)
        assert!(!v.take(30.0), "above the view");
        assert!(v.take(30.0));
        assert!(v.take(30.0));
        assert!(!v.take(60.0), "does not fit at the bottom");
    }
}
