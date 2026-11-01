//! Ascension progress of one account and the settings that shape it: pure logic, no game.

use serde_json::{json, Value};

/// The six primary attributes points can go to, in the order of the game's character screen.
pub const STATS: [(&str, &str); 6] = [
    ("con", "Constitution"),
    ("spi", "Spirit"),
    ("end", "Endurance"),
    ("str", "Strength"),
    ("dex", "Dexterity"),
    ("int", "Intelligence"),
];

/// A Paragon skill: ranks bought with Paragon points, each adding its amount to every
/// slot in `slots` (attribute slot name, amount per rank; `shroudlink_sdk::attr`). Amounts
/// are added on top of what the game, buffs and gear put there.
pub struct Skill {
    pub key: &'static str,
    pub name: &'static str,
    pub slots: &'static [(&'static str, f64)],
    pub max_rank: u32,
    /// what one rank does, for the page
    pub effect: &'static str,
    /// tested in game: the player notices it
    pub verified: bool,
    /// the game skill whose icon the node in the Paragon tree borrows (0: a small node)
    pub icon: u32,
}

/// The Paragon skills. Movement speed is measured in game (Movement_Run 5.0 m/s and
/// Movement_Sprint 8.4 m/s for a player; Movement_Mod has no effect on players). Stamina
/// shows on the character screen. The others write their slots on the server; whether the
/// client uses them (gliding and casting are computed on the client) is not tested yet.
pub const SKILLS: [Skill; 5] = [
    Skill { key: "mining", name: "Mining", slots: &[("ToolDamageMod_Wood", 0.05), ("ToolDamageMod_Stone", 0.05), ("ToolDamageMod_Metal", 0.05)], max_rank: 20, effect: "+5 % tool damage on wood, stone and metal", verified: false, icon: 0xfa13_85f4 }, // Pickaxe Specialization
    Skill { key: "glide", name: "Glider endurance", slots: &[("StaminaDepletionMod_Glider", -0.03)], max_rank: 20, effect: "3 % less stamina used while gliding", verified: false, icon: 0xb81e_3dcc }, // Updraft
    Skill { key: "stamina", name: "Stamina", slots: &[("Stamina_Max_Adder", 10.0)], max_rank: 30, effect: "+10 maximum stamina", verified: true, icon: 0xf2a8_b9be }, // Athlete
    Skill { key: "speed", name: "Movement speed", slots: &[("Movement_Run", 0.1), ("Movement_Sprint", 0.17)], max_rank: 15, effect: "+2 % running and sprinting speed", verified: true, icon: 0xc4f9_ee05 }, // Splash Dash
    Skill { key: "cast", name: "Cast speed", slots: &[("RangedCombat_Spell_ChargeDurationMod", -0.03), ("RangedCombat_Spell_FireDurationMod", -0.03)], max_rank: 15, effect: "3 % faster spell charge and cast", verified: false, icon: 0x4826_aafc }, // Wand Specialization
];

/// The `X_Base` slot of each of [`STATS`]: what skill nodes raise.
pub const BASE_SLOTS: [&str; 6] = ["Constitution_Base", "Spirit_Base", "Endurance_Base", "Strength_Base", "Dexterity_Base", "Intelligence_Base"];

/// What the server owner can tune (`settings` in the file).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Ascension XP per game XP
    pub xp_multiplier: f64,
    /// attribute points per Ascension level
    pub points_per_level: u32,
    /// Paragon points per Ascension level
    pub paragon_per_level: u32,
    /// points one attribute can take (the client shows primary attributes as one byte)
    pub max_points_per_attribute: u32,
    /// deepest Shroud Depth (until Breaches unlock depths, Ascension level N unlocks depth N)
    pub max_depth: u32,
    /// enemy levels added per Torment level above 1
    pub levels_per_depth: u32,
    /// the level every hostile enemy has at least from Torment 1
    pub torment_min_level: f64,
    /// extra Ascension XP per depth (0.1: +10 %)
    pub depth_xp_bonus: f64,
    /// metres around a player in which enemies are raised
    pub depth_radius: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { xp_multiplier: 1.0, points_per_level: 1, paragon_per_level: 1, max_points_per_attribute: 100, max_depth: 20, levels_per_depth: 3, torment_min_level: 50.0, depth_xp_bonus: 0.1, depth_radius: 80.0 }
    }
}

impl Settings {
    pub fn from_json(v: &Value) -> Settings {
        let d = Settings::default();
        Settings {
            xp_multiplier: v["xp_multiplier"].as_f64().filter(|m| m.is_finite() && *m >= 0.0).unwrap_or(d.xp_multiplier),
            points_per_level: v["points_per_level"].as_u64().map_or(d.points_per_level, |p| p.min(100) as u32),
            paragon_per_level: v["paragon_per_level"].as_u64().map_or(d.paragon_per_level, |p| p.min(100) as u32),
            max_points_per_attribute: v["max_points_per_attribute"].as_u64().map_or(d.max_points_per_attribute, |p| p.min(200) as u32),
            max_depth: v["max_depth"].as_u64().map_or(d.max_depth, |p| p.min(100) as u32),
            levels_per_depth: v["levels_per_depth"].as_u64().map_or(d.levels_per_depth, |p| p.min(50) as u32),
            torment_min_level: v["torment_min_level"].as_f64().filter(|l| l.is_finite() && *l >= 0.0).map_or(d.torment_min_level, |l| l.min(255.0)),
            depth_xp_bonus: v["depth_xp_bonus"].as_f64().filter(|b| b.is_finite() && *b >= 0.0).unwrap_or(d.depth_xp_bonus),
            depth_radius: v["depth_radius"].as_f64().filter(|r| r.is_finite() && *r > 0.0).map_or(d.depth_radius, |r| r.min(300.0)),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "xp_multiplier": self.xp_multiplier,
            "points_per_level": self.points_per_level,
            "paragon_per_level": self.paragon_per_level,
            "max_points_per_attribute": self.max_points_per_attribute,
            "max_depth": self.max_depth,
            "levels_per_depth": self.levels_per_depth,
            "torment_min_level": self.torment_min_level,
            "depth_xp_bonus": self.depth_xp_bonus,
            "depth_radius": self.depth_radius,
        })
    }
}

/// XP to leave game level `level` (`FUN_140934910`): a table up to level 5, then
/// `round((50 + 150 L / 100) * 50 * 1.5 + 2562)` (level 50: 11937, as the game shows).
pub fn xp_required(level: u32) -> u64 {
    match level {
        0 | 1 => 1000,
        2 => 2023,
        3 => 3046,
        4 => 4069,
        5 => 5092,
        l => ((50.0 + 150.0 * l as f64 / 100.0) * 50.0 * 1.5 + 2562.0).round() as u64,
    }
}

/// XP to reach Ascension `n + 1` from `n`: the game's curve continued past the cap.
pub fn ascension_required(n: u32) -> u64 {
    xp_required(50 + n + 1)
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub level: u32,
    /// XP into the current Ascension level
    pub xp: u64,
    /// points on each of [`STATS`]
    pub points: [u32; 6],
    /// the Shroud Depth the player chose (0: vanilla)
    pub depth: u32,
    /// ranks of each of [`SKILLS`]
    pub paragon: [u32; SKILLS.len()],
}

impl Account {
    pub fn from_json(v: &Value) -> Account {
        let mut points = [0u32; 6];
        for (i, (key, _)) in STATS.iter().enumerate() {
            points[i] = v["points"][key].as_u64().unwrap_or(0) as u32;
        }
        let mut paragon = [0u32; SKILLS.len()];
        for (i, skill) in SKILLS.iter().enumerate() {
            paragon[i] = v["paragon"][skill.key].as_u64().unwrap_or(0) as u32;
        }
        Account { level: v["level"].as_u64().unwrap_or(0) as u32, xp: v["xp"].as_u64().unwrap_or(0), points, depth: v["depth"].as_u64().unwrap_or(0) as u32, paragon }
    }

    pub fn to_json(&self) -> Value {
        let points: serde_json::Map<String, Value> = STATS.iter().zip(self.points).map(|((k, _), p)| (k.to_string(), json!(p))).collect();
        let paragon: serde_json::Map<String, Value> = SKILLS.iter().zip(self.paragon).map(|(s, r)| (s.key.to_string(), json!(r))).collect();
        json!({ "level": self.level, "xp": self.xp, "points": points, "depth": self.depth, "paragon": paragon })
    }

    /// Adds Ascension XP; returns how many levels were gained.
    pub fn add_xp(&mut self, xp: u64) -> u32 {
        self.xp = self.xp.saturating_add(xp);
        let mut gained = 0;
        while self.xp >= ascension_required(self.level) {
            self.xp -= ascension_required(self.level);
            self.level += 1;
            gained += 1;
        }
        gained
    }

    pub fn spent(&self) -> u32 {
        self.points.iter().sum()
    }

    pub fn unspent(&self, settings: &Settings) -> u32 {
        (self.level * settings.points_per_level).saturating_sub(self.spent())
    }

    /// Puts one point on stat `i`; false if none is left or the stat is full.
    pub fn spend(&mut self, i: usize, settings: &Settings) -> bool {
        if i >= 6 || self.unspent(settings) == 0 || self.points[i] >= settings.max_points_per_attribute {
            return false;
        }
        self.points[i] += 1;
        true
    }

    pub fn paragon_unspent(&self, settings: &Settings) -> u32 {
        (self.level * settings.paragon_per_level).saturating_sub(self.paragon.iter().sum())
    }

    /// Buys one rank of skill `i`; false if no point is left or the skill is at its maximum.
    pub fn learn(&mut self, i: usize, settings: &Settings) -> bool {
        if i >= SKILLS.len() || self.paragon_unspent(settings) == 0 || self.paragon[i] >= SKILLS[i].max_rank {
            return false;
        }
        self.paragon[i] += 1;
        true
    }

    /// Takes every Paragon rank back (free).
    pub fn reset_paragon(&mut self) {
        self.paragon = [0; SKILLS.len()];
    }

    /// What this account adds to each slot (by name): attribute points and Paragon ranks.
    pub fn bonuses(&self) -> Vec<(&'static str, f64)> {
        let mut out: Vec<(&'static str, f64)> = Vec::new();
        for (slot, &points) in BASE_SLOTS.iter().zip(&self.points) {
            out.push((slot, points as f64));
        }
        for (skill, &rank) in SKILLS.iter().zip(&self.paragon) {
            for &(slot, per_rank) in skill.slots {
                out.push((slot, per_rank * rank as f64));
            }
        }
        out
    }

    /// The deepest depth this account may choose.
    pub fn unlocked_depth(&self, settings: &Settings) -> u32 {
        self.level.min(settings.max_depth)
    }

    /// The chosen depth, never above what is unlocked (settings may have changed).
    pub fn depth(&self, settings: &Settings) -> u32 {
        self.depth.min(self.unlocked_depth(settings))
    }

    /// Chooses a depth (clamped to the unlocked ones); returns the depth now chosen.
    pub fn choose_depth(&mut self, depth: u32, settings: &Settings) -> u32 {
        self.depth = depth.min(self.unlocked_depth(settings));
        self.depth
    }

    /// Ascension XP for `xp` game XP at the chosen depth.
    pub fn ascension_xp(&self, xp: u32, settings: &Settings) -> u64 {
        (xp as f64 * settings.xp_multiplier * (1.0 + settings.depth_xp_bonus * self.depth(settings) as f64)).round() as u64
    }

    /// Takes every point back (free).
    pub fn reset(&mut self) {
        self.points = [0; 6];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curve_matches_the_game() {
        assert_eq!(xp_required(49), 11825);
        assert_eq!(xp_required(50), 11937);
        assert_eq!(ascension_required(0), 12050);
    }

    #[test]
    fn xp_levels_up_and_carries_over() {
        let mut a = Account::default();
        assert_eq!(a.add_xp(12_000), 0);
        assert_eq!(a.add_xp(100), 1);
        assert_eq!((a.level, a.xp), (1, 50));
        assert_eq!(a.add_xp(ascension_required(1) + ascension_required(2)), 2);
        assert_eq!((a.level, a.xp), (3, 50));
    }

    #[test]
    fn points_are_limited() {
        let s = Settings { max_points_per_attribute: 2, ..Settings::default() };
        let mut a = Account { level: 3, ..Account::default() };
        assert!(a.spend(0, &s) && a.spend(0, &s));
        assert!(!a.spend(0, &s), "full");
        assert!(a.spend(3, &s));
        assert!(!a.spend(4, &s), "none left");
        assert_eq!(a.unspent(&s), 0);
        a.reset();
        assert_eq!(a.unspent(&s), 3);
    }

    #[test]
    fn depths_follow_ascension() {
        let s = Settings::default();
        let mut a = Account { level: 3, ..Account::default() };
        assert_eq!(a.choose_depth(10, &s), 3);
        assert_eq!(a.ascension_xp(100, &s), 130);
        a.level = 1;
        assert_eq!(a.depth(&s), 1, "a lower unlock caps the chosen depth");
        assert_eq!(a.choose_depth(0, &s), 0);
        assert_eq!(a.ascension_xp(100, &s), 100);
    }

    #[test]
    fn paragon_ranks() {
        let s = Settings::default();
        let mut a = Account { level: 2, ..Account::default() };
        assert!(a.learn(0, &s) && a.learn(0, &s));
        assert!(!a.learn(1, &s), "no point left");
        let mining: f64 = a.bonuses().iter().filter(|b| b.0 == "ToolDamageMod_Stone").map(|b| b.1).sum();
        assert!((mining - 0.1).abs() < 1e-9);
        a.reset_paragon();
        assert_eq!(a.paragon_unspent(&s), 2);
        for skill in &SKILLS {
            for (slot, _) in skill.slots {
                assert!(shroudlink_sdk::attr::id_of(slot).is_some(), "{slot} is an attribute");
            }
        }
        for slot in BASE_SLOTS {
            assert!(shroudlink_sdk::attr::id_of(slot).is_some(), "{slot} is an attribute");
        }
    }

    #[test]
    fn json_round_trip() {
        let a = Account { level: 4, xp: 77, points: [1, 0, 0, 2, 0, 1], depth: 2, paragon: [1, 0, 2, 0, 0] };
        assert_eq!(Account::from_json(&a.to_json()), a);
        let s = Settings { xp_multiplier: 2.0, points_per_level: 2, paragon_per_level: 3, max_points_per_attribute: 50, max_depth: 5, levels_per_depth: 4, torment_min_level: 40.0, depth_xp_bonus: 0.2, depth_radius: 60.0 };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json(&json!({})), Settings::default());
    }
}
