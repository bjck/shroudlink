//! Standing jobs, switched on per survivor in the window and kept until switched off:
//!
//! - the **gardener** (the Farmer or the Farmer's assistant) harvests what grows around them
//!   into the chests: ripe crops, which they replant with a seedling from the chests (the
//!   plant starts over from its first stage; without a seedling the crop stays ripe until
//!   there is one, as picking it would leave the soil empty), and the fruit of planted
//!   trees and bushes and the produce of tame animals, which refill on their own;
//! - the **fisher** fishes: one catch a minute while the base is loaded, each costing one
//!   bait from the chests, the fish drawn from the spawn table of a fishing spot nearby (or
//!   the one players fish from anywhere) as the bait attracts it. The fish is created by
//!   the game's own inventory function (core v7), after the bait has been taken.

use shroudlink_sdk::log;

use crate::base::{Base, Harvest};
use crate::fishing::{self, Dice};
use crate::work::{deposit, RADIUS};

/// `NpcResource.workshopId` of the Farmer and the Farmer's assistant.
pub const FARMER_WORKSHOP: u32 = 3651361391;
/// Workshop of the Fisher (`NPC_Workshop_Fisher_Trader`).
pub const FISHER: u32 = 3299655940;
/// Seconds of fishing per catch.
pub const SECONDS_PER_CATCH: u32 = 60;
/// Catches made up at most in one step (after a pause the fisher does not empty the chests).
const MAX_CATCHES: u32 = 3;
/// How far from the fisher a fishing spot counts, in metres.
const SPOT_RADIUS: f64 = 60.0;
/// How far around the farmer feeders are looked after outside a base (inside one: its whole
/// build zone).
const FEEDER_RADIUS: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Harvest,
    Fish,
    /// keeping the animal feeders filled
    Feed,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Harvest => "harvest",
            Kind::Fish => "fish",
            Kind::Feed => "feed",
        }
    }
    pub fn from_name(name: &str) -> Option<Kind> {
        match name {
            "harvest" => Some(Kind::Harvest),
            "fish" => Some(Kind::Fish),
            "feed" => Some(Kind::Feed),
            _ => None,
        }
    }
}

/// The jobs the people of a workshop can do.
pub fn kinds_for(workshop: u32) -> Vec<Kind> {
    let mut kinds = Vec::new();
    if workshop == FARMER_WORKSHOP {
        kinds.push(Kind::Harvest);
        kinds.push(Kind::Feed);
    }
    if workshop == FISHER {
        kinds.push(Kind::Fish);
    }
    kinds
}

#[derive(Clone, Debug, PartialEq)]
pub struct Job {
    pub npc: u32,
    pub kind: Kind,
    /// items harvested or fish caught so far
    pub done: u32,
    /// seconds fished towards the next catch
    pub progress: u32,
    pub status: String,
    /// crops replanted so far (gardener)
    pub replanted: u32,
    /// where the fisher fishes (set with "Fish here" in the window)
    pub spot: Option<[f32; 3]>,
    /// where the gardener last harvested
    pub site: Option<[f32; 3]>,
    /// what this step brought in, for the players' notification: (item, count)
    pub news: Vec<(u32, u32)>,
    /// the most crops of a kind replanted (or left without a seedling) in one round:
    /// (seedling, count), about the size of that field; the seedling stock is kept above it
    pub peaks: Vec<(u32, u32)>,
    /// the baits the fisher uses first, in this order (set in the window); the others after
    pub baits: Vec<u32>,
}

/// Every bait, in the order the fisher uses them: `preferred` first, then the others by
/// name.
pub fn bait_order(preferred: &[u32]) -> Vec<u32> {
    let data = fishing::data();
    let mut order: Vec<u32> = preferred.iter().copied().filter(|&b| data.is_bait(b)).collect();
    let mut rest: Vec<u32> = data.baits.keys().copied().filter(|b| !order.contains(b)).collect();
    rest.sort_by_key(|&b| (crate::names::item(b), b));
    order.extend(rest);
    order
}

/// Seedlings kept in the chests beyond a field's size.
pub const SEEDLING_RESERVE: u32 = 5;

/// Seedlings to order so the chests hold a field's worth plus the reserve: (seedling,
/// amount), from the peaks and the seedlings in stock.
pub fn seedlings_short(peaks: &[(u32, u32)], stock: impl Fn(u32) -> u32) -> Vec<(u32, u32)> {
    peaks
        .iter()
        .filter_map(|&(seedling, peak)| {
            let want = peak + SEEDLING_RESERVE;
            let have = stock(seedling);
            (have < want).then(|| (seedling, want - have))
        })
        .collect()
}

impl Job {
    fn report(&mut self, item: u32, count: u32) {
        match self.news.iter_mut().find(|n| n.0 == item) {
            Some(n) => n.1 += count,
            None => self.news.push((item, count)),
        }
    }
}

impl Job {
    pub fn new(npc: u32, kind: Kind) -> Job {
        Job { npc, kind, done: 0, progress: 0, status: "waiting until someone is at the base".into(), replanted: 0, spot: None, site: None, news: Vec::new(), peaks: Vec::new(), baits: Vec::new() }
    }
}

/// Empties everything ripe around the gardener into the chests. Returns the items moved.
pub fn harvest(base: &dyn Base, gardener: u32, job: &mut Job) -> u32 {
    job.news.clear();
    let chests = base.chests_near(gardener, RADIUS);
    let mut moved = 0;
    let mut stuck = false;
    let mut no_seedling = None;
    // seedlings this round wanted, by crop kind
    let mut round: Vec<(u32, u32)> = Vec::new();
    let mut want = |seedling: u32| match round.iter_mut().find(|r| r.0 == seedling) {
        Some(r) => r.1 += 1,
        None => round.push((seedling, 1)),
    };
    let mut harvested = Vec::new();
    let mut crops_harvested = Vec::new();
    for (plant, kind) in base.harvestables_near(gardener, RADIUS) {
        let mut from_plant = 0;
        // a crop is picked only when it can be replanted: without a seedling it stays ripe
        // on its field (picking it would leave the soil empty) until the Seedbed made some
        if kind == Harvest::Crop {
            if let Some(seedling) = base.crop_seedling(plant) {
                let have = chests.iter().any(|&c| base.slots(c).iter().any(|s| s.item == seedling && s.count != 0));
                if !have {
                    want(seedling);
                    no_seedling = Some(seedling);
                    continue;
                }
            }
        }
        // a scatter-loot crop (beans …) holds nothing: its harvest is made as picking makes
        // it, after it is marked picked, into the chests
        if let (Harvest::Crop, Some(loot)) = (kind, base.crop_loot(plant)) {
            if base.mark_looted(plant) {
                for (item, count) in loot {
                    let mut left = count;
                    for &chest in &chests {
                        if left == 0 {
                            break;
                        }
                        left -= base.give(chest, item, left);
                    }
                    if count > left {
                        job.report(item, count - left);
                    }
                    from_plant += count - left;
                    stuck |= left > 0;
                }
            }
        }
        for slot in base.slots(plant).into_iter().filter(|s| s.count != 0) {
            let n = deposit(base, plant, slot.id, slot.item, slot.count, &chests);
            if n > 0 {
                job.report(slot.item, n);
            }
            from_plant += n;
            stuck |= n < slot.count;
        }
        if from_plant > 0 {
            log!("assistants: harvested {from_plant} from {plant} ({kind:?})");
            harvested.extend(base.position(plant));
            if kind == Harvest::Crop {
                crops_harvested.extend(base.position(plant));
            }
        }
        moved += from_plant;
        // a crop is finished (or replanted) only right after its harvest was taken, or when
        // it was picked before and still waits for its seedling
        let picked_before = from_plant == 0 && kind == Harvest::Crop && base.crop_picked(plant);
        if kind != Harvest::Crop || (from_plant == 0 && !picked_before) || base.slots(plant).iter().any(|s| s.count != 0) {
            continue;
        }
        match replant(base, plant, &chests) {
            Ok(seedling) => {
                want(seedling);
                job.replanted += 1;
                log!("assistants: crop {plant} harvested and replanted with item {seedling}");
            }
            // no seedling, or a crop the table does not know: done, the soil stays empty
            Err(missing) if missing.is_some() || base.crop_seedling(plant).is_none() => {
                if let Some(seedling) = missing {
                    want(seedling);
                    no_seedling = missing;
                }
                if base.finish_crop(plant) {
                    log!("assistants: crop {plant} harvested, not replanted");
                }
            }
            // the plant could not start over (or did already, and the game has not swapped
            // the stage yet): left as it is, never marked for removal
            Err(_) => {}
        }
    }
    job.done += moved;
    for (seedling, n) in round {
        match job.peaks.iter_mut().find(|p| p.0 == seedling) {
            Some(p) => p.1 = p.1.max(n),
            None => job.peaks.push((seedling, n)),
        }
    }
    // where they work: a crop just harvested, else among the growing crops, else the last
    // place they harvested (fruit trees, animals)
    let site = crate::walk::site_of(&crops_harvested).or_else(|| crate::walk::site_of(&base.growing_near(gardener, RADIUS))).or_else(|| crate::walk::site_of(&harvested));
    if site.is_some() {
        job.site = site;
    }
    job.status = if stuck {
        "no room in the chests".into()
    } else if let Some(seedling) = no_seedling {
        format!("ripe crops wait for {} to replant them", crate::names::item(seedling))
    } else if moved > 0 {
        format!("harvested {moved}")
    } else {
        "nothing ripe right now".into()
    };
    moved
}

/// Replants an emptied crop with its seedling from the chests: the seedling goes first and
/// comes back if the plant cannot start over. Err(Some(seedling)) when none is there,
/// Err(None) when the crop is unknown or could not be replanted.
fn replant(base: &dyn Base, crop: u32, chests: &[u32]) -> Result<u32, Option<u32>> {
    let seedling = base.crop_seedling(crop).ok_or(None)?;
    let (chest, slot) = chests.iter().find_map(|&c| base.slots(c).into_iter().find(|s| s.item == seedling && s.count != 0).map(|s| (c, s.id))).ok_or(Some(seedling))?;
    if base.remove(chest, slot, 1) != 1 {
        return Err(None);
    }
    if base.replant(crop) {
        return Ok(seedling);
    }
    if base.give(chest, seedling, 1) != 1 {
        log!("assistants: crop {crop} could not be replanted, and seedling {seedling} could not go back to {chest}");
    }
    Err(None)
}

/// Fills the empty animal feeders around the farmer: with their food from the chests, else
/// with a batch made from the ingredients in the chests (the Farmer's workshop recipe, as a
/// player crafts it there: the ingredients go first, then the food is made in the feeder,
/// so a failure can cost ingredients, never make food twice). Returns the food put in.
pub fn feed(base: &dyn Base, farmer: u32, job: &mut Job) -> u32 {
    job.news.clear();
    let feeding = crate::feeding::data();
    let chests = base.chests_near(farmer, RADIUS);
    let mut filled = 0;
    let mut short: Option<String> = None;
    let feeders = base.feeders_near(farmer, FEEDER_RADIUS);
    for &(feeder, item) in &feeders {
        let Some(food) = feeding.food_of(item) else { continue };
        // what is no food for it goes back to the chests (feeders counted as chests once,
        // and the gardener put harvest into them)
        for s in base.slots(feeder).into_iter().filter(|s| s.count != 0 && s.item != food) {
            let moved = deposit(base, feeder, s.id, s.item, s.count, &chests);
            log!("assistants: feeder {feeder}: {moved} x item {} that is no food for it back to the chests", s.item);
        }
        if base.slots(feeder).iter().any(|s| s.count != 0) {
            continue;
        }
        // food the players made already
        let mut put = 0;
        for &chest in &chests {
            for s in base.slots(chest).into_iter().filter(|s| s.item == food && s.count != 0) {
                put += base.move_items(chest, s.id, feeder, u32::MAX);
            }
            if put > 0 {
                break;
            }
        }
        if put == 0 {
            put = make_food(base, &chests, feeder, food).unwrap_or_else(|missing| {
                short = Some(format!("needs {} for {}", missing, crate::names::item(food)));
                0
            });
        }
        if put > 0 {
            job.site = base.position(feeder).map(|p| p.map(|v| v as f32));
            log!("assistants: feeder {feeder} filled with {put} x item {food}");
            job.report(food, put);
            filled += put;
        }
    }
    job.done += filled;
    job.status = match (feeders.is_empty(), short) {
        (true, _) => "no animal feeder nearby".into(),
        (false, Some(short)) => short,
        (false, None) if filled > 0 => format!("filled {filled}"),
        (false, None) => "the feeders have food".into(),
    };
    filled
}

/// Makes one batch of `food` into `feeder` from the chests; Err(what is missing).
fn make_food(base: &dyn Base, chests: &[u32], feeder: u32, food: u32) -> Result<u32, String> {
    let feeding = crate::feeding::data();
    let recipe = feeding.recipe(food).ok_or_else(|| "a recipe".to_string())?;
    let mut stock = crate::planner::Counts::new();
    for &c in chests {
        for s in base.slots(c).into_iter().filter(|s| s.count != 0) {
            *stock.entry(s.item).or_default() += s.count;
        }
    }
    let plan = crate::planner::feed(&feeding.table, recipe, 1, &crate::planner::Counts::new(), &stock);
    if plan.runs == 0 {
        let missing: Vec<String> = plan.missing.iter().map(|&(i, n)| format!("{n} {}", crate::names::item(i))).collect();
        return Err(missing.join(", "));
    }
    for &(item, count) in &plan.moves {
        let mut left = count;
        for &c in chests {
            for s in base.slots(c).into_iter().filter(|s| s.item == item && s.count != 0) {
                if left == 0 {
                    break;
                }
                left -= base.remove(c, s.id, left.min(s.count));
            }
        }
        if left > 0 {
            log!("assistants: {left} of {count} x item {item} could not be taken for feeder {feeder}");
            return Err(format!("{left} {}", crate::names::item(item)));
        }
    }
    Ok(base.give(feeder, food, recipe.makes(food)))
}

/// The first bait of the fisher's order that is in the chests: (chest, slot, bait item).
fn find_bait(base: &dyn Base, chests: &[u32], preferred: &[u32]) -> Option<(u32, u64, u32)> {
    let slots: Vec<(u32, shroudlink_sdk::Slot)> = chests.iter().flat_map(|&c| base.slots(c).into_iter().filter(|s| s.count != 0).map(move |s| (c, s))).collect();
    bait_order(preferred).into_iter().find_map(|bait| slots.iter().find(|(_, s)| s.item == bait).map(|(c, s)| (*c, s.id, bait)))
}

/// Fishes for `seconds`; returns the fish caught.
pub fn fish(base: &dyn Base, fisher: u32, seconds: u32, job: &mut Job, dice: &mut Dice) -> u32 {
    job.news.clear();
    job.progress = (job.progress + seconds).min(SECONDS_PER_CATCH * MAX_CATCHES);
    if job.progress < SECONDS_PER_CATCH {
        job.status = "fishing".into();
        return 0;
    }
    let data = fishing::data();
    let table = base.fishing_spot_near(fisher, SPOT_RADIUS).unwrap_or_else(|| data.default_table.clone());
    let chests = base.chests_near(fisher, RADIUS);
    let mut caught = 0;
    while job.progress >= SECONDS_PER_CATCH {
        let Some((chest, slot, bait)) = find_bait(base, &chests, &job.baits) else {
            job.status = "no bait in the chests".into();
            job.progress = SECONDS_PER_CATCH;
            break;
        };
        let Some(catch) = data.draw(&table, bait, dice.roll()) else {
            job.status = "their bait catches nothing here".into();
            job.progress = SECONDS_PER_CATCH;
            break;
        };
        // the bait goes first: a failure below can cost a bait, never make a fish twice
        if base.remove(chest, slot, 1) != 1 {
            job.status = "could not take the bait".into();
            break;
        }
        let holds = |c: u32| base.slots(c).iter().any(|s| s.item == catch && s.count != 0);
        let (mut order, rest): (Vec<u32>, Vec<u32>) = chests.iter().partition(|&&c| holds(c));
        order.extend(rest);
        if !order.iter().any(|&c| base.give(c, catch, 1) == 1) {
            if base.give(chest, bait, 1) != 1 {
                log!("assistants: no room for the fish, and the bait {bait} could not go back to {chest}");
            }
            job.status = "no room in the chests".into();
            break;
        }
        log!("assistants: the fisher caught item {catch} with bait {bait}");
        job.report(catch, 1);
        caught += 1;
        job.done += 1;
        job.progress -= SECONDS_PER_CATCH;
        job.status = "fishing".into();
    }
    caught
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::fake::FakeBase;

    const GARDENER: u32 = 1;
    const CHEST: u32 = 60;
    const CROP: u32 = 80;
    const TREE: u32 = 81;

    #[test]
    fn the_gardener_harvests_crops_and_trees_into_the_chests() {
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[]);
        base.chest(CROP, 8, &[(500, 3)]);
        base.chest(TREE, 8, &[(501, 4)]);
        base.harvest = vec![(CROP, Harvest::Crop), (TREE, Harvest::Refill)];
        let mut job = Job::new(7, Kind::Harvest);
        assert_eq!(harvest(&base, GARDENER, &mut job), 7);
        assert_eq!((base.count(CHEST, 500), base.count(CHEST, 501)), (3, 4));
        assert_eq!(base.finished.borrow().as_slice(), &[CROP], "only the crop goes, the tree stays");
        assert_eq!(job.done, 7);
        assert_eq!(job.status, "harvested 7");
        assert_eq!(job.news, vec![(500, 3), (501, 4)], "what came in, for the notification");
    }

    const SEEDLING: u32 = 600;

    #[test]
    fn a_seedling_from_the_chests_replants_the_crop() {
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(SEEDLING, 2)]);
        base.chest(CROP, 8, &[(500, 3)]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert_eq!(base.replanted.borrow().as_slice(), &[CROP]);
        assert!(base.finished.borrow().is_empty(), "a replanted crop is not removed");
        assert_eq!((base.count(CHEST, SEEDLING), base.count(CHEST, 500), job.replanted), (1, 3, 1));
    }

    #[test]
    fn an_empty_feeder_gets_food_from_the_chests_or_a_batch_made_of_ingredients() {
        let feeding = crate::feeding::data();
        let (goat_feeder, yak_feeder) = (761665893, 3583405641);
        let goat_food = feeding.food_of(goat_feeder).unwrap();
        let yak_food = feeding.food_of(yak_feeder).unwrap();
        let mut base = FakeBase::new(vec![]);
        base.max_stack = 100;
        // goat food made by a player already; for the yak only the ingredients
        let yak: Vec<(u32, u32)> = feeding.recipe(yak_food).unwrap().inputs.iter().map(|i| match *i {
            crate::recipes::Input::Item { item, count } => (item, count),
            crate::recipes::Input::Category { .. } => unreachable!(),
        }).collect();
        let mut stock = vec![(goat_food, 30)];
        stock.extend(yak.iter().copied());
        base.chest(CHEST, 48, &stock);
        base.chest(500, 8, &[]);
        base.chest(501, 8, &[]);
        base.feeders = vec![(500, goat_feeder), (501, yak_feeder)];
        let mut job = Job::new(7, Kind::Feed);
        feed(&base, GARDENER, &mut job);
        assert_eq!(base.count(500, goat_food), 30, "the food in the chests");
        assert_eq!(base.count(501, yak_food), 50, "a batch made");
        assert!(yak.iter().all(|&(i, _)| base.count(CHEST, i) == 0), "the ingredients were used");
        // something that is no food for it goes back to the chests, then the food goes in
        base.chest(503, 8, &[(SEEDLING, 4)]);
        base.feeders.push((503, goat_feeder));
        base.chest(CHEST + 1, 48, &[(goat_food, 10)]);
        feed(&base, GARDENER, &mut job);
        assert_eq!((base.count(503, SEEDLING), base.count(503, goat_food)), (0, 10));
        base.feeders.pop();
        // full feeders are left alone; without ingredients the status says what is missing
        feed(&base, GARDENER, &mut job);
        assert_eq!(base.count(501, yak_food), 50);
        base.chest(502, 8, &[]);
        base.feeders.push((502, yak_feeder));
        feed(&base, GARDENER, &mut job);
        assert!(job.status.starts_with("needs "), "{}", job.status);
    }

    #[test]
    fn the_fisher_takes_the_bait_of_his_order_first() {
        let (worm, other) = (700318057, 1313427371);
        let base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(worm, 3), (other, 2)]);
        let chests = [CHEST];
        assert_eq!(find_bait(&base, &chests, &[]).map(|b| b.2), Some(bait_order(&[]).into_iter().find(|b| *b == worm || *b == other).unwrap()), "without an order: the first by name");
        assert_eq!(find_bait(&base, &chests, &[other]).map(|b| b.2), Some(other));
        assert_eq!(find_bait(&base, &chests, &[999, worm]).map(|b| b.2), Some(worm), "unknown items in the order are skipped");
        let order = bait_order(&[other]);
        assert_eq!((order[0], order.len()), (other, fishing::data().baits.len()));
    }

    #[test]
    fn a_round_remembers_the_field_size_and_the_stock_is_topped_up() {
        let mut base = FakeBase::new(vec![]);
        // two crops ripe, one seedling: one replanted, one left without
        base.chest(CHEST, 48, &[(SEEDLING, 1)]);
        base.chest(CROP, 8, &[(500, 3)]);
        base.chest(CROP + 1, 8, &[(500, 3)]);
        base.harvest = vec![(CROP, Harvest::Crop), (CROP + 1, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        base.seedlings.insert(CROP + 1, SEEDLING);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert_eq!(job.peaks, vec![(SEEDLING, 2)]);
        // a smaller round keeps the peak
        job.peaks.push((999, 1));
        assert_eq!(seedlings_short(&job.peaks, |item| if item == SEEDLING { 3 } else { 10 }), vec![(SEEDLING, 2 + SEEDLING_RESERVE - 3)]);
    }

    #[test]
    fn a_bean_is_picked_into_the_chests_once_and_replanted() {
        const BEAN: u32 = 650;
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(SEEDLING, 2)]);
        base.chest(CROP, 8, &[]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        base.loot.insert(CROP, vec![(BEAN, 3)]);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert_eq!(base.count(CHEST, BEAN), 3);
        assert_eq!(base.replanted.borrow().as_slice(), &[CROP]);
        // marked picked: the next round gives nothing more
        harvest(&base, GARDENER, &mut job);
        assert_eq!(base.count(CHEST, BEAN), 3);
    }

    #[test]
    fn a_bean_picked_before_is_replanted_once_a_seedling_is_there() {
        const BEAN: u32 = 650;
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(SEEDLING, 1)]);
        base.chest(CROP, 8, &[]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        base.loot.insert(CROP, vec![(BEAN, 3)]);
        base.looted.borrow_mut().push(CROP);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert_eq!(base.count(CHEST, BEAN), 0, "no second harvest");
        assert_eq!(base.replanted.borrow().as_slice(), &[CROP]);
    }

    #[test]
    fn a_crop_that_cannot_start_over_is_left_standing_and_keeps_its_seedling() {
        // replanted already, the game has not swapped the stage yet (seen on beans)
        const BEAN: u32 = 650;
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(SEEDLING, 1)]);
        base.chest(CROP, 8, &[]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        base.loot.insert(CROP, vec![(BEAN, 3)]);
        base.looted.borrow_mut().push(CROP);
        base.unplantable.push(CROP);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert!(base.finished.borrow().is_empty(), "never marked for removal");
        assert_eq!(base.count(CHEST, SEEDLING), 1);
    }

    #[test]
    fn the_gardener_works_among_the_growing_crops() {
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[]);
        base.chest(TREE, 8, &[(501, 4)]);
        base.harvest = vec![(TREE, Harvest::Refill)];
        base.positions.insert(TREE, [100.0, 0.0, 100.0]);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert_eq!(job.site, Some([100.0, 0.0, 100.0]), "no crops: at the tree");
        base.growing = vec![[10.0, 0.0, 10.0], [12.0, 0.0, 10.0]];
        harvest(&base, GARDENER, &mut job);
        assert_eq!(job.site, Some([11.0, 0.0, 10.0]), "growing crops come before trees");
    }

    #[test]
    fn an_empty_crop_stage_is_left_alone() {
        // a seedling stage: no harvest to take, so nothing is finished or replanted
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(SEEDLING, 2)]);
        base.chest(CROP, 8, &[]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        let mut job = Job::new(7, Kind::Harvest);
        for _ in 0..3 {
            harvest(&base, GARDENER, &mut job);
        }
        assert!(base.finished.borrow().is_empty() && base.replanted.borrow().is_empty());
        assert_eq!(base.count(CHEST, SEEDLING), 2);
    }

    #[test]
    fn without_a_seedling_the_crop_stays_ripe_on_its_field() {
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[]);
        base.chest(CROP, 8, &[(500, 3)]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert!(base.finished.borrow().is_empty(), "not removed");
        assert_eq!((base.count(CROP, 500), base.count(CHEST, 500)), (3, 0), "not picked");
        assert_eq!(job.peaks, vec![(SEEDLING, 1)], "the seedling it waits for is restocked");
        assert!(job.status.contains("wait for"), "{}", job.status);
    }

    const SIMPLE_BAIT: u32 = 3151503945;
    /// what the simple bait attracts
    const COMMON_FISH: [u32; 2] = [3480216747, 2004880509];

    #[test]
    fn the_fisher_spends_one_bait_per_catch() {
        let base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[(SIMPLE_BAIT, 2)]);
        let mut job = Job::new(FISHER, Kind::Fish);
        let mut dice = Dice::new(7);
        assert_eq!(fish(&base, 1, 30, &mut job, &mut dice), 0, "half a minute is no catch yet");
        assert_eq!(fish(&base, 1, 300, &mut job, &mut dice), 2, "two baits, two fish");
        assert_eq!(job.news.iter().map(|n| n.1).sum::<u32>(), 2);
        assert_eq!((base.count(CHEST, SIMPLE_BAIT), COMMON_FISH.map(|f| base.count(CHEST, f)).iter().sum::<u32>()), (0, 2));
        assert_eq!(fish(&base, 1, 60, &mut job, &mut dice), 0);
        assert_eq!(job.status, "no bait in the chests");
        assert_eq!(job.done, 2);
    }

    #[test]
    fn a_full_chest_gives_the_bait_back() {
        let base = FakeBase::new(vec![]);
        base.chest(CHEST, 1, &[(SIMPLE_BAIT, 5)]);
        let mut job = Job::new(FISHER, Kind::Fish);
        assert_eq!(fish(&base, 1, 60, &mut job, &mut Dice::new(1)), 0);
        assert_eq!(base.count(CHEST, SIMPLE_BAIT), 5);
        assert_eq!(job.status, "no room in the chests");
    }
}
