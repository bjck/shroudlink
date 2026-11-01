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

/// What the server owner can tune (`settings` in the file).
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// Ascension XP per game XP
    pub xp_multiplier: f64,
    /// attribute points per Ascension level
    pub points_per_level: u32,
    /// points one attribute can take (the client shows primary attributes as one byte)
    pub max_points_per_attribute: u32,
    /// deepest Shroud Depth (until Breaches unlock depths, Ascension level N unlocks depth N)
    pub max_depth: u32,
    /// enemy levels added per depth
    pub levels_per_depth: u32,
    /// extra Ascension XP per depth (0.1: +10 %)
    pub depth_xp_bonus: f64,
    /// metres around a player in which enemies are raised
    pub depth_radius: f64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings { xp_multiplier: 1.0, points_per_level: 1, max_points_per_attribute: 100, max_depth: 20, levels_per_depth: 3, depth_xp_bonus: 0.1, depth_radius: 80.0 }
    }
}

impl Settings {
    pub fn from_json(v: &Value) -> Settings {
        let d = Settings::default();
        Settings {
            xp_multiplier: v["xp_multiplier"].as_f64().filter(|m| m.is_finite() && *m >= 0.0).unwrap_or(d.xp_multiplier),
            points_per_level: v["points_per_level"].as_u64().map_or(d.points_per_level, |p| p.min(100) as u32),
            max_points_per_attribute: v["max_points_per_attribute"].as_u64().map_or(d.max_points_per_attribute, |p| p.min(200) as u32),
            max_depth: v["max_depth"].as_u64().map_or(d.max_depth, |p| p.min(100) as u32),
            levels_per_depth: v["levels_per_depth"].as_u64().map_or(d.levels_per_depth, |p| p.min(50) as u32),
            depth_xp_bonus: v["depth_xp_bonus"].as_f64().filter(|b| b.is_finite() && *b >= 0.0).unwrap_or(d.depth_xp_bonus),
            depth_radius: v["depth_radius"].as_f64().filter(|r| r.is_finite() && *r > 0.0).map_or(d.depth_radius, |r| r.min(300.0)),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "xp_multiplier": self.xp_multiplier,
            "points_per_level": self.points_per_level,
            "max_points_per_attribute": self.max_points_per_attribute,
            "max_depth": self.max_depth,
            "levels_per_depth": self.levels_per_depth,
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
}

impl Account {
    pub fn from_json(v: &Value) -> Account {
        let mut points = [0u32; 6];
        for (i, (key, _)) in STATS.iter().enumerate() {
            points[i] = v["points"][key].as_u64().unwrap_or(0) as u32;
        }
        Account { level: v["level"].as_u64().unwrap_or(0) as u32, xp: v["xp"].as_u64().unwrap_or(0), points, depth: v["depth"].as_u64().unwrap_or(0) as u32 }
    }

    pub fn to_json(&self) -> Value {
        let points: serde_json::Map<String, Value> = STATS.iter().zip(self.points).map(|((k, _), p)| (k.to_string(), json!(p))).collect();
        json!({ "level": self.level, "xp": self.xp, "points": points, "depth": self.depth })
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
    fn json_round_trip() {
        let a = Account { level: 4, xp: 77, points: [1, 0, 0, 2, 0, 1], depth: 2 };
        assert_eq!(Account::from_json(&a.to_json()), a);
        let s = Settings { xp_multiplier: 2.0, points_per_level: 2, max_points_per_attribute: 50, max_depth: 5, levels_per_depth: 4, depth_xp_bonus: 0.2, depth_radius: 60.0 };
        assert_eq!(Settings::from_json(&s.to_json()), s);
        assert_eq!(Settings::from_json(&json!({})), Settings::default());
    }
}
