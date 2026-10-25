//! The assistant at work: one step per order, every few seconds while its base is loaded.
//! A step empties the order's stations into storage, counts the results, fills the
//! stations from storage for the runs still needed (a few at a time), and sets their next
//! recipe. The game runs the stations; when the order is complete the stations get their
//! recipe cleared and the unused material goes back into storage.

use shroudlink_sdk::log;

use crate::base::{missing, state, Base, Station};
use crate::orders::{Order, Status};
use crate::planner::{self, Counts};
use crate::recipes::{Input, Recipe, Table};

/// How far around the assistant its stations and chests may stand, in metres.
pub const RADIUS: f64 = 40.0;
/// Runs of material a station is given at a time.
pub const BATCH: u32 = 5;
/// Tank units per water charge: tanks hold 64 per charge (the Oven's 100 charges show as
/// 6400 in its `Fuel` attribute, seen in game).
pub const WATER_UNITS: u32 = 64;

fn counts_of(base: &dyn Base, entities: &[u32]) -> Counts {
    let mut counts = Counts::new();
    for &e in entities {
        for s in base.slots(e).into_iter().filter(|s| s.count != 0) {
            *counts.entry(s.item).or_default() += s.count;
        }
    }
    counts
}

/// Every item a recipe may take (category options included).
fn input_items(table: &Table, recipe: &Recipe) -> Vec<u32> {
    let mut items = Vec::new();
    for input in &recipe.inputs {
        match *input {
            Input::Item { item, .. } => items.push(item),
            Input::Category { id, .. } => items.extend(table.categories.get(&id).into_iter().flatten().map(|o| o.0)),
        }
    }
    items
}

/// Moves a stack into the chests, those already holding the item first. Returns the
/// number of items moved.
pub(crate) fn deposit(base: &dyn Base, from: u32, slot: u64, item: u32, count: u32, chests: &[u32]) -> u32 {
    let holds = |c: u32| base.slots(c).iter().any(|s| s.item == item && s.count != 0);
    let (mut first, rest): (Vec<u32>, Vec<u32>) = chests.iter().partition(|&&c| holds(c));
    first.extend(rest);
    let mut moved = 0;
    for chest in first {
        moved += base.move_items(from, slot, chest, u32::MAX);
        if moved >= count {
            break;
        }
    }
    moved
}

/// Takes `count` of `item` from the chests into `to`; returns how many it got.
fn take(base: &dyn Base, chests: &[u32], item: u32, count: u32, to: u32) -> u32 {
    let mut got = 0;
    for &chest in chests {
        for s in base.slots(chest).into_iter().filter(|s| s.item == item && s.count != 0) {
            if got >= count {
                return got;
            }
            got += base.move_items(chest, s.id, to, count - got);
        }
    }
    got
}

/// Stations near the assistant this order may use: the recipe's workshop, not taken by an
/// earlier order this round, and idle or already on this recipe.
/// The stations an order may use: its own and free ones, else idle ones set to someone
/// else's recipe (taken over, and given back later). Otherwise why it waits.
fn stations_for(table: &Table, base: &dyn Base, recipe: &Recipe, assistant: u32, claimed: &[u32]) -> Result<Vec<Station>, Status> {
    let all: Vec<Station> = base.stations_near(assistant, RADIUS).into_iter().filter(|s| s.workshop == recipe.workshop && !claimed.contains(&s.entity)).collect();
    let ours: Vec<Station> = all.iter().filter(|s| s.next == recipe.id || s.running == recipe.id || (s.next == 0 && s.state != state::RUNNING)).cloned().collect();
    if !ours.is_empty() {
        return Ok(ours);
    }
    let idle: Vec<Station> = all.iter().filter(|s| s.state != state::RUNNING && s.state != state::PAUSED).cloned().collect();
    if !idle.is_empty() {
        return Ok(idle);
    }
    let Some(busy) = all.first() else { return Err(Status::NoStation) };
    let station = table.station(busy.workshop).map_or_else(|| "the station".to_string(), |s| format!("the {}", crate::names::pretty(&s.name)));
    let other = if busy.running != 0 { busy.running } else { busy.next };
    let what = table.recipe(other).and_then(|r| r.outputs.first()).map(|&(item, _)| crate::names::item(item));
    Err(Status::Busy(match what {
        Some(item) => format!("{station} is making {item}"),
        None => format!("{station} is making something else"),
    }))
}

/// Points the order's borrowed stations at the stations standing where they stood (after
/// a restart the entity ids are new).
fn rebind(base: &dyn Base, order: &mut Order, assistant: u32) {
    if order.borrowed.is_empty() {
        return;
    }
    let stations = base.stations_near(assistant, RADIUS);
    let at = |entity: u32, pos: [f32; 3]| base.position(entity).is_some_and(|p| (0..3).map(|i| (p[i] as f32 - pos[i]).powi(2)).sum::<f32>() < 1.0);
    for b in order.borrowed.iter_mut().filter(|b| b.3 != [0.0; 3]) {
        if stations.iter().any(|s| s.entity == b.0 && at(s.entity, b.3)) {
            continue;
        }
        if let Some(s) = stations.iter().find(|s| at(s.entity, b.3)) {
            log!("assistants: order {}: borrowed station {} is now {}", order.id, b.0, s.entity);
            b.0 = s.entity;
        }
    }
}

/// Gives a station the order no longer needs back: the recipe it was taken over from, or none.
fn give_back(base: &dyn Base, order: &mut Order, station: u32) {
    match order.borrowed.iter().position(|b| b.0 == station) {
        Some(i) => {
            let (_, recipe, selections, _) = order.borrowed.remove(i);
            let ok = base.set_recipe(station, recipe, &selections);
            log!("assistants: order {}: station {station} back to recipe {recipe}: {ok}", order.id);
        }
        None => {
            base.set_recipe(station, 0, &[]);
        }
    }
}

/// Carries water from the Wells around the assistant into a station's tank for its next
/// runs, filling it up while at it, as a player does with a Watering Can: what a Well
/// gives, the station gets. A station without a tank draws from Wells in its own range when
/// it crafts. False when the station still has too little water for one run.
fn water(base: &dyn Base, recipe: &Recipe, station: u32, runs: u32, assistant: u32, order: u32) -> bool {
    if recipe.charges == 0 || runs == 0 {
        return true;
    }
    let Some(tank) = base.tank(station) else { return true };
    let need = (recipe.charges * WATER_UNITS * runs).min(tank.max);
    if tank.amount >= need {
        return true;
    }
    let mut amount = tank.amount;
    for well in base.wells_near(assistant, RADIUS).into_iter().filter(|&w| w != station) {
        let Some(w) = base.tank(well) else { continue };
        let take = w.amount.min(tank.max - amount);
        if take == 0 || !base.set_tank(well, w.amount - take) {
            continue;
        }
        if base.set_tank(station, amount + take) {
            amount += take;
            log!("assistants: order {order}: {take} water from well {well} into station {station} ({amount}/{})", tank.max);
        } else {
            base.set_tank(well, w.amount);
        }
        if amount >= tank.max {
            break;
        }
    }
    amount >= recipe.charges * WATER_UNITS
}

/// Clears the recipe of the order's stations and returns what is left of its material and
/// results to storage. A station in the middle of a run keeps it unless `cancel`.
pub fn release(table: &Table, order: &mut Order, assistant: u32, base: &dyn Base, cancel: bool) {
    let Some(recipe) = table.recipe(order.recipe) else { return };
    rebind(base, order, assistant);
    let chests = base.chests_near(assistant, RADIUS);
    let inputs = input_items(table, recipe);
    for s in base.stations_near(assistant, RADIUS).into_iter().filter(|s| s.workshop == recipe.workshop && (s.next == recipe.id || s.running == recipe.id)) {
        if s.next == recipe.id && (cancel || s.state != state::RUNNING) {
            give_back(base, order, s.entity);
        }
        for slot in base.slots(s.entity).into_iter().filter(|x| x.count != 0) {
            if inputs.contains(&slot.item) || recipe.makes(slot.item) > 0 {
                let moved = deposit(base, s.entity, slot.id, slot.item, slot.count, &chests);
                if slot.item == order.product {
                    order.produced += moved;
                }
            }
        }
    }
}

/// One work step for an order whose assistant is the entity `assistant`. `claimed` collects
/// the stations orders use this round, so two orders never share one.
pub fn step(table: &Table, order: &mut Order, assistant: u32, base: &dyn Base, claimed: &mut Vec<u32>) -> Status {
    let Some(recipe) = table.recipe(order.recipe) else { return Status::NoStation };
    rebind(base, order, assistant);
    let stations = match stations_for(table, base, recipe, assistant, claimed) {
        Ok(stations) => stations,
        Err(status) => return status,
    };
    claimed.extend(stations.iter().map(|s| s.entity));
    let chests = base.chests_near(assistant, RADIUS);

    // results into storage
    let mut stuck = false;
    for s in &stations {
        for slot in base.slots(s.entity).into_iter().filter(|x| x.count != 0 && recipe.makes(x.item) > 0) {
            let moved = deposit(base, s.entity, slot.id, slot.item, slot.count, &chests);
            if slot.item == order.product {
                order.produced += moved;
            }
            if moved != 0 {
                log!("assistants: order {}: {moved} x item {} from station {} into storage ({}/{})", order.id, slot.item, s.entity, order.produced, order.amount);
            }
            stuck |= moved < slot.count;
        }
    }
    if order.produced >= order.amount {
        release(table, order, assistant, base, false);
        return Status::Done;
    }

    // what is on its way: runs going on, results still in a station
    let per_run = recipe.makes(order.product);
    let running = stations.iter().filter(|s| s.state == state::RUNNING && s.running == recipe.id).count() as u32;
    let waiting: u32 = stations.iter().map(|s| counts_of(base, &[s.entity]).get(&order.product).copied().unwrap_or(0)).sum();
    let left = order.amount.saturating_sub(order.produced + running * per_run + waiting);
    let mut runs = planner::runs_for(recipe, order.product, left);

    let mut storage = counts_of(base, &chests);
    let mut fed = false;
    let mut needs = Vec::new();
    let mut energy = false;
    let mut dry = false;
    for (i, s) in stations.iter().enumerate() {
        let held = counts_of(base, &[s.entity]);
        // runs the station already has material for
        let have = planner::feed(table, recipe, BATCH, &held, &Counts::new()).runs;
        // the runs still needed, shared among the stations left
        let want = runs.div_ceil((stations.len() - i) as u32).min(BATCH);
        let plan = planner::feed(table, recipe, want.max(have), &held, &storage);
        if want > have {
            for &(item, count) in &plan.moves {
                let got = take(base, &chests, item, count, s.entity);
                if let Some(c) = storage.get_mut(&item) {
                    *c = c.saturating_sub(got);
                }
                log!("assistants: order {}: {got} of {count} x item {item} into station {}", order.id, s.entity);
            }
        }
        runs = runs.saturating_sub(plan.runs.max(have));
        let has_material = plan.runs > 0 || have > 0;
        if has_material {
            fed = true;
            dry |= !water(base, recipe, s.entity, plan.runs.max(have), assistant, order.id);
            if s.next != recipe.id {
                if s.next != 0 && !order.borrowed.iter().any(|b| b.0 == s.entity) {
                    let pos = base.position(s.entity).map_or([0.0; 3], |p| p.map(|v| v as f32));
                    order.borrowed.push((s.entity, s.next, s.selections.clone(), pos));
                }
                let set = base.set_recipe(s.entity, recipe.id, &plan.selections);
                log!("assistants: order {}: station {} (state {}) set to {} {:?}: {set}", order.id, s.entity, s.state, recipe.name, plan.selections);
            }
            energy |= s.missing & (missing::ENERGY_SOURCE | missing::ENERGY_AMOUNT | missing::ABOVE_WATER) != 0;
        } else if s.state != state::RUNNING {
            if s.next == recipe.id {
                // nothing to make on it now: give it back to its owner's own use
                give_back(base, order, s.entity);
            }
            if needs.is_empty() {
                needs = plan.missing;
            }
        }
    }
    if stuck {
        Status::StorageFull
    } else if dry && running == 0 {
        Status::NeedsWater
    } else if energy {
        Status::NeedsEnergy
    } else if fed || running > 0 || waiting > 0 {
        Status::Running
    } else {
        Status::Missing(needs)
    }
}

#[cfg(test)]
pub mod fake {
    //! A simulated base: chests, NPCs and stations that run recipes the way the game does
    //! (inputs taken at the start, results put into the station at the end).
    use std::cell::RefCell;
    use std::collections::HashMap;

    use shroudlink_sdk::Slot;

    use crate::base::{state, Base, Harvest, Home, Station, Tank};
    use crate::recipes::{Input, Table};

    pub struct FakeBase {
        pub inventories: RefCell<HashMap<u32, Vec<(u32, u32)>>>,
        pub stations: RefCell<Vec<Station>>,
        pub npcs: Vec<(u32, u32)>,
        pub max_stack: u32,
        pub tanks: RefCell<HashMap<u32, Tank>>,
        pub wells: Vec<u32>,
        pub harvest: Vec<(u32, Harvest)>,
        /// crops the game was told are looted
        pub finished: RefCell<Vec<u32>>,
        /// crop -> its seedling
        pub seedlings: HashMap<u32, u32>,
        pub replanted: RefCell<Vec<u32>>,
        pub homes: RefCell<HashMap<u32, Home>>,
        pub positions: HashMap<u32, [f64; 3]>,
        pub assistants: Vec<u32>,
        pub growing: Vec<[f64; 3]>,
        /// (feeder, building item)
        pub feeders: Vec<(u32, u32)>,
        /// scatter-loot crops: what picking them gives
        pub loot: HashMap<u32, Vec<(u32, u32)>>,
        pub looted: RefCell<Vec<u32>>,
        /// growing crop stages: (stage, seedling, false)
        pub young: Vec<(u32, u32, bool)>,
    }

    impl FakeBase {
        pub fn new(npcs: Vec<(u32, u32)>) -> FakeBase {
            FakeBase { inventories: RefCell::default(), stations: RefCell::default(), npcs, max_stack: 50, tanks: RefCell::default(), wells: Vec::new(), harvest: Vec::new(), finished: RefCell::default(), seedlings: HashMap::new(), replanted: RefCell::default(), homes: RefCell::default(), positions: HashMap::new(), assistants: Vec::new(), growing: Vec::new(), feeders: Vec::new(), loot: HashMap::new(), looted: RefCell::default(), young: Vec::new() }
        }
        pub fn chest(&self, entity: u32, slots: usize, items: &[(u32, u32)]) {
            let mut inv = vec![(0, 0); slots];
            for (i, &it) in items.iter().enumerate() {
                inv[i] = it;
            }
            self.inventories.borrow_mut().insert(entity, inv);
        }
        pub fn station(&self, entity: u32, workshop: u32) {
            self.chest(entity, 16, &[]);
            self.stations.borrow_mut().push(Station { entity, workshop, state: state::NO_RECIPE, running: 0, next: 0, selections: Vec::new(), missing: 0 });
        }
        pub fn count(&self, entity: u32, item: u32) -> u32 {
            self.inventories.borrow()[&entity].iter().filter(|s| s.0 == item).map(|s| s.1).sum()
        }
        fn remove(&self, entity: u32, item: u32, mut n: u32) {
            for s in self.inventories.borrow_mut().get_mut(&entity).unwrap().iter_mut().filter(|s| s.0 == item) {
                let k = s.1.min(n);
                s.1 -= k;
                n -= k;
                if s.1 == 0 {
                    s.0 = 0;
                }
            }
        }
        /// Adds items, partial stacks first; returns what did not fit.
        fn add(&self, entity: u32, item: u32, mut n: u32) -> u32 {
            let max = self.max_stack;
            let mut inv = self.inventories.borrow_mut();
            let inv = inv.get_mut(&entity).unwrap();
            for s in inv.iter_mut().filter(|s| s.0 == item && s.1 < max) {
                let k = (max - s.1).min(n);
                s.1 += k;
                n -= k;
            }
            for s in inv.iter_mut().filter(|s| s.1 == 0) {
                if n == 0 {
                    break;
                }
                let k = max.min(n);
                *s = (item, k);
                n -= k;
            }
            n
        }

        /// One frame of the game's station system.
        pub fn advance(&self, table: &Table) {
            let stations = self.stations.borrow().clone();
            for (i, s) in stations.iter().enumerate() {
                let mut s = s.clone();
                if s.state == state::RUNNING {
                    let recipe = table.recipe(s.running).unwrap();
                    for &(item, n) in &recipe.outputs {
                        self.add(s.entity, item, n);
                    }
                    s.state = state::NO_RECIPE;
                    s.running = 0;
                } else if s.next != 0 {
                    let recipe = table.recipe(s.next).unwrap();
                    let need: Vec<(u32, u32)> = recipe
                        .inputs
                        .iter()
                        .map(|input| match *input {
                            Input::Item { item, count } => (item, count),
                            Input::Category { id, count } => {
                                let opts = &table.categories[&id];
                                *opts.iter().map(|o| (o.0, count)).find(|&(it, c)| self.count(s.entity, it) >= c).get_or_insert((opts[0].0, count))
                            }
                        })
                        .collect();
                    let water = self.tanks.borrow().get(&s.entity).map_or(true, |t| t.amount >= recipe.charges * super::WATER_UNITS);
                    if water && need.iter().all(|&(item, n)| self.count(s.entity, item) >= n) {
                        for &(item, n) in &need {
                            self.remove(s.entity, item, n);
                        }
                        if let Some(t) = self.tanks.borrow_mut().get_mut(&s.entity) {
                            t.amount -= recipe.charges * super::WATER_UNITS;
                        }
                        s.state = state::RUNNING;
                        s.running = s.next;
                    } else {
                        s.state = state::MISSING_MATERIALS;
                    }
                }
                self.stations.borrow_mut()[i] = s;
            }
        }
    }

    fn slot_id(entity: u32, index: usize) -> u64 {
        entity as u64 | (index as u64) << 32
    }

    impl Base for FakeBase {
        fn npcs_near(&self, _: u32, _: f64) -> Vec<(u32, u32)> {
            self.npcs.clone()
        }
        fn stations_near(&self, _: u32, _: f64) -> Vec<Station> {
            self.stations.borrow().clone()
        }
        fn chests_near(&self, _: u32, _: f64) -> Vec<u32> {
            let mut not_storage: Vec<u32> = self.stations.borrow().iter().map(|s| s.entity).collect();
            not_storage.extend(self.harvest.iter().map(|h| h.0));
            not_storage.extend(self.feeders.iter().map(|f| f.0));
            let mut chests: Vec<u32> = self.inventories.borrow().keys().copied().filter(|e| !not_storage.contains(e)).collect();
            chests.sort();
            chests
        }
        fn slots(&self, entity: u32) -> Vec<Slot> {
            self.inventories.borrow().get(&entity).map_or(Vec::new(), |inv| inv.iter().enumerate().map(|(i, &(item, count))| Slot { id: slot_id(entity, i), item, count }).collect())
        }
        fn move_items(&self, from: u32, slot: u64, to: u32, count: u32) -> u32 {
            let index = (slot >> 32) as usize;
            let (item, have) = self.inventories.borrow()[&from][index];
            if have == 0 {
                return 0;
            }
            let n = have.min(count);
            let left = self.add(to, item, n);
            let moved = n - left;
            let mut inv = self.inventories.borrow_mut();
            let s = &mut inv.get_mut(&from).unwrap()[index];
            s.1 -= moved;
            if s.1 == 0 {
                s.0 = 0;
            }
            moved
        }
        fn set_recipe(&self, station: u32, recipe: u32, _: &[(u32, u8)]) -> bool {
            let mut stations = self.stations.borrow_mut();
            let s = stations.iter_mut().find(|s| s.entity == station).unwrap();
            s.next = recipe;
            true
        }
        fn tank(&self, entity: u32) -> Option<Tank> {
            self.tanks.borrow().get(&entity).copied()
        }
        fn set_tank(&self, entity: u32, amount: u32) -> bool {
            match self.tanks.borrow_mut().get_mut(&entity) {
                Some(t) => {
                    t.amount = amount.min(t.max);
                    true
                }
                None => false,
            }
        }
        fn wells_near(&self, _: u32, _: f64) -> Vec<u32> {
            self.wells.clone()
        }
        fn harvestables_near(&self, _: u32, _: f64) -> Vec<(u32, Harvest)> {
            self.harvest.clone()
        }
        fn growing_near(&self, _: u32, _: f64) -> Vec<[f64; 3]> {
            self.growing.clone()
        }
        fn finish_crop(&self, crop: u32) -> bool {
            self.finished.borrow_mut().push(crop);
            true
        }
        fn give(&self, entity: u32, item: u32, count: u32) -> u32 {
            count - self.add(entity, item, count)
        }
        fn remove(&self, entity: u32, slot: u64, count: u32) -> u32 {
            let index = (slot >> 32) as usize;
            let mut inv = self.inventories.borrow_mut();
            let s = &mut inv.get_mut(&entity).unwrap()[index];
            let n = s.1.min(count);
            s.1 -= n;
            if s.1 == 0 {
                s.0 = 0;
            }
            n
        }
        fn fishing_spot_near(&self, _: u32, _: f64) -> Option<String> {
            None
        }
        fn feeders_near(&self, _: u32, _: f64) -> Vec<(u32, u32)> {
            self.feeders.clone()
        }
        fn crop_loot(&self, crop: u32) -> Option<Vec<(u32, u32)>> {
            (!self.looted.borrow().contains(&crop)).then(|| self.loot.get(&crop).cloned()).flatten()
        }
        fn mark_looted(&self, crop: u32) -> bool {
            self.looted.borrow_mut().push(crop);
            true
        }
        fn crops_near(&self, _: u32, _: f64) -> Vec<(u32, u32, bool)> {
            self.harvest.iter().filter(|h| h.1 == Harvest::Crop).filter_map(|h| Some((h.0, *self.seedlings.get(&h.0)?, true))).chain(self.young.iter().copied()).collect()
        }
        fn crop_picked(&self, crop: u32) -> bool {
            self.looted.borrow().contains(&crop) && self.inventories.borrow().get(&crop).is_none_or(|i| i.iter().all(|s| s.1 == 0))
        }
        fn crop_seedling(&self, crop: u32) -> Option<u32> {
            self.seedlings.get(&crop).copied()
        }
        fn replant(&self, crop: u32) -> bool {
            self.replanted.borrow_mut().push(crop);
            true
        }
        fn position(&self, entity: u32) -> Option<[f64; 3]> {
            self.positions.get(&entity).copied()
        }
        fn is_assistant(&self, npc: u32) -> bool {
            self.assistants.contains(&npc)
        }
        fn npc_home(&self, npc: u32) -> Option<Home> {
            self.homes.borrow().get(&npc).copied()
        }
        fn set_npc_home(&self, npc: u32, home: Home) -> bool {
            self.homes.borrow_mut().insert(npc, home);
            true
        }
        fn nudge_npc(&self, _: u32) -> bool {
            true
        }
        fn work_at_station(&self, _: u32) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::FakeBase;
    use crate::base::Tank;
    use super::*;

    const KITCHEN: u32 = 100;
    const STEW: u32 = 20;
    const MEAT: u32 = 10;
    const VEG_A: u32 = 30;
    const VEG_B: u32 = 31;
    const ASSISTANT: u32 = 1;
    const STATION: u32 = 50;
    const CHEST: u32 = 60;

    fn table() -> Table {
        Table::parse(
            "recipe\t1\t100\t60\t0\t0\t0\titem:10:2,cat:50:1\t20:5\tStew\n\
             recipe\t2\t100\t60\t0\t0\t0\titem:10:1\t21:1\tOther\n\
             recipe\t3\t100\t60\t0\t0\t0\titem:10:1\t22:1\tSoup\t5\n\
             category\t50\t30:1.0,31:1.0\n",
        )
    }

    fn order(amount: u32) -> Order {
        Order { id: 1, npc: 7, product: STEW, recipe: 1, amount, produced: 0, by: "p".into(), status: Status::Away, parent: None, borrowed: Vec::new() }
    }

    /// Steps the order and the simulated game until it is done (or gives up).
    fn run(t: &Table, base: &FakeBase, o: &mut Order) -> Status {
        let mut status = Status::Away;
        for _ in 0..100 {
            status = step(t, o, ASSISTANT, base, &mut Vec::new());
            if status == Status::Done {
                break;
            }
            base.advance(t);
        }
        status
    }

    #[test]
    fn makes_the_amount_ordered_and_puts_it_into_storage() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        // material for 4 runs; 12 stew take 3 runs of 5
        base.chest(CHEST, 48, &[(MEAT, 8), (VEG_A, 2), (VEG_B, 2)]);
        let mut o = order(12);
        assert_eq!(run(&t, &base, &mut o), Status::Done);
        assert_eq!(o.produced, 15);
        assert_eq!(base.count(CHEST, STEW), 15);
        // the fourth run's material is back in storage, the station is free again
        assert_eq!(base.count(CHEST, MEAT), 2);
        assert_eq!(base.count(CHEST, VEG_A) + base.count(CHEST, VEG_B), 1);
        assert_eq!(base.count(STATION, MEAT), 0);
        assert_eq!(base.stations.borrow()[0].next, 0);
    }

    #[test]
    fn runs_several_stations_side_by_side() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.station(STATION + 1, KITCHEN);
        base.chest(CHEST, 48, &[(MEAT, 40), (VEG_A, 20)]);
        let mut o = order(20);
        step(&t, &mut o, ASSISTANT, &base, &mut Vec::new());
        assert!(base.stations.borrow().iter().all(|s| s.next == 1), "both stations got the recipe");
        assert_eq!(run(&t, &base, &mut o), Status::Done);
        assert_eq!(base.count(CHEST, STEW), 20);
    }

    #[test]
    fn says_what_is_missing() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.chest(CHEST, 48, &[(MEAT, 1)]);
        let mut o = order(5);
        assert_eq!(step(&t, &mut o, ASSISTANT, &base, &mut Vec::new()), Status::Missing(vec![(MEAT, 1), (VEG_A, 1)]));
        assert_eq!(base.stations.borrow()[0].next, 0, "an idle station is not claimed");
    }

    #[test]
    fn leaves_a_station_alone_that_runs_something_else() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.stations.borrow_mut()[0].next = 2;
        base.stations.borrow_mut()[0].running = 2;
        base.stations.borrow_mut()[0].state = state::RUNNING;
        base.chest(CHEST, 48, &[(MEAT, 10), (VEG_A, 10)]);
        let mut o = order(5);
        assert!(matches!(step(&t, &mut o, ASSISTANT, &base, &mut Vec::new()), Status::Busy(_)));
        assert_eq!(base.count(CHEST, MEAT), 10);
    }

    #[test]
    fn takes_over_an_idle_station_set_to_another_recipe_and_gives_it_back() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        // the player's recipe, out of materials
        base.stations.borrow_mut()[0].next = 2;
        base.stations.borrow_mut()[0].selections = vec![(77, 1)];
        base.stations.borrow_mut()[0].state = state::MISSING_MATERIALS;
        base.chest(CHEST, 48, &[(MEAT, 10), (VEG_A, 10)]);
        let mut o = order(5);
        step(&t, &mut o, ASSISTANT, &base, &mut Vec::new());
        assert_eq!(base.stations.borrow()[0].next, 1, "taken over");
        assert_eq!(o.borrowed, vec![(STATION, 2, vec![(77, 1)], [0.0; 3])]);
        release(&t, &mut o, ASSISTANT, &base, true);
        assert_eq!(base.stations.borrow()[0].next, 2, "the player's recipe is back");
        assert!(o.borrowed.is_empty());
    }

    #[test]
    fn a_borrowed_station_is_found_again_after_a_restart() {
        let t = table();
        let mut base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.positions.insert(STATION, [5.0, 0.0, 5.0]);
        base.stations.borrow_mut()[0].next = 2;
        base.stations.borrow_mut()[0].state = state::MISSING_MATERIALS;
        base.chest(CHEST, 48, &[(MEAT, 10), (VEG_A, 10)]);
        let mut o = order(5);
        step(&t, &mut o, ASSISTANT, &base, &mut Vec::new());
        // a restart: the same station under a new entity id
        const NEW: u32 = 99;
        base.stations.borrow_mut()[0].entity = NEW;
        let slots = base.inventories.borrow_mut().remove(&STATION).unwrap();
        base.inventories.borrow_mut().insert(NEW, slots);
        base.positions.insert(NEW, [5.2, 0.0, 5.0]);
        base.positions.remove(&STATION);
        release(&t, &mut o, ASSISTANT, &base, true);
        assert_eq!(base.stations.borrow()[0].next, 2, "the player's recipe is back on the same station");
    }

    #[test]
    fn two_orders_never_share_a_station() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.chest(CHEST, 48, &[(MEAT, 10), (VEG_A, 10)]);
        let mut claimed = Vec::new();
        let (mut a, mut b) = (order(5), order(5));
        assert_eq!(step(&t, &mut a, ASSISTANT, &base, &mut claimed), Status::Running);
        assert_eq!(step(&t, &mut b, ASSISTANT, &base, &mut claimed), Status::NoStation);
    }

    #[test]
    fn full_storage_is_reported_and_nothing_is_lost() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        // one slot each, and both stay occupied: the stew has nowhere to go
        base.chest(CHEST, 1, &[(MEAT, 50)]);
        base.chest(CHEST + 1, 1, &[(VEG_A, 50)]);
        let mut o = order(10);
        step(&t, &mut o, ASSISTANT, &base, &mut Vec::new());
        base.advance(&t);
        base.advance(&t);
        assert_eq!(step(&t, &mut o, ASSISTANT, &base, &mut Vec::new()), Status::StorageFull);
        assert_eq!(base.count(STATION, STEW), 5, "the results wait in the station");
        assert_eq!(o.produced, 0);
    }

    #[test]
    fn carries_water_from_the_well_to_the_station() {
        let t = table();
        const WELL: u32 = 70;
        let mut base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.wells = vec![WELL];
        base.station(STATION, KITCHEN);
        base.tanks.borrow_mut().insert(STATION, Tank { amount: 0, max: 6400 });
        base.tanks.borrow_mut().insert(WELL, Tank { amount: 16000, max: 16000 });
        base.chest(CHEST, 48, &[(MEAT, 3)]);
        let mut o = Order { recipe: 3, product: 22, ..order(3) };
        assert_eq!(run(&t, &base, &mut o), Status::Done);
        assert_eq!(base.count(CHEST, 22), 3);
        // the tank was filled once (6400 from the well), three runs took 3 x 5 charges
        assert_eq!(base.tanks.borrow()[&WELL].amount, 9600);
        assert_eq!(base.tanks.borrow()[&STATION].amount, 6400 - 15 * WATER_UNITS);
    }

    #[test]
    fn without_water_the_order_says_so() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.tanks.borrow_mut().insert(STATION, Tank { amount: 2 * WATER_UNITS, max: 6400 });
        base.chest(CHEST, 48, &[(MEAT, 3)]);
        let mut o = Order { recipe: 3, product: 22, ..order(3) };
        assert_eq!(step(&t, &mut o, ASSISTANT, &base, &mut Vec::new()), Status::NeedsWater);
    }

    #[test]
    fn cancelling_returns_the_material() {
        let t = table();
        let base = FakeBase::new(vec![(ASSISTANT, 7)]);
        base.station(STATION, KITCHEN);
        base.chest(CHEST, 48, &[(MEAT, 20), (VEG_A, 10)]);
        let mut o = order(25);
        step(&t, &mut o, ASSISTANT, &base, &mut Vec::new());
        assert!(base.count(STATION, MEAT) > 0);
        release(&t, &mut o, ASSISTANT, &base, true);
        assert_eq!((base.count(CHEST, MEAT), base.count(CHEST, VEG_A)), (20, 10));
        assert_eq!(base.stations.borrow()[0].next, 0);
    }
}
