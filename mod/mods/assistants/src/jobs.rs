//! Standing jobs, switched on per survivor in the window and kept until switched off:
//!
//! - the **gardener** (the Farmer or the Farmer's assistant) harvests what grows around them
//!   into the chests: ripe crops, which they replant with a seedling from the chests (the
//!   plant starts over from its first stage; without a seedling the game removes it, as
//!   after a player's harvest), and the fruit of planted trees and bushes and the produce
//!   of tame animals, which refill on their own;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Harvest,
    Fish,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Harvest => "harvest",
            Kind::Fish => "fish",
        }
    }
    pub fn from_name(name: &str) -> Option<Kind> {
        match name {
            "harvest" => Some(Kind::Harvest),
            "fish" => Some(Kind::Fish),
            _ => None,
        }
    }
}

/// The jobs the people of a workshop can do.
pub fn kinds_for(workshop: u32) -> Vec<Kind> {
    let mut kinds = Vec::new();
    if workshop == FARMER_WORKSHOP {
        kinds.push(Kind::Harvest);
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
        Job { npc, kind, done: 0, progress: 0, status: "waiting until someone is at the base".into(), replanted: 0, spot: None, site: None, news: Vec::new(), peaks: Vec::new() }
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
        // a crop is finished (or replanted) only right after its harvest was taken
        if kind != Harvest::Crop || from_plant == 0 || base.slots(plant).iter().any(|s| s.count != 0) {
            continue;
        }
        match replant(base, plant, &chests) {
            Ok(seedling) => {
                want(seedling);
                job.replanted += 1;
                log!("assistants: crop {plant} harvested and replanted with item {seedling}");
            }
            Err(missing) => {
                if let Some(seedling) = missing {
                    want(seedling);
                    no_seedling = missing;
                }
                if base.finish_crop(plant) {
                    log!("assistants: crop {plant} harvested, not replanted");
                }
            }
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
        format!("no {} in the chests to replant", crate::names::item(seedling))
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

/// A bait in the chests: (chest, slot, bait item).
fn find_bait(base: &dyn Base, chests: &[u32]) -> Option<(u32, u64, u32)> {
    let data = fishing::data();
    chests.iter().find_map(|&c| base.slots(c).into_iter().find(|s| s.count != 0 && data.is_bait(s.item)).map(|s| (c, s.id, s.item)))
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
        let Some((chest, slot, bait)) = find_bait(base, &chests) else {
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
    fn without_a_seedling_the_crop_goes_as_in_vanilla() {
        let mut base = FakeBase::new(vec![]);
        base.chest(CHEST, 48, &[]);
        base.chest(CROP, 8, &[(500, 3)]);
        base.harvest = vec![(CROP, Harvest::Crop)];
        base.seedlings.insert(CROP, SEEDLING);
        let mut job = Job::new(7, Kind::Harvest);
        harvest(&base, GARDENER, &mut job);
        assert_eq!(base.finished.borrow().as_slice(), &[CROP]);
        assert!(job.status.starts_with("no "), "{}", job.status);
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
