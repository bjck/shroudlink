//! Generic inventory building blocks for server mods (see `shroudlink::ui`): slot hotkeys,
//! slot marks, per-server storage on this PC and the backpack layout. The addon only draws
//! and stores; what a key or a mark means is up to the mod (chestsort's locks, for example).
//!
//! Page entity ids are handed out per session (client and server share them). Mods that
//! keep something across sessions keep positions instead: the n-th page the chest
//! screen's backpack panel draws and the slot within it. The panel's page order is sent to
//! the mods as `layout`, so they can map positions to this session's slot ids.

use std::collections::HashMap;
use std::sync::Mutex;

use chestsort_common::log;
use serde_json::{json, Value};
use shroudlink::ui::{FromClient, Mark, MarkStyle, SlotKey};

const STORE_FILE: &str = "shroudlink_store.json";
/// Locks from the addon before the building blocks: positions, handed to chestsort once.
const LEGACY_LOCKS: &str = "shroudlink_locks.json";

/// (page position in the backpack panel, slot 0..8)
pub type Position = (u32, u32);

#[derive(Default)]
struct State {
    /// pages in the order the panel draws them this frame, and the last complete order
    drawing: Vec<u32>,
    order: Vec<u32>,
    /// the order the mods last got
    sent_layout: Option<Vec<u32>>,
    keys: HashMap<String, Vec<SlotKey>>,
    marks: HashMap<String, Vec<Mark>>,
    /// this server's id from the welcome (storage key)
    server: String,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(STATE.lock().unwrap().get_or_insert_with(State::default))
}

/// `keen::InventorySlotId` from its 8 in-memory bytes: entity id, slot index, padding.
pub fn slot_id(raw: u64) -> u64 {
    (raw & 0xffff_ffff) | (raw >> 32 & 0xff) << 32
}

/// A backpack slot the chest screen's backpack panel draws now; returns its position.
pub fn observe(slot: u64) -> Position {
    let (page, index) = (slot as u32, (slot >> 32) as u32);
    with(|s| {
        let ordinal = s.drawing.iter().position(|&p| p == page).unwrap_or_else(|| {
            s.drawing.push(page);
            s.drawing.len() - 1
        });
        (ordinal as u32, index)
    })
}

/// Position of a slot drawn anywhere else (inventory view, ...), once the chest screen's
/// backpack panel has shown where its page sits.
pub fn position_of(slot: u64) -> Option<Position> {
    let (page, index) = (slot as u32, (slot >> 32) as u32);
    with(|s| s.order.iter().position(|&p| p == page).map(|i| (i as u32, index)))
}

/// How mods want a slot drawn (a rim wins over dimming).
pub fn mark_of(slot: u64) -> Option<MarkStyle> {
    with(|s| {
        let mut style = None;
        for mark in s.marks.values().flatten().filter(|m| m.slots.contains(&slot)) {
            if style != Some(MarkStyle::Rim) {
                style = Some(mark.style);
            }
        }
        style
    })
}

/// Letters mods registered as slot keys, as a bit mask (bit 0 = A).
fn key_mask(s: &State) -> u32 {
    s.keys.values().flatten().filter_map(|k| k.key.chars().next()).filter(char::is_ascii_alphabetic).fold(0, |m, c| m | 1 << (c.to_ascii_uppercase() as u8 - b'A'))
}

/// A registered key was pressed over a backpack slot: tell the mods that registered it.
pub fn key_pressed(key: char, slot: u64, position: Position, item: u32) {
    let owners: Vec<String> = with(|s| s.keys.iter().filter(|(_, keys)| keys.iter().any(|k| k.key.eq_ignore_ascii_case(&key.to_string()))).map(|(o, _)| o.clone()).collect());
    for owner in owners {
        log!("slots: {key} on slot {} of page {} for {owner}", position.1, position.0);
        let event = FromClient::SlotKey { key: key.to_ascii_uppercase().to_string(), slot, position, item };
        crate::link::send(event.to_message(&owner));
    }
}

/// After the backpack panel was drawn: remembers where the pages sit and tells the mods
/// that use slots when that changed.
pub fn panel_done() {
    let changed = with(|s| {
        let order = std::mem::take(&mut s.drawing);
        if order.is_empty() {
            return None;
        }
        s.order = order;
        (s.sent_layout.as_ref() != Some(&s.order)).then(|| {
            s.sent_layout = Some(s.order.clone());
            (s.order.clone(), owners(s))
        })
    });
    if let Some((pages, owners)) = changed {
        // the core learns which player this addon belongs to from the backpack's pages,
        // and the player's name for mods that show players
        crate::link::send(json!({ "mod": "shroudlink", "op": "identify", "pages": pages, "name": crate::steam::persona_name() }));
        for owner in owners {
            crate::link::send(FromClient::Layout { pages: pages.clone() }.to_message(&owner));
        }
    }
}

/// Mods that use slot keys or marks.
fn owners(s: &State) -> Vec<String> {
    let mut owners: Vec<String> = s.keys.keys().chain(s.marks.keys()).cloned().collect();
    owners.sort();
    owners.dedup();
    owners
}

pub fn set_keys(owner: String, keys: Vec<SlotKey>) {
    let (mask, layout) = with(|s| {
        s.keys.insert(owner.clone(), keys);
        (key_mask(s), (!s.order.is_empty()).then(|| s.order.clone()))
    });
    crate::input::arm_slot_keys(mask);
    // a mod that just started using slots needs the layout already known
    if let Some(pages) = layout {
        crate::link::send(FromClient::Layout { pages }.to_message(&owner));
    }
}

pub fn set_marks(owner: String, marks: Vec<Mark>) {
    with(|s| s.marks.insert(owner, marks));
}

fn load_store() -> Value {
    std::fs::read_to_string(chestsort_common::game_dir().join(STORE_FILE)).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_else(|| json!({}))
}

pub fn store(owner: &str, data: Value) {
    let server = with(|s| s.server.clone());
    let mut all = load_store();
    all[&server][owner] = data;
    let _ = std::fs::write(chestsort_common::game_dir().join(STORE_FILE), serde_json::to_string_pretty(&all).unwrap_or_default());
}

/// A new session: page ids are new, mods register their keys and marks again, and each
/// mod gets what it stored for this server.
pub fn on_welcome(server: &str, mods: &[Value]) {
    with(|s| {
        *s = State { server: server.to_owned(), ..State::default() };
    });
    crate::input::arm_slot_keys(0);
    let all = load_store();
    for id in mods.iter().filter_map(|m| m.get("id")?.as_str()) {
        let mut data = all.get(server).and_then(|s| s.get(id)).cloned();
        if data.is_none() && id == "chestsort" {
            data = legacy_locks();
        }
        if let Some(data) = data {
            crate::link::send(FromClient::Stored { data }.to_message(id));
        }
    }
}

/// Locks saved by the addon before the building blocks (positions, same meaning).
fn legacy_locks() -> Option<Value> {
    let text = std::fs::read_to_string(chestsort_common::game_dir().join(LEGACY_LOCKS)).ok()?;
    let positions: Vec<Position> = serde_json::from_str(&text).ok()?;
    Some(json!({ "locked": positions }))
}
