//! Inventory slots players locked with the client addon: transfers never take from them.
//! Client and server share entity ids, so a lock is the slot id itself
//! (`page entity | slot << 32`); the addon sends its full list for this session.

use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct Locks {
    /// client session -> its locked slots
    by_client: HashMap<u64, HashSet<u64>>,
}

impl Locks {
    pub fn set(&mut self, client: u64, slots: HashSet<u64>) {
        self.by_client.insert(client, slots);
    }

    pub fn forget(&mut self, client: u64) {
        self.by_client.remove(&client);
    }

    pub fn any(&self) -> bool {
        self.by_client.values().any(|s| !s.is_empty())
    }

    /// Locked slots on these pages (entity ids).
    pub fn locked_in(&self, pages: &HashSet<u32>) -> Vec<u64> {
        self.by_client.values().flatten().copied().filter(|&slot| pages.contains(&(slot as u32))).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locks_apply_to_own_pages_only() {
        let mut locks = Locks::default();
        locks.set(7, [6672u64 | 6 << 32, 9999 | 1 << 32].into());
        assert_eq!(locks.locked_in(&[6671, 6672].into()), vec![6672u64 | 6 << 32]);
        locks.forget(7);
        assert!(!locks.any());
    }
}
