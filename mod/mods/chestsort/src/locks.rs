//! Slot locks: backpack slots a player locked with L never leave in any transfer.
//!
//! Built from the addon's generic building blocks: the `L` slot key toggles a lock, the
//! locks are kept on the player's PC (`store`/`stored`) as positions (page in display
//! order, slot), the backpack `layout` maps positions to this session's slot ids
//! (`page entity | slot << 32`, shared by client and server), and `marks` draws the rims.
//! Addons from before the building blocks send their slot ids directly (`"op":"locks"`).

use std::collections::{BTreeSet, HashMap, HashSet};

use shroudlink_sdk::ui::{FromClient, Mark, MarkStyle, SlotKey, ToClient};
use shroudlink_sdk::{json, log, Host, Value};

use crate::ID;

pub const KEY: &str = "L";

#[derive(Default)]
struct Client {
    positions: BTreeSet<(u32, u32)>,
    /// backpack page entities in display order (this session)
    layout: Vec<u32>,
    /// slot ids from an addon without building blocks
    legacy: HashSet<u64>,
}

impl Client {
    fn slots(&self) -> Vec<u64> {
        let mapped = self.positions.iter().filter_map(|&(page, index)| self.layout.get(page as usize).map(|&e| e as u64 | (index as u64) << 32));
        let mut slots: Vec<u64> = mapped.chain(self.legacy.iter().copied()).collect();
        slots.sort_unstable();
        slots.dedup();
        slots
    }
}

#[derive(Default)]
pub struct Locks {
    clients: HashMap<u64, Client>,
}

impl Locks {
    /// A client joined: offer the lock key.
    pub fn on_join(&mut self, host: &mut Host, client: u64) {
        self.clients.insert(client, Client::default());
        let keys = ToClient::SlotKeys { owner: ID.into(), keys: vec![SlotKey { key: KEY.into(), label: "Lock".into() }] };
        host.send(client, &keys.to_message());
    }

    /// Handles the lock messages; false for messages that are not about locks.
    pub fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) -> bool {
        let state = self.clients.entry(client).or_default();
        if message.get("op").and_then(Value::as_str) == Some("locks") {
            state.legacy = message.get("slots").and_then(Value::as_array).into_iter().flatten().filter_map(Value::as_u64).collect();
            log!("chestsort: client {client:016x} (older addon) locked {} slot(s)", state.legacy.len());
            return true;
        }
        match FromClient::from_message(message) {
            Some(FromClient::Stored { data }) => {
                state.positions = data.get("locked").and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
            }
            Some(FromClient::Layout { pages }) => state.layout = pages,
            Some(FromClient::SlotKey { key, position, .. }) if key == KEY => {
                if !state.positions.remove(&position) {
                    state.positions.insert(position);
                }
                let store = ToClient::Store { owner: ID.into(), data: json!({ "locked": state.positions }) };
                host.send(client, &store.to_message());
            }
            _ => return false,
        }
        let slots = state.slots();
        log!("chestsort: client {client:016x} has {} locked slot(s)", slots.len());
        let marks = ToClient::Marks { owner: ID.into(), marks: vec![Mark { slots, style: MarkStyle::Rim }] };
        host.send(client, &marks.to_message());
        true
    }

    pub fn forget(&mut self, client: u64) {
        self.clients.remove(&client);
    }

    pub fn any(&self) -> bool {
        self.clients.values().any(|c| !c.slots().is_empty())
    }

    /// Locked slots on these pages (entity ids).
    pub fn locked_in(&self, pages: &HashSet<u32>) -> Vec<u64> {
        self.clients.values().flat_map(Client::slots).filter(|&slot| pages.contains(&(slot as u32))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shroudlink_sdk::testing::MockHost;

    fn sent_ops(host: &mut MockHost) -> Vec<Value> {
        host.take_sent().into_iter().map(|(_, v)| v).collect()
    }

    #[test]
    fn lock_key_toggles_stores_and_marks() {
        let mut locks = Locks::default();
        let mut host = MockHost::default();
        locks.on_join(&mut host.host(), 7);
        assert_eq!(sent_ops(&mut host)[0]["op"], "slot_keys");

        // the layout arrives, then L on page 1 slot 6
        locks.on_message(&mut host.host(), 7, &FromClient::Layout { pages: vec![6660, 6672] }.to_message(ID));
        let key = FromClient::SlotKey { key: KEY.into(), slot: 6672 | 6 << 32, position: (1, 6), item: 42 };
        locks.on_message(&mut host.host(), 7, &key.to_message(ID));
        let sent = sent_ops(&mut host);
        let store = sent.iter().find(|m| m["op"] == "store").expect("stored on the PC");
        assert_eq!(store["data"]["locked"], json!([[1, 6]]));
        let marks = sent.iter().rev().find(|m| m["op"] == "marks").expect("marked");
        assert_eq!(marks["marks"][0]["slots"], json!([6672u64 | 6 << 32]));
        assert_eq!(locks.locked_in(&[6672].into()), vec![6672u64 | 6 << 32]);

        // next session: new page ids, the stored position maps to the new slot
        locks.forget(7);
        locks.on_join(&mut host.host(), 8);
        locks.on_message(&mut host.host(), 8, &FromClient::Stored { data: store["data"].clone() }.to_message(ID));
        locks.on_message(&mut host.host(), 8, &FromClient::Layout { pages: vec![7000, 7001] }.to_message(ID));
        assert_eq!(locks.locked_in(&[7001].into()), vec![7001u64 | 6 << 32]);

        // L again unlocks
        locks.on_message(&mut host.host(), 8, &FromClient::SlotKey { key: KEY.into(), slot: 7001 | 6 << 32, position: (1, 6), item: 42 }.to_message(ID));
        assert!(!locks.any());
    }

    #[test]
    fn older_addons_send_slot_ids() {
        let mut locks = Locks::default();
        let mut host = MockHost::default();
        locks.on_message(&mut host.host(), 9, &json!({ "mod": ID, "op": "locks", "slots": [6672u64 | 6 << 32, 9999u64 | 1 << 32] }));
        assert_eq!(locks.locked_in(&[6671, 6672].into()), vec![6672u64 | 6 << 32]);
        locks.forget(9);
        assert!(!locks.any());
    }
}
