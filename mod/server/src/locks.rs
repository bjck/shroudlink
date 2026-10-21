//! Inventory slots players locked with the client addon: deposits never take from them.
//! Client and server share entity ids, so a lock is the slot id itself
//! (`page entity | slot << 32`); the addon sends its full list for this session.

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;

use crate::mods::ClientId;

static LOCKS: Mutex<Option<HashMap<ClientId, HashSet<u64>>>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut HashMap<ClientId, HashSet<u64>>) -> R) -> R {
    f(LOCKS.lock().unwrap().get_or_insert_with(HashMap::new))
}

pub fn set(client: ClientId, slots: HashSet<u64>) {
    with(|map| map.insert(client, slots));
}

pub fn any() -> bool {
    with(|map| map.values().any(|s| !s.is_empty()))
}

pub fn forget(client: ClientId) {
    with(|map| map.remove(&client));
}

/// Locked slots on these pages (entity ids).
pub fn locked_in(pages: &[u32]) -> HashSet<u64> {
    with(|map| map.values().flatten().copied().filter(|&slot| pages.contains(&(slot as u32))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_apply_to_own_pages_only() {
        let client = ClientId(7);
        set(client, [6672u64 | 6 << 32, 9999 | 1 << 32].into());
        assert_eq!(locked_in(&[6671, 6672]), [6672u64 | 6 << 32].into());
        forget(client);
        assert!(!any());
    }
}
