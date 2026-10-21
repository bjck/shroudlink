//! Bookkeeping of the damage meter, free of the game: hits in, fights, totals and deaths
//! out. Times are seconds since the mod started, so tests can feed any timeline.

use std::collections::{HashMap, VecDeque};

/// A fight ends after this long without a hit involving the player.
pub const FIGHT_GAP: f64 = 10.0;
/// Hits taken kept per player, the killing blow and what came before it.
const RECENT_HITS: usize = 4;
/// Deaths kept per player.
pub const DEATHS_KEPT: usize = 5;

/// One hit, as the server reported it (`keen::ecs::HitEvent`).
#[derive(Clone, Debug, PartialEq)]
pub struct Hit {
    /// who caused it (the player for arrows, spells, summons); 0: nobody, e.g. a fall
    pub source: u32,
    pub target: u32,
    /// health taken (or given, for healing), always positive
    pub amount: u32,
    pub healing: bool,
    pub killing_blow: bool,
    pub crit: bool,
    /// `keen::impact::DamageTypeNew` bits
    pub damage_types: u16,
    /// who the source is, read when the hit arrived (it may be gone later)
    pub attacker: String,
}

/// Totals of one player over some time.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Totals {
    pub damage: u64,
    pub taken: u64,
    pub healing: u64,
    pub healed: u64,
    pub top_hit: u32,
}

impl Totals {
    fn is_empty(&self) -> bool {
        self.damage + self.taken + self.healing + self.healed == 0
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Fight {
    pub start: f64,
    pub last: f64,
    pub totals: Totals,
}

impl Fight {
    /// Seconds from the first to the last hit, at least one.
    pub fn seconds(&self) -> f64 {
        (self.last - self.start).max(1.0)
    }

    pub fn dps(&self) -> f64 {
        self.totals.damage as f64 / self.seconds()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Death {
    pub at: f64,
    /// the killing blow first, then the hits before it
    pub hits: Vec<Hit>,
}

#[derive(Default)]
struct Player {
    session: Totals,
    fight: Option<Fight>,
    last_fight: Option<Fight>,
    recent: VecDeque<(f64, Hit)>,
    deaths: VecDeque<Death>,
}

#[derive(Default)]
pub struct Tally {
    players: HashMap<u32, Player>,
}

impl Tally {
    /// Counts a hit; `is_player` tells players from everything else.
    pub fn hit(&mut self, now: f64, hit: &Hit, is_player: impl Fn(u32) -> bool) {
        let source_is_player = hit.source != 0 && is_player(hit.source);
        let target_is_player = is_player(hit.target);
        // Healing received counts everything that refills health (regeneration, potions,
        // other players); healing done only what a player gives others. Heals neither start
        // nor extend a fight: regeneration ticks every two seconds.
        if hit.healing {
            if source_is_player && hit.source != hit.target {
                self.add(hit.source, now, false, |t| t.healing += hit.amount as u64);
            }
            if target_is_player {
                self.add(hit.target, now, false, |t| t.healed += hit.amount as u64);
            }
            return;
        }
        // damage dealt counts against anything but players (no friendly fire)
        if source_is_player && !target_is_player && hit.amount > 0 {
            self.add(hit.source, now, true, |t| {
                t.damage += hit.amount as u64;
                t.top_hit = t.top_hit.max(hit.amount);
            });
        }
        if target_is_player {
            self.add(hit.target, now, true, |t| t.taken += hit.amount as u64);
            let p = self.players.entry(hit.target).or_default();
            // blocked hits (0 damage) say nothing about the death
            if hit.amount > 0 || hit.killing_blow {
                p.recent.push_front((now, hit.clone()));
                p.recent.truncate(RECENT_HITS);
            }
            if hit.killing_blow {
                self.died(hit.target, now);
            }
        }
    }

    /// The player died (`EntityDiedEvent`); the hits taken just before are the cause.
    /// A killing blow already recorded the death, so a second report within a moment
    /// is ignored.
    pub fn died(&mut self, player: u32, now: f64) {
        let p = self.players.entry(player).or_default();
        if p.deaths.front().is_some_and(|d| now - d.at < 2.0) {
            return;
        }
        let hits: Vec<Hit> = p.recent.iter().filter(|(at, _)| now - at <= FIGHT_GAP).map(|(_, h)| h.clone()).collect();
        p.recent.clear();
        p.deaths.push_front(Death { at: now, hits });
        p.deaths.truncate(DEATHS_KEPT);
    }

    /// Adds to the session and, for combat (`fighting`), to the current fight, starting
    /// one if needed; anything else only joins a fight that is going on.
    fn add(&mut self, player: u32, now: f64, fighting: bool, f: impl Fn(&mut Totals)) {
        let p = self.players.entry(player).or_default();
        f(&mut p.session);
        if fighting && p.fight.is_none() {
            p.fight = Some(Fight { start: now, last: now, totals: Totals::default() });
        }
        if let Some(fight) = p.fight.as_mut() {
            f(&mut fight.totals);
            if fighting {
                fight.last = now;
            }
        }
    }

    /// Fights that are over (no hit for [`FIGHT_GAP`]), per player.
    pub fn finished(&mut self, now: f64) -> Vec<(u32, Fight)> {
        let mut out = Vec::new();
        for (&id, p) in self.players.iter_mut() {
            if p.fight.as_ref().is_some_and(|f| now - f.last >= FIGHT_GAP) {
                let fight = p.fight.take().unwrap();
                if !fight.totals.is_empty() {
                    p.last_fight = Some(fight.clone());
                    out.push((id, fight));
                }
            }
        }
        out.sort_by_key(|(id, _)| *id);
        out
    }

    /// Players with anything counted, with their session totals and last (or current) fight.
    pub fn table(&self) -> Vec<(u32, Totals, Option<Fight>)> {
        let mut rows: Vec<_> = self.players.iter().filter(|(_, p)| !p.session.is_empty()).map(|(&id, p)| (id, p.session, p.fight.clone().or_else(|| p.last_fight.clone()))).collect();
        rows.sort_by(|a, b| b.1.damage.cmp(&a.1.damage).then(a.0.cmp(&b.0)));
        rows
    }

    pub fn deaths(&self, player: u32) -> Vec<Death> {
        self.players.get(&player).map(|p| p.deaths.iter().cloned().collect()).unwrap_or_default()
    }

    /// Clears the session totals (deaths stay).
    pub fn reset(&mut self) {
        for p in self.players.values_mut() {
            p.session = Totals::default();
            p.fight = None;
            p.last_fight = None;
        }
    }
}

/// Names of `keen::impact::DamageTypeNew` bits.
pub fn damage_type_names(bits: u16) -> Vec<&'static str> {
    const NAMES: [&str; 13] = ["cut", "pierce", "blunt", "fire", "ice", "shroud", "lightning", "healing", "poison", "wood", "stone", "metal", "explosion"];
    NAMES.iter().enumerate().filter(|(i, _)| bits & (1 << i) != 0).map(|(_, n)| *n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PLAYER: u32 = 7;
    const FRIEND: u32 = 8;
    const ENEMY: u32 = 100;

    fn players(id: u32) -> bool {
        id == PLAYER || id == FRIEND
    }

    fn hit(source: u32, target: u32, amount: u32) -> Hit {
        Hit { source, target, amount, healing: false, killing_blow: false, crit: false, damage_types: 0, attacker: format!("entity {source}") }
    }

    #[test]
    fn a_fight_adds_up_and_ends_after_a_quiet_gap() {
        let mut t = Tally::default();
        t.hit(0.0, &hit(PLAYER, ENEMY, 100), players);
        t.hit(4.0, &hit(ENEMY, PLAYER, 30), players);
        t.hit(5.0, &hit(PLAYER, ENEMY, 150), players);
        t.hit(5.0, &Hit { healing: true, ..hit(FRIEND, PLAYER, 20) }, players);
        assert!(t.finished(10.0).is_empty(), "still fighting");
        let done = t.finished(15.0);
        let (id, fight) = done.iter().find(|(id, _)| *id == PLAYER).unwrap();
        assert_eq!(*id, PLAYER);
        assert_eq!(fight.totals, Totals { damage: 250, taken: 30, healing: 0, healed: 20, top_hit: 150 });
        assert_eq!(fight.dps(), 50.0);
        // the healer was not fighting: the heal counts for their session only
        assert!(done.iter().all(|(id, _)| *id != FRIEND));
        assert_eq!(t.table().iter().find(|r| r.0 == FRIEND).unwrap().1.healing, 20);
        assert!(t.finished(30.0).is_empty(), "reported once");
        assert_eq!(t.table()[0].0, PLAYER);
    }

    #[test]
    fn regeneration_is_healing_received_but_no_fight() {
        let mut t = Tally::default();
        let regen = Hit { healing: true, ..hit(PLAYER, PLAYER, 13) };
        t.hit(0.0, &regen, players);
        t.hit(2.0, &regen, players);
        assert!(t.finished(100.0).is_empty(), "no fight from regeneration");
        let rows = t.table();
        assert_eq!(rows[0].1, Totals { healed: 26, ..Totals::default() });
    }

    #[test]
    fn hitting_a_player_is_not_damage_dealt() {
        let mut t = Tally::default();
        t.hit(0.0, &hit(PLAYER, FRIEND, 40), players);
        let rows = t.table();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, FRIEND);
        assert_eq!(rows[0].1.taken, 40);
    }

    #[test]
    fn the_killing_blow_and_the_hits_before_it_are_kept() {
        let mut t = Tally::default();
        t.hit(0.0, &hit(ENEMY, PLAYER, 10), players);
        t.hit(1.0, &hit(ENEMY, PLAYER, 20), players);
        t.hit(2.0, &Hit { killing_blow: true, damage_types: 1 << 3, ..hit(ENEMY + 1, PLAYER, 300) }, players);
        t.died(PLAYER, 2.1); // the death event right after the killing blow
        let deaths = t.deaths(PLAYER);
        assert_eq!(deaths.len(), 1);
        assert_eq!(deaths[0].hits.iter().map(|h| h.amount).collect::<Vec<_>>(), vec![300, 20, 10]);
        assert_eq!(damage_type_names(deaths[0].hits[0].damage_types), vec!["fire"]);
        for i in 0..10 {
            t.died(PLAYER, 100.0 + i as f64 * 10.0);
        }
        assert_eq!(t.deaths(PLAYER).len(), DEATHS_KEPT);
    }

    #[test]
    fn reset_clears_totals_but_not_deaths() {
        let mut t = Tally::default();
        t.hit(0.0, &Hit { killing_blow: true, ..hit(ENEMY, PLAYER, 5) }, players);
        t.reset();
        assert!(t.table().is_empty());
        assert_eq!(t.deaths(PLAYER).len(), 1);
    }
}
