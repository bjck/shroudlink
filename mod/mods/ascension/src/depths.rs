//! Torment (called depth in the code and the file): from Torment 1 hostile enemies around
//! a player are at least `min_level` (50), and each Torment level above 1 adds
//! `levels_per_depth` more; Torment 0 is the game's own. The game does the rest: `enemy_scaling` rescales an enemy's health when its
//! Level changes (keeping the health percentage), its damage and the XP it gives follow
//! Level, and the client shows the new level (tested: a wolf at 150 nearly one-shot a
//! level 50 player). An enemy is raised once, never lowered while it lives; where players
//! at different depths meet, the deeper one wins.

use std::collections::HashMap;

use shroudlink_sdk::{attr, World};

/// `keen::FactionType` values that fight players: Fogger, Vukah, Scavenger, WildBeast,
/// Skeleton, Cyclops, Drak, Ancients. Not Wildlife (deer, …), FarmAnimal, PlayerAlly
/// (survivors), Player or Environment.
const HOSTILE: [u8; 8] = [2, 3, 4, 6, 10, 13, 14, 15];

/// An enemy this mod raised: its own level and maximum, and what it was raised to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Raised {
    pub base: f64,
    pub base_max: f64,
    pub level: f64,
}

#[derive(Default)]
pub struct Depths {
    pub raised: HashMap<u32, Raised>,
}

/// The level an enemy of level `base` gets at Torment `depth`.
pub fn target(base: f64, depth: u32, levels_per_depth: u32, min_level: f64) -> f64 {
    if depth == 0 {
        return base;
    }
    base.max(min_level) + ((depth - 1) * levels_per_depth) as f64
}

/// The loot boost at Torment `depth` (`World::set_loot_boost`): items at least `min_level`
/// plus the same levels enemies get, and per Torment level 5 % fewer common, 5 % more rare,
/// 8 % more epic and 15 % more legendary rolls. Torment 0: the game's own loot.
pub fn loot(depth: u32, min_level: f64, levels_per_depth: u32) -> (f64, f64, [f64; 5]) {
    if depth == 0 {
        return (0.0, 0.0, [1.0; 5]);
    }
    let t = depth as f64;
    (min_level, ((depth - 1) * levels_per_depth) as f64, [1.0 / (1.0 + 0.05 * t), 1.0, 1.0 + 0.05 * t, 1.0 + 0.08 * t, 1.0 + 0.15 * t])
}

fn hostile(world: &World, entity: u32) -> bool {
    world.component(entity, "Faction").and_then(|f| f.first().copied()).is_some_and(|f| HOSTILE.contains(&f))
}

impl Depths {
    /// Raises the hostile enemies within `radius` of `player` to depth `depth`; returns how
    /// many were raised now.
    pub fn sweep(&mut self, world: &World, player: u32, depth: u32, levels_per_depth: u32, min_level: f64, radius: f64) -> u32 {
        let mut count = 0;
        for enemy in world.entities_near(player, radius, "EnemyScaling") {
            let Some(level) = world.attribute(enemy, attr::LEVEL) else { continue };
            if world.attribute(enemy, attr::HEALTH).is_none_or(|h| h <= 0.0) || !hostile(world, enemy) {
                continue;
            }
            // an id seen before but at another level is a new enemy with a reused id
            let known = self.raised.get(&enemy).copied().filter(|r| r.level == level);
            let (base, base_max) = match known {
                Some(r) => (r.base, r.base_max),
                None => (level, world.attribute(enemy, attr::LEVEL_MAX).unwrap_or(level)),
            };
            let want = target(base, depth, levels_per_depth, min_level);
            if want <= level {
                continue;
            }
            if want > world.attribute(enemy, attr::LEVEL_MAX).unwrap_or(0.0) && !world.set_attribute(enemy, attr::LEVEL_MAX, want) {
                continue;
            }
            if world.set_attribute(enemy, attr::LEVEL, want) {
                self.raised.insert(enemy, Raised { base, base_max, level: want });
                count += 1;
            }
        }
        count
    }

    /// Forgets enemies that died, unloaded or whose id now belongs to another entity.
    pub fn forget_gone(&mut self, world: &World) {
        self.raised.retain(|&e, r| world.attribute(e, attr::LEVEL) == Some(r.level) && world.attribute(e, attr::HEALTH).is_some_and(|h| h > 0.0));
    }

    /// Puts every raised enemy that still lives back to its own level (depth 0 for all,
    /// or the mod is being switched off).
    pub fn lower_all(&mut self, world: &World) -> u32 {
        let mut count = 0;
        for (e, r) in std::mem::take(&mut self.raised) {
            if world.attribute(e, attr::LEVEL) == Some(r.level) && world.set_attribute(e, attr::LEVEL, r.base) {
                world.set_attribute(e, attr::LEVEL_MAX, r.base_max);
                count += 1;
            }
        }
        count
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loot_gets_better_with_torment() {
        assert_eq!(loot(0, 50.0, 3), (0.0, 0.0, [1.0; 5]));
        let (min, add, rarity) = loot(3, 50.0, 3);
        assert_eq!((min, add), (50.0, 6.0));
        assert!(rarity[0] < 1.0 && rarity[4] > rarity[3] && rarity[3] > rarity[2] && rarity[2] > 1.0);
    }

    #[test]
    fn torment_lifts_to_the_floor_then_adds() {
        assert_eq!(target(1.0, 0, 3, 50.0), 1.0, "Torment 0 is vanilla");
        assert_eq!(target(1.0, 1, 3, 50.0), 50.0);
        assert_eq!(target(1.0, 4, 3, 50.0), 59.0);
        assert_eq!(target(105.0, 1, 3, 50.0), 105.0, "stronger enemies keep their level");
        assert_eq!(target(105.0, 2, 3, 50.0), 108.0);
    }
}
