//! Locked inventory slots: the server's deposits never take items from them. Toggled
//! with L while hovering a backpack slot on the chest screen.
//!
//! Page entity ids are handed out per session (client and server share them), so a lock
//! is stored by its position in the backpack panel: the n-th page drawn there and the
//! slot within it. `shroudlink_locks.json` next to the game keeps them. Whenever the panel
//! is drawn, positions are translated to this session's slot ids for the server.

use std::collections::BTreeSet;
use std::sync::Mutex;

use chestsort_common::log;
use serde_json::json;

const FILE: &str = "shroudlink_locks.json";

/// (page position in the backpack panel, slot 0..8)
type Position = (u32, u32);

#[derive(Default)]
struct State {
    /// None until loaded from the file
    locked: Option<BTreeSet<Position>>,
    /// pages in the order the panel draws them this frame, and the last complete order
    drawing: Vec<u32>,
    order: Vec<u32>,
    /// what the server last got
    sent: Option<Vec<u64>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    let mut guard = STATE.lock().unwrap();
    let state = guard.get_or_insert_with(State::default);
    if state.locked.is_none() {
        // older files held session entity ids, which mean nothing now: start empty
        let loaded = std::fs::read_to_string(chestsort_common::game_dir().join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str::<Vec<Position>>(&t).ok())
            .unwrap_or_default();
        state.locked = Some(loaded.into_iter().collect());
    }
    f(state)
}

/// `keen::InventorySlotId` from its 8 in-memory bytes: entity id, slot index, padding.
pub fn slot_id(raw: u64) -> u64 {
    (raw & 0xffff_ffff) | (raw >> 32 & 0xff) << 32
}

/// A backpack slot the game draws now; returns its position.
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

/// Position of a slot drawn anywhere else (inventory view, ...), once the chest
/// screen's backpack panel has shown where its page sits.
pub fn position_of(slot: u64) -> Option<Position> {
    let (page, index) = (slot as u32, (slot >> 32) as u32);
    with(|s| s.order.iter().position(|&p| p == page).map(|i| (i as u32, index)))
}

pub fn is_locked(position: Position) -> bool {
    with(|s| s.locked.as_ref().unwrap().contains(&position))
}

pub fn toggle(position: Position) {
    let locked = with(|s| {
        let set = s.locked.as_mut().unwrap();
        let locked = !set.remove(&position);
        if locked {
            set.insert(position);
        }
        let list: Vec<Position> = set.iter().copied().collect();
        let _ = std::fs::write(chestsort_common::game_dir().join(FILE), serde_json::to_string(&list).unwrap_or_default());
        locked
    });
    log!("locks: page {} slot {} {}", position.0, position.1, if locked { "locked" } else { "unlocked" });
    sync();
}

/// A new session: page ids are new, and the server has to be told everything again.
pub fn send() {
    with(|s| {
        s.sent = None;
        s.order.clear();
    });
}

/// After the backpack panel was drawn: remembers where the pages sit and syncs.
pub fn panel_done() {
    let order = with(|s| std::mem::take(&mut s.drawing));
    if !order.is_empty() {
        with(|s| s.order = order);
        sync();
    }
}

/// Sends this session's locked slot ids to the server mod (chestsort) when they changed.
fn sync() {
    let slots = with(|s| {
        let slots: Vec<u64> = s
            .locked
            .as_ref()
            .unwrap()
            .iter()
            .filter_map(|&(page, slot)| s.order.get(page as usize).map(|&e| e as u64 | (slot as u64) << 32))
            .collect();
        if s.order.is_empty() || s.sent.as_ref() == Some(&slots) {
            return None;
        }
        s.sent = Some(slots.clone());
        Some(slots)
    });
    if let Some(slots) = slots {
        crate::link::send(json!({ "mod": "chestsort", "op": "locks", "slots": slots }));
    }
}
