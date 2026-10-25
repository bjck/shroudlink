//! Survivors walk to their work. A survivor wanders around a home point
//! (`ecs::ActiveNpcStateSaved`: spawnPosition, overrideRoamingRange, roamingRange), so while
//! they work the home point moves to the work site with a small range, and the game's own
//! AI walks them there and keeps them pottering about it. When the work stops, the home
//! point they had comes back. Moved home points are kept in the order file, so a restart
//! cannot strand anyone at a work site.

use shroudlink_sdk::log;

use crate::base::{Base, Home};

/// How far around the work site they wander, in metres.
pub const RANGE: f32 = 3.0;
/// A site that moved less than this is not worth a new home point.
const SAME_SITE: f32 = 1.5;

/// A survivor whose home point was moved.
#[derive(Clone, Debug, PartialEq)]
pub struct Moved {
    pub workshop: u32,
    /// the survivor's entity this session (0 after a restart, until found again)
    pub npc: u32,
    /// the home point to give back
    pub original: Home,
    /// where the home point is now
    pub site: [f32; 3],
}

/// What a survivor's work asks of their home point.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Want {
    /// work at this site
    Go([f32; 3]),
    /// working, but without a site right now: leave the home point as it is
    Stay,
    /// not working: give the home point back
    Free,
}

impl Want {
    /// The more specific of two wishes: a site beats staying beats being free.
    pub fn or(self, other: Want) -> Want {
        match (self, other) {
            (Want::Go(_), _) => self,
            (_, Want::Go(_)) => other,
            (Want::Stay, _) | (_, Want::Stay) => Want::Stay,
            _ => Want::Free,
        }
    }
}

/// The middle of some positions, as a work site.
pub fn site_of(points: &[[f64; 3]]) -> Option<[f32; 3]> {
    if points.is_empty() {
        return None;
    }
    let n = points.len() as f64;
    let mut c = [0.0; 3];
    for p in points {
        for i in 0..3 {
            c[i] += p[i] / n;
        }
    }
    Some(c.map(|v| v as f32))
}

fn near(a: [f32; 3], b: [f32; 3], d: f32) -> bool {
    (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>() < d * d
}

/// Sends a workshop's people to their work: the assistant if one is there (the survivor
/// keeps their home point), otherwise the survivor. Returns whether `moved` changed.
pub fn send(base: &dyn Base, workshop: u32, npcs: &[u32], want: Want, moved: &mut Vec<Moved>) -> bool {
    let workers = workers(base, npcs);
    let others: Vec<u32> = npcs.iter().copied().filter(|n| !workers.contains(n)).collect();
    let a = apply(base, workshop, &workers, want, moved);
    let b = apply(base, workshop, &others, Want::Free, moved);
    a || b
}

/// Who of a workshop's people does its work: the assistants, or the survivor without one.
pub fn workers(base: &dyn Base, npcs: &[u32]) -> Vec<u32> {
    let (assistants, survivors): (Vec<u32>, Vec<u32>) = npcs.iter().partition(|&&n| base.is_assistant(n));
    if assistants.is_empty() { survivors } else { assistants }
}

/// Moves or gives back the home points of some of a workshop's people. Returns whether
/// `moved` changed.
pub fn apply(base: &dyn Base, workshop: u32, npcs: &[u32], want: Want, moved: &mut Vec<Moved>) -> bool {
    let mut changed = false;
    for &npc in npcs {
        let Some(home) = base.npc_home(npc) else { continue };
        // the record of this survivor: by entity, or after a restart (new entity ids) by
        // workshop and a home point near where we put it (the game may nudge it a little
        // onto walkable ground)
        let record = moved.iter().position(|m| m.npc == npc).or_else(|| moved.iter().position(|m| m.npc == 0 && m.workshop == workshop && near(m.site, home.pos, 6.0)));
        if let Some(i) = record {
            moved[i].npc = npc;
        }
        match (want, record) {
            (Want::Go(site), Some(i)) => {
                if !near(moved[i].site, site, SAME_SITE) && base.set_npc_home(npc, Home { pos: site, override_range: true, range: RANGE }) {
                    moved[i].site = site;
                    base.nudge_npc(npc);
                    changed = true;
                }
            }
            (Want::Go(site), None) => {
                if base.set_npc_home(npc, Home { pos: site, override_range: true, range: RANGE }) {
                    base.nudge_npc(npc);
                    log!("assistants: survivor {npc} walks to their work at {site:?}");
                    moved.push(Moved { workshop, npc, original: home, site });
                    changed = true;
                }
            }
            (Want::Free, Some(i)) => {
                if base.set_npc_home(npc, moved[i].original) {
                    base.nudge_npc(npc);
                    log!("assistants: survivor {npc} goes home to {:?}", moved[i].original.pos);
                    moved.remove(i);
                    changed = true;
                }
            }
            (Want::Stay, _) | (Want::Free, None) => {}
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::work::fake::FakeBase;

    const FARMER: u32 = 9;
    const NPC: u32 = 1;

    fn base() -> FakeBase {
        let base = FakeBase::new(vec![(NPC, FARMER)]);
        base.homes.borrow_mut().insert(NPC, Home { pos: [10.0, 0.0, 10.0], override_range: false, range: 30.0 });
        base
    }

    #[test]
    fn walks_to_the_work_and_back_home() {
        let base = base();
        let mut moved = Vec::new();
        assert!(apply(&base, FARMER, &[NPC], Want::Go([50.0, 0.0, 50.0]), &mut moved));
        assert_eq!(base.homes.borrow()[&NPC], Home { pos: [50.0, 0.0, 50.0], override_range: true, range: RANGE });
        // the same site again changes nothing, a new one moves the home point
        assert!(!apply(&base, FARMER, &[NPC], Want::Go([50.5, 0.0, 50.0]), &mut moved));
        assert!(apply(&base, FARMER, &[NPC], Want::Go([70.0, 0.0, 50.0]), &mut moved));
        assert!(!apply(&base, FARMER, &[NPC], Want::Stay, &mut moved));
        assert_eq!(base.homes.borrow()[&NPC].pos, [70.0, 0.0, 50.0]);
        // done: the home point they had comes back
        assert!(apply(&base, FARMER, &[NPC], Want::Free, &mut moved));
        assert_eq!(base.homes.borrow()[&NPC], Home { pos: [10.0, 0.0, 10.0], override_range: false, range: 30.0 });
        assert!(moved.is_empty());
    }

    #[test]
    fn the_assistant_walks_and_the_survivor_stays() {
        let mut base = base();
        const ASSISTANT: u32 = 2;
        base.homes.borrow_mut().insert(ASSISTANT, Home { pos: [20.0, 0.0, 20.0], override_range: false, range: 30.0 });
        base.assistants = vec![ASSISTANT];
        let mut moved = Vec::new();
        send(&base, FARMER, &[NPC, ASSISTANT], Want::Go([50.0, 0.0, 50.0]), &mut moved);
        assert_eq!(base.homes.borrow()[&ASSISTANT].pos, [50.0, 0.0, 50.0]);
        assert_eq!(base.homes.borrow()[&NPC].pos, [10.0, 0.0, 10.0], "the survivor stays home");
        // without an assistant the survivor goes
        base.assistants.clear();
        send(&base, FARMER, &[NPC], Want::Go([50.0, 0.0, 50.0]), &mut moved);
        assert_eq!(base.homes.borrow()[&NPC].pos, [50.0, 0.0, 50.0]);
    }

    #[test]
    fn a_home_point_the_game_nudged_still_goes_back_to_the_original() {
        let base = base();
        let mut moved = Vec::new();
        apply(&base, FARMER, &[NPC], Want::Go([50.0, 0.0, 50.0]), &mut moved);
        // the game moves the home point a little onto walkable ground
        base.homes.borrow_mut().get_mut(&NPC).unwrap().pos = [52.5, 0.0, 47.0];
        apply(&base, FARMER, &[NPC], Want::Go([80.0, 0.0, 80.0]), &mut moved);
        assert_eq!(moved.len(), 1, "no second record with the nudged point as the original");
        apply(&base, FARMER, &[NPC], Want::Free, &mut moved);
        assert_eq!(base.homes.borrow()[&NPC].pos, [10.0, 0.0, 10.0]);
        // after a restart the entity is unknown: found again by workshop and site
        apply(&base, FARMER, &[NPC], Want::Go([50.0, 0.0, 50.0]), &mut moved);
        moved[0].npc = 0;
        base.homes.borrow_mut().get_mut(&NPC).unwrap().pos = [51.0, 0.0, 51.0];
        apply(&base, FARMER, &[NPC], Want::Free, &mut moved);
        assert_eq!(base.homes.borrow()[&NPC].pos, [10.0, 0.0, 10.0]);
    }

    #[test]
    fn the_assistant_works_if_there_is_one() {
        let mut base = base();
        assert_eq!(workers(&base, &[NPC]), vec![NPC]);
        base.assistants = vec![2];
        assert_eq!(workers(&base, &[NPC, 2]), vec![2]);
    }

    #[test]
    fn a_site_beats_staying_beats_being_free() {
        let site = Want::Go([1.0, 2.0, 3.0]);
        assert_eq!(Want::Free.or(site), site);
        assert_eq!(Want::Stay.or(Want::Free), Want::Stay);
        assert_eq!(site.or(Want::Go([9.0, 9.0, 9.0])), site);
        assert_eq!(site_of(&[[0.0, 0.0, 0.0], [2.0, 4.0, 6.0]]), Some([1.0, 2.0, 3.0]));
    }
}
