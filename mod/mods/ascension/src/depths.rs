//! Shroud Depths: hostile enemies around a player at depth `d` get `levels_per_depth * d`
//! levels. The game does the rest: `enemy_scaling` rescales an enemy's health when its
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

/// The level an enemy of level `base` gets at depth `depth`.
pub fn target(base: f64, depth: u32, levels_per_depth: u32) -> f64 {
    base + (depth * levels_per_depth) as f64
}

fn hostile(world: &World, entity: u32) -> bool {
    world.component(entity, "Faction").and_then(|f| f.first().copied()).is_some_and(|f| HOSTILE.contains(&f))
}

impl Depths {
    /// Raises the hostile enemies within `radius` of `player` to depth `depth`; returns how
    /// many were raised now.
    pub fn sweep(&mut self, world: &World, player: u32, depth: u32, levels_per_depth: u32, radius: f64) -> u32 {
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
            let want = target(base, depth, levels_per_depth);
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
    fn depth_adds_levels() {
        assert_eq!(target(45.0, 0, 3), 45.0);
        assert_eq!(target(45.0, 20, 3), 105.0);
    }
}
