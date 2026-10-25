//! Gemforge: the Gemforge players build in their base takes its upgrade material from the
//! base's chests. The forge's gem actions (`ecs::ShroudForge`: slot, unslot, upgrade,
//! recycle) pay with items from a slot of the player's own inventory, unlike crafting, so
//! a player standing at a forge gets the material from the chests around it lent into
//! the backpack; walking away gives back what is left of it. Built from the SDK:
//! `entities_near`, `inventories_near`, `inventory`, `move_items` and `on_tick`.

use std::collections::HashMap;

use shroudlink_sdk::{export_mod, log, Host, Mod, Slot, Value, World};

const ID: &str = "gemforge";
/// `ShroudForge.upgradeMaterial` of every forge: `Material_T7_Fognest_Ressource`
/// (`research/scripts/eml/gemforge_dump`).
const MATERIAL: u32 = 2135602472;
/// A player this close to a forge is using it (its interaction range is a few metres).
const REACH: f64 = 4.0;
/// Farther away than this the player has left it.
const LEAVE: f64 = 10.0;
/// Chests around the forge the material comes from (the assistants' work radius).
const CHESTS: f64 = 40.0;
/// Ticks (about a second each) between looks for forges: `entities_near` scans every
/// loaded entity.
const LOOK_EVERY: u32 = 2;

/// What a player was lent at a forge.
#[derive(Clone, Debug, PartialEq)]
struct Loan {
    forge: u32,
    /// material taken from the chests
    lent: u32,
    /// material the player carried before: theirs to keep
    own: u32,
    /// the chests it came from, which take it back
    chests: Vec<u32>,
}

/// The world as the lending sees it (a fake in the tests).
trait Place {
    fn players(&self) -> Vec<u32>;
    fn forge_near(&self, player: u32, radius: f64) -> Option<u32>;
    fn within(&self, player: u32, entity: u32, radius: f64) -> bool;
    /// Inventories around the forge that are not players'.
    fn chests(&self, forge: u32) -> Vec<u32>;
    fn slots(&self, entity: u32) -> Vec<Slot>;
    fn move_items(&self, from: u32, slot: u64, to: u32, count: u32) -> u32;
}

impl Place for World<'_> {
    fn players(&self) -> Vec<u32> {
        World::players(self)
    }

    fn forge_near(&self, player: u32, radius: f64) -> Option<u32> {
        self.entities_near(player, radius, "ShroudForge").into_iter().next()
    }

    fn within(&self, player: u32, entity: u32, radius: f64) -> bool {
        match (self.position(player), self.position(entity)) {
            (Some(a), Some(b)) => (a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2) + (a[2] - b[2]).powi(2) <= radius * radius,
            _ => false,
        }
    }

    fn chests(&self, forge: u32) -> Vec<u32> {
        let players = World::players(self);
        self.inventories_near(forge, CHESTS).into_iter().filter(|e| *e != forge && !players.contains(e)).collect()
    }

    fn slots(&self, entity: u32) -> Vec<Slot> {
        self.inventory(entity)
    }

    fn move_items(&self, from: u32, slot: u64, to: u32, count: u32) -> u32 {
        World::move_items(self, from, slot, to, count)
    }
}

fn material_in(place: &dyn Place, entity: u32) -> u32 {
    place.slots(entity).iter().filter(|s| s.item == MATERIAL).map(|s| s.count).sum()
}

#[derive(Default)]
pub struct Gemforge {
    loans: HashMap<u32, Loan>,
    ticks: u32,
}

impl Gemforge {
    fn step(&mut self, place: &dyn Place) {
        let players = place.players();
        // a player who left the game keeps what they carry
        self.loans.retain(|player, loan| {
            let here = players.contains(player);
            if !here {
                log!("gemforge: player {player} left with {} lent material", loan.lent);
            }
            here
        });
        for player in players {
            match self.loans.get(&player) {
                Some(loan) if !place.within(player, loan.forge, LEAVE) => {
                    let loan = self.loans.remove(&player).unwrap();
                    give_back(place, player, &loan);
                }
                Some(_) => {}
                None => {
                    if let Some(forge) = place.forge_near(player, REACH) {
                        self.loans.insert(player, lend(place, player, forge));
                    }
                }
            }
        }
    }
}

/// Moves the material of the chests around the forge into the player's inventory.
fn lend(place: &dyn Place, player: u32, forge: u32) -> Loan {
    let mut loan = Loan { forge, lent: 0, own: material_in(place, player), chests: Vec::new() };
    for chest in place.chests(forge) {
        for slot in place.slots(chest).into_iter().filter(|s| s.item == MATERIAL && s.count != 0) {
            let moved = place.move_items(chest, slot.id, player, slot.count);
            if moved > 0 {
                loan.lent += moved;
                if !loan.chests.contains(&chest) {
                    loan.chests.push(chest);
                }
            }
        }
    }
    if loan.lent > 0 {
        log!("gemforge: player {player} at forge {forge}: {} material lent from {} chest(s)", loan.lent, loan.chests.len());
    }
    loan
}

/// Puts what is left of the lent material back into the chests it came from; what the
/// player carried before stays with them (the forge used the lent material first).
fn give_back(place: &dyn Place, player: u32, loan: &Loan) {
    let mut left = loan.lent.min(material_in(place, player).saturating_sub(loan.own));
    let back = left;
    for &chest in &loan.chests {
        for slot in place.slots(player).into_iter().filter(|s| s.item == MATERIAL && s.count != 0) {
            if left == 0 {
                break;
            }
            left -= place.move_items(player, slot.id, chest, left.min(slot.count));
        }
    }
    if loan.lent > 0 {
        log!("gemforge: player {player} left forge {}: {} of {} lent material back, {} used{}", loan.forge, back - left, loan.lent, loan.lent - back, if left > 0 { ", the chests are full: the rest stays with the player" } else { "" });
    }
}

impl Mod for Gemforge {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_message(&mut self, _host: &mut Host, _client: u64, _message: &Value) {}

    fn on_tick(&mut self, host: &mut Host) {
        self.ticks += 1;
        if self.ticks % LOOK_EVERY == 0 {
            self.step(&host.world());
        }
    }
}

export_mod!(Gemforge);

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    const PLAYER: u32 = 1;
    const FORGE: u32 = 50;
    const CHEST: u32 = 60;
    const OTHER: u32 = 61;
    const GEM: u32 = 7;

    struct Fake {
        inventories: RefCell<HashMap<u32, Vec<(u32, u32)>>>,
        near: RefCell<bool>,
        here: RefCell<bool>,
        capacity: u32,
    }

    impl Fake {
        fn new(chests: &[(u32, &[(u32, u32)])], player: &[(u32, u32)]) -> Fake {
            let mut inventories: HashMap<u32, Vec<(u32, u32)>> = chests.iter().map(|(c, s)| (*c, s.to_vec())).collect();
            inventories.insert(PLAYER, player.to_vec());
            Fake { inventories: RefCell::new(inventories), near: RefCell::new(true), here: RefCell::new(true), capacity: u32::MAX }
        }

        fn count(&self, entity: u32, item: u32) -> u32 {
            self.inventories.borrow()[&entity].iter().filter(|s| s.0 == item).map(|s| s.1).sum()
        }
    }

    impl Place for Fake {
        fn players(&self) -> Vec<u32> {
            if *self.here.borrow() { vec![PLAYER] } else { Vec::new() }
        }
        fn forge_near(&self, _: u32, _: f64) -> Option<u32> {
            self.near.borrow().then_some(FORGE)
        }
        fn within(&self, _: u32, _: u32, _: f64) -> bool {
            *self.near.borrow()
        }
        fn chests(&self, _: u32) -> Vec<u32> {
            let mut c: Vec<u32> = self.inventories.borrow().keys().copied().filter(|&e| e != PLAYER).collect();
            c.sort();
            c
        }
        fn slots(&self, entity: u32) -> Vec<Slot> {
            self.inventories.borrow().get(&entity).map_or(Vec::new(), |s| s.iter().enumerate().map(|(i, &(item, count))| Slot { id: entity as u64 | (i as u64) << 32, item, count }).collect())
        }
        fn move_items(&self, from: u32, slot: u64, to: u32, count: u32) -> u32 {
            let mut inv = self.inventories.borrow_mut();
            let (item, have) = inv[&from][(slot >> 32) as usize];
            let room = if to == PLAYER { u32::MAX } else { self.capacity.saturating_sub(inv[&to].iter().map(|s| s.1).sum()) };
            let n = count.min(have).min(room);
            inv.get_mut(&from).unwrap()[(slot >> 32) as usize].1 -= n;
            inv.get_mut(&to).unwrap().push((item, n));
            n
        }
    }

    #[test]
    fn at_the_forge_the_chests_material_is_lent_and_the_rest_comes_back() {
        let place = Fake::new(&[(CHEST, &[(MATERIAL, 30), (GEM, 2)]), (OTHER, &[(MATERIAL, 20)])], &[(MATERIAL, 5)]);
        let mut m = Gemforge::default();
        m.step(&place);
        assert_eq!((place.count(PLAYER, MATERIAL), place.count(CHEST, MATERIAL), place.count(CHEST, GEM)), (55, 0, 2));
        m.step(&place);
        assert_eq!(place.count(PLAYER, MATERIAL), 55, "lent once per visit");
        // the forge uses 12, then the player walks away
        take(&place, 12);
        *place.near.borrow_mut() = false;
        m.step(&place);
        assert_eq!(place.count(PLAYER, MATERIAL), 5, "their own stays");
        assert_eq!(place.count(CHEST, MATERIAL) + place.count(OTHER, MATERIAL), 38);
        assert!(m.loans.is_empty());
    }

    /// The forge takes `n` material from the player.
    fn take(place: &Fake, mut n: u32) {
        let mut inv = place.inventories.borrow_mut();
        for s in inv.get_mut(&PLAYER).unwrap().iter_mut().filter(|s| s.0 == MATERIAL) {
            let t = n.min(s.1);
            s.1 -= t;
            n -= t;
        }
    }

    #[test]
    fn the_lent_material_is_used_first() {
        let place = Fake::new(&[(CHEST, &[(MATERIAL, 10)])], &[(MATERIAL, 5)]);
        let mut m = Gemforge::default();
        m.step(&place);
        take(&place, 13);
        *place.near.borrow_mut() = false;
        m.step(&place);
        // 15 carried, 13 used: the 10 lent and 3 of their own; the 2 left are theirs
        assert_eq!((place.count(PLAYER, MATERIAL), place.count(CHEST, MATERIAL)), (2, 0));
    }

    #[test]
    fn full_chests_leave_the_rest_with_the_player_and_leaving_the_game_keeps_it() {
        let mut place = Fake::new(&[(CHEST, &[(MATERIAL, 10)])], &[]);
        let mut m = Gemforge::default();
        m.step(&place);
        place.capacity = 4;
        *place.near.borrow_mut() = false;
        m.step(&place);
        assert_eq!((place.count(PLAYER, MATERIAL), place.count(CHEST, MATERIAL)), (6, 4));
        *place.near.borrow_mut() = true;
        m.step(&place);
        assert!(m.loans.contains_key(&PLAYER));
        *place.here.borrow_mut() = false;
        m.step(&place);
        assert!(m.loans.is_empty());
    }

    #[test]
    fn the_component_is_known() {
        assert!(shroudlink_sdk::components::hash_of("ShroudForge").is_some());
    }
}
