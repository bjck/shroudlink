//! Ascension: XP past the level cap fills Ascension levels without end; each gives an
//! attribute point the player puts on one of the six primary attributes
//! (mod/docs/endgame.md).
//!
//! - XP: the game discards XP at the cap (`player_level` zeroes the gain) but still sends
//!   `XPGainedEvent`; XP of a player whose Level is at Level_Max counts here.
//! - Key: the player's Steam account (`World::player_account`, reported by the addon),
//!   as entity ids change every session. Characters of one account share it.
//! - Points go onto `X_Base` (what skill nodes raise), never the Flame or derived slots.
//!   `X_Base` is rebuilt when a player spawns: the mod adds its points to each player
//!   entity once and adds them again when the slot drops by exactly that much (a rebuild).
//!   What it added to which entity is saved with the server's process id, so a hot reload
//!   does not add twice and a restart starts over.
//! - Shroud Depths (`depths.rs`): a player picks a depth up to their Ascension level;
//!   hostile enemies around them get more levels, and their Ascension XP a bonus.
//! - Saved in `shroudlink_ascension.json` next to the server (`.new`, then renamed).

mod depths;
mod progress;

use std::collections::HashMap;
use std::path::Path;

use depths::Depths;
use progress::{ascension_required, Account, Settings, STATS};
use shroudlink_sdk::ui::{Event, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{attr, export_mod, json, log, Events, GameEvent, Host, Known, Mod, Value, World};

const ID: &str = "ascension";
const FILE: &str = "shroudlink_ascension.json";
/// ticks (about seconds) between saves of new XP
const SAVE_EVERY: u32 = 30;
/// ticks between checks for raised enemies that are gone
const FORGET_EVERY: u32 = 30;

/// `X_Base` slots of [`STATS`], in the same order.
const BASE: [u32; 6] = [attr::CONSTITUTION_BASE, attr::SPIRIT_BASE, attr::ENDURANCE_BASE, attr::STRENGTH_BASE, attr::DEXTERITY_BASE, attr::INTELLIGENCE_BASE];

/// Points added to one player entity in this server run.
#[derive(Clone, Debug, PartialEq)]
struct Applied {
    account: u64,
    points: [u32; 6],
    /// `X_Base` right after the last look: a drop by exactly `points` is a rebuild
    seen: [f64; 6],
}

#[derive(Default)]
pub struct Ascension {
    settings: Settings,
    accounts: HashMap<u64, Account>,
    applied: HashMap<u32, Applied>,
    clients: Vec<u64>,
    /// client -> its player entity, as last seen
    players: HashMap<u64, u32>,
    /// clients with the window open
    open: Vec<u64>,
    /// players at the cap without a known account, told once
    warned: Vec<u32>,
    dirty: bool,
    ticks: u32,
    depths: Depths,
    /// round robin over the players at a depth, one sweep per tick
    sweep_next: usize,
}

impl Ascension {
    fn load(&mut self) {
        let Ok(text) = std::fs::read_to_string(FILE) else { return };
        let Ok(v) = serde_json::from_str::<Value>(&text) else {
            log!("ascension: {FILE} is not valid JSON, starting empty (the file is kept until the next save)");
            return;
        };
        self.settings = Settings::from_json(&v["settings"]);
        self.accounts = v["accounts"].as_object().into_iter().flatten().filter_map(|(k, a)| Some((k.parse().ok()?, Account::from_json(a)))).collect();
        // what was added to which entity survives a hot reload, not a server restart
        if v["session"]["pid"].as_u64() == Some(std::process::id() as u64) {
            for a in v["session"]["applied"].as_array().into_iter().flatten() {
                let (Some(entity), Some(account)) = (a["entity"].as_u64(), a["account"].as_str().and_then(|s| s.parse().ok())) else { continue };
                let mut applied = Applied { account, points: [0; 6], seen: [f64::NAN; 6] };
                for i in 0..6 {
                    applied.points[i] = a["points"][i].as_u64().unwrap_or(0) as u32;
                    applied.seen[i] = a["seen"][i].as_f64().unwrap_or(f64::NAN);
                }
                self.applied.insert(entity as u32, applied);
            }
        }
        log!("ascension: {} account(s), {} player(s) with points this run", self.accounts.len(), self.applied.len());
    }

    fn save(&mut self) {
        let accounts: serde_json::Map<String, Value> = self.accounts.iter().map(|(k, a)| (k.to_string(), a.to_json())).collect();
        let applied: Vec<Value> = self
            .applied
            .iter()
            .map(|(e, a)| json!({ "entity": e, "account": a.account.to_string(), "points": a.points, "seen": a.seen.map(|s| if s.is_finite() { json!(s) } else { Value::Null }) }))
            .collect();
        let text = serde_json::to_string_pretty(&json!({
            "settings": self.settings.to_json(),
            "accounts": accounts,
            "session": { "pid": std::process::id(), "applied": applied },
        }))
        .unwrap_or_default();
        let new = Path::new(FILE).with_extension("json.new");
        match std::fs::write(&new, text).and_then(|_| std::fs::rename(&new, FILE)) {
            Ok(()) => self.dirty = false,
            Err(e) => log!("ascension: could not save {FILE}: {e}"),
        }
    }

    fn client_of(&self, player: u32) -> Option<u64> {
        self.players.iter().find(|(_, &p)| p == player).map(|(&c, _)| c)
    }

    /// Adds or takes back points on a player's `X_Base` until they match the account's.
    /// Returns whether anything was written.
    fn sync(&mut self, world: &World, player: u32, account: u64) -> bool {
        let desired = self.accounts.get(&account).map_or([0; 6], |a| a.points);
        let entry = self.applied.entry(player).or_insert(Applied { account, points: [0; 6], seen: [f64::NAN; 6] });
        if entry.account != account {
            *entry = Applied { account, points: [0; 6], seen: [f64::NAN; 6] };
        }
        let mut wrote = false;
        for i in 0..6 {
            let Some(current) = world.attribute(player, BASE[i]) else { continue };
            // the game rebuilt the slot (a spawn): our points are gone
            if entry.points[i] > 0 && entry.seen[i].is_finite() && current == entry.seen[i] - entry.points[i] as f64 {
                log!("ascension: player {player}'s {} was rebuilt, adding {} point(s) again", STATS[i].1, entry.points[i]);
                entry.points[i] = 0;
            }
            let delta = desired[i] as f64 - entry.points[i] as f64;
            if delta != 0.0 && world.set_attribute(player, BASE[i], current + delta) {
                entry.points[i] = desired[i];
                wrote = true;
            }
            entry.seen[i] = world.attribute(player, BASE[i]).unwrap_or(current);
        }
        wrote
    }

    /// Raises the enemies around one player at a depth (round robin, one per tick); with
    /// nobody at a depth, puts raised enemies back to their own level.
    fn sweep(&mut self, world: &World) {
        if self.ticks % FORGET_EVERY == 0 {
            self.depths.forget_gone(world);
        }
        let deep: Vec<(u32, u32)> = self
            .players
            .values()
            .filter_map(|&p| {
                let depth = self.accounts.get(&world.player_account(p)?)?.depth(&self.settings);
                (depth > 0).then_some((p, depth))
            })
            .collect();
        if deep.is_empty() {
            if !self.depths.raised.is_empty() {
                let lowered = self.depths.lower_all(world);
                log!("ascension: nobody at a Shroud Depth, {lowered} enemy(s) back to their own level");
            }
            return;
        }
        let (player, depth) = deep[self.sweep_next % deep.len()];
        self.sweep_next = self.sweep_next.wrapping_add(1);
        let raised = self.depths.sweep(world, player, depth, self.settings.levels_per_depth, self.settings.depth_radius);
        if raised > 0 {
            log!("ascension: {raised} enemy(s) around player {player} raised to depth {depth} ({} raised in all)", self.depths.raised.len());
        }
    }

    fn window(&self, world: &World, client: u64) -> ToClient {
        let mut content = Vec::new();
        let player = self.players.get(&client).copied();
        let account = player.and_then(|p| world.player_account(p));
        match (player, account) {
            (None, _) => content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() }),
            (Some(_), None) => content.push(Node::Text { text: "Your shroudlink addon is too old to keep Ascension: please update it.".into() }),
            (Some(p), Some(account)) => {
                let a = self.accounts.get(&account).cloned().unwrap_or_default();
                content.push(Node::Heading { text: format!("Ascension {}", a.level) });
                let at_cap = matches!((world.attribute(p, attr::LEVEL), world.attribute(p, attr::LEVEL_MAX)), (Some(l), Some(m)) if l >= m);
                if !at_cap {
                    content.push(Node::Text { text: "Ascension starts at the level cap: XP you earn past it counts here.".into() });
                }
                content.push(Node::Text { text: format!("XP {} / {} to Ascension {}", a.xp, ascension_required(a.level), a.level + 1) });
                content.push(Node::Text { text: format!("Points to spend: {}  (each level gives {}, at most {} per attribute)", a.unspent(&self.settings), self.settings.points_per_level, self.settings.max_points_per_attribute) });
                content.push(Node::Separator);
                for (i, (key, name)) in STATS.iter().enumerate() {
                    content.push(Node::Row {
                        children: vec![Node::Button { id: format!("add:{key}"), label: "+1".into() }, Node::Text { text: format!("{name} +{}", a.points[i]) }],
                    });
                }
                content.push(Node::Row { children: vec![Node::Button { id: "reset".into(), label: "Take all points back".into() }] });
                content.push(Node::Separator);
                let unlocked = a.unlocked_depth(&self.settings);
                if unlocked == 0 {
                    content.push(Node::Text { text: "Shroud Depths unlock with Ascension 1: enemies around you get stronger, your Ascension XP grows faster.".into() });
                } else {
                    let depth = a.depth(&self.settings);
                    content.push(Node::Heading { text: format!("Shroud Depth {depth} of {unlocked}") });
                    content.push(Node::Text {
                        text: if depth == 0 {
                            "Enemies as the game made them.".into()
                        } else {
                            format!("Enemies around you: +{} levels. Ascension XP: +{:.0} %.", depth * self.settings.levels_per_depth, self.settings.depth_xp_bonus * depth as f64 * 100.0)
                        },
                    });
                    content.push(Node::Row {
                        children: vec![Node::Button { id: "depth:-".into(), label: "Shallower".into() }, Node::Button { id: "depth:+".into(), label: "Deeper".into() }],
                    });
                }
            }
        }
        ToClient::Open { owner: ID.into(), window: Window { id: "main".into(), title: "Ascension".into(), content } }
    }

    fn refresh(&self, host: &mut Host) {
        for &client in &self.open {
            let window = self.window(&host.world(), client);
            host.send(client, &window.to_message());
        }
    }
}

impl Mod for Ascension {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_load(&mut self) {
        self.load();
    }

    fn game_events(&self) -> Vec<&'static str> {
        vec!["XPGainedEvent"]
    }

    fn on_game_event(&mut self, events: &Events, event: &GameEvent) {
        let Some(Known::XpGained { player, amount }) = event.known() else { return };
        let world = events.world();
        match (world.attribute(player, attr::LEVEL), world.attribute(player, attr::LEVEL_MAX)) {
            (Some(level), Some(max)) if level >= max => {}
            _ => return,
        }
        let Some(account) = world.player_account(player) else {
            if !self.warned.contains(&player) {
                self.warned.push(player);
                log!("ascension: player {player} is at the cap but has no Steam account known (no addon, an old addon, or no chest opened yet): XP not counted");
            }
            return;
        };
        let a = self.accounts.entry(account).or_default();
        let gain = a.ascension_xp(amount, &self.settings);
        let gained = a.add_xp(gain);
        self.dirty = true;
        if gained > 0 {
            let level = a.level;
            log!("ascension: player {player} reached Ascension {level}");
            if let Some(client) = self.client_of(player) {
                let text = format!("Ascension {level}! A new attribute point waits in MODS > Ascension.");
                events.send(client, &ToClient::Notify { owner: ID.into(), text }.to_message());
                if self.open.contains(&client) {
                    events.send(client, &self.window(&world, client).to_message());
                }
            }
        }
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        self.clients.push(client);
        let entries = ToClient::Entries { owner: ID.into(), entries: vec![MenuEntry { id: "open".into(), label: "Ascension".into() }] };
        host.send(client, &entries.to_message());
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.clients.retain(|&c| c != client);
        self.open.retain(|&c| c != client);
        if let Some(player) = self.players.remove(&client) {
            self.applied.remove(&player);
        }
        self.save();
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        if matches!(event, Event::Closed) {
            self.open.retain(|&c| c != client);
            return;
        }
        let world = host.world();
        let player = world.player_of(client);
        if let Some(p) = player {
            self.players.insert(client, p);
        }
        let account = player.and_then(|p| world.player_account(p));
        match (node.as_str(), &event, account) {
            ("open", Event::Menu, _) => {
                if !self.open.contains(&client) {
                    self.open.push(client);
                }
            }
            ("depth:-" | "depth:+", Event::Click { .. }, Some(account)) => {
                let a = self.accounts.entry(account).or_default();
                let now = a.depth(&self.settings);
                let want = if node == "depth:+" { now + 1 } else { now.saturating_sub(1) };
                let chosen = a.choose_depth(want, &self.settings);
                log!("ascension: player {:?} chose Shroud Depth {chosen}", player);
                self.dirty = true;
            }
            ("reset", Event::Click { .. }, Some(account)) => {
                self.accounts.entry(account).or_default().reset();
                self.dirty = true;
            }
            (id, Event::Click { .. }, Some(account)) => {
                let Some(i) = id.strip_prefix("add:").and_then(|k| STATS.iter().position(|s| s.0 == k)) else { return };
                if !self.accounts.entry(account).or_default().spend(i, &self.settings) {
                    return;
                }
                self.dirty = true;
            }
            _ => return,
        }
        if let (Some(p), Some(account)) = (player, account) {
            if self.sync(&world, p, account) {
                self.save();
            }
        }
        let window = self.window(&host.world(), client);
        host.send(client, &window.to_message());
    }

    fn on_tick(&mut self, host: &mut Host) {
        self.ticks += 1;
        let world = host.world();
        let mut wrote = false;
        for client in self.clients.clone() {
            let Some(player) = world.player_of(client) else { continue };
            if let Some(old) = self.players.insert(client, player).filter(|&old| old != player) {
                self.applied.remove(&old);
            }
            if let Some(account) = world.player_account(player) {
                wrote |= self.sync(&world, player, account);
            }
        }
        self.sweep(&world);
        if wrote {
            self.save();
            self.refresh(host);
        } else if self.dirty && self.ticks % SAVE_EVERY == 0 {
            self.save();
        }
    }
}

export_mod!(Ascension);
