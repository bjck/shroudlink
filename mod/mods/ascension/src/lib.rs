//! Ascension: XP past the level cap fills Ascension levels without end; each gives an
//! attribute point the player puts on one of the six primary attributes and a Paragon
//! point for skills that improve what the character already does (mining, gliding,
//! stamina, speed, casting) (mod/docs/endgame.md).
//!
//! - XP: the game discards XP at the cap (`player_level` zeroes the gain) but still sends
//!   `XPGainedEvent`; XP of a player whose Level is at Level_Max counts here.
//! - Key: the player's Steam account (`World::player_account`, reported by the addon),
//!   as entity ids change every session. Characters of one account share it.
//! - Points go onto `X_Base` (what skill nodes raise), never the Flame or derived slots;
//!   Paragon ranks onto the slots of their skill. Slots are rebuilt when a player spawns:
//!   the mod adds its amount to each player entity once, on top of what the game, buffs and
//!   gear put there, and adds it again when the slot drops by exactly that much (a rebuild).
//!   What it added to which entity is saved with the server's process id, so a hot reload
//!   does not add twice and a restart starts over.
//! - Torment (`depths.rs`, "depth" in the code): a player picks a Torment level up to their
//!   Ascension level; from Torment 1 hostile enemies around them are at least level 50 and
//!   get stronger with each level, and their Ascension XP gets a bonus.
//! - Saved in `shroudlink_ascension.json` next to the server (`.new`, then renamed).

mod depths;
mod progress;

use std::collections::HashMap;
use std::path::Path;

use depths::Depths;
use progress::{ascension_required, Account, Settings, SKILLS, STATS};
use shroudlink_sdk::ui::{Event, MenuEntry, Node, ToClient, TreeLink, TreeNode, UiEvent, Window};
use shroudlink_sdk::{attr, export_mod, json, log, Events, GameEvent, Host, Known, Mod, Value, World};

const ID: &str = "ascension";
/// The Paragon tree (`ToClient::SkillTree`) and the tab that shows it share this id.
const TREE: &str = "paragon";
/// Centre of the game's skill tree space and the radius of the Paragon ring around it.
const TREE_CENTRE: (f32, f32) = (4000.0, 4000.0);
const TREE_RADIUS: f32 = 520.0;
/// The skill point emblem on the Paragon tree: purple (0xAABBGGRR; it multiplies the gold).
const PARAGON_TINT: u32 = 0xffff_3080;
const FILE: &str = "shroudlink_ascension.json";
/// ticks (about seconds) between saves of new XP
const SAVE_EVERY: u32 = 30;
/// ticks between checks for raised enemies that are gone
const FORGET_EVERY: u32 = 30;

/// The mod's two windows: Ascension (MODS menu) and Paragon (its own tab next to MODS).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Page {
    Ascension,
    Paragon,
}

impl Page {
    fn id(self) -> &'static str {
        match self {
            Page::Ascension => "main",
            Page::Paragon => "paragon",
        }
    }
}

/// What the mod added to one player entity in this server run.
#[derive(Clone, Debug, PartialEq)]
struct Applied {
    account: u64,
    /// slot id -> (amount added, the slot right after the last look: a drop by exactly the
    /// amount is a rebuild)
    slots: HashMap<u32, (f64, f64)>,
}

fn same(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-3 * a.abs().max(b.abs()).max(1.0)
}

#[derive(Default)]
pub struct Ascension {
    settings: Settings,
    accounts: HashMap<u64, Account>,
    applied: HashMap<u32, Applied>,
    clients: Vec<u64>,
    /// client -> its player entity, as last seen
    players: HashMap<u64, u32>,
    /// clients with a window open, and which
    open: HashMap<u64, Page>,
    /// players at the cap without a known account, told once
    warned: Vec<u32>,
    dirty: bool,
    ticks: u32,
    depths: Depths,
    /// round robin over the players at a depth, one sweep per tick
    sweep_next: usize,
    /// clients that have their Paragon tree
    trees_sent: Vec<u64>,
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
                let slots = a["slots"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| Some((s[0].as_u64()? as u32, (s[1].as_f64()?, s[2].as_f64().unwrap_or(f64::NAN)))))
                    .collect();
                self.applied.insert(entity as u32, Applied { account, slots });
            }
        }
        log!("ascension: {} account(s), {} player(s) with points this run", self.accounts.len(), self.applied.len());
    }

    fn save(&mut self) {
        let accounts: serde_json::Map<String, Value> = self.accounts.iter().map(|(k, a)| (k.to_string(), a.to_json())).collect();
        let applied: Vec<Value> = self
            .applied
            .iter()
            .map(|(e, a)| {
                let slots: Vec<Value> = a.slots.iter().map(|(id, (amount, seen))| json!([id, amount, if seen.is_finite() { json!(seen) } else { Value::Null }])).collect();
                json!({ "entity": e, "account": a.account.to_string(), "slots": slots })
            })
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

    /// Adds or takes back what the account gives (attribute points, Paragon ranks) until each
    /// slot of the player holds it on top of its own value. Returns whether anything was written.
    fn sync(&mut self, world: &World, player: u32, account: u64) -> bool {
        let wanted: Vec<(u32, f64)> = self.accounts.get(&account).map_or_else(Vec::new, |a| {
            let mut by_slot: Vec<(u32, f64)> = Vec::new();
            for (name, amount) in a.bonuses() {
                let Some(id) = attr::id_of(name) else { continue };
                match by_slot.iter_mut().find(|s| s.0 == id) {
                    Some(s) => s.1 += amount,
                    None => by_slot.push((id, amount)),
                }
            }
            by_slot
        });
        let entry = self.applied.entry(player).or_insert_with(|| Applied { account, slots: HashMap::new() });
        if entry.account != account {
            *entry = Applied { account, slots: HashMap::new() };
        }
        // slots the account no longer adds to are taken back too
        let mut ids: Vec<u32> = wanted.iter().map(|w| w.0).collect();
        let old: Vec<u32> = entry.slots.keys().filter(|id| !ids.contains(id)).copied().collect();
        ids.extend(old);
        let mut wrote = false;
        for id in ids {
            let want = wanted.iter().find(|w| w.0 == id).map_or(0.0, |w| w.1);
            let Some(current) = world.attribute(player, id) else { continue };
            let (mut added, seen) = entry.slots.get(&id).copied().unwrap_or((0.0, f64::NAN));
            // the game rebuilt the slot (a spawn): what we added is gone
            if added != 0.0 && seen.is_finite() && same(current, seen - added) {
                log!("ascension: player {player}: slot {id:#010x} was rebuilt, adding {added} again");
                added = 0.0;
            }
            if want != added && world.set_attribute(player, id, current + want - added) {
                added = want;
                wrote = true;
            }
            let now = world.attribute(player, id).unwrap_or(current);
            if added == 0.0 && want == 0.0 {
                entry.slots.remove(&id);
            } else {
                entry.slots.insert(id, (added, now));
            }
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
        // loot follows the deepest Torment among the players online (the game's loot rolls
        // carry no player; on a server of friends they mostly play together)
        let deepest = deep.iter().map(|d| d.1).max().unwrap_or(0);
        let (min_level, add_levels, rarity) = depths::loot(deepest, self.settings.torment_min_level, self.settings.levels_per_depth);
        world.set_loot_boost(min_level, add_levels, rarity);
        if deep.is_empty() {
            if !self.depths.raised.is_empty() {
                let lowered = self.depths.lower_all(world);
                log!("ascension: nobody at a Torment level, {lowered} enemy(s) back to their own level");
            }
            return;
        }
        let (player, depth) = deep[self.sweep_next % deep.len()];
        self.sweep_next = self.sweep_next.wrapping_add(1);
        let raised = self.depths.sweep(world, player, depth, self.settings.levels_per_depth, self.settings.torment_min_level, self.settings.depth_radius);
        if raised > 0 {
            log!("ascension: {raised} enemy(s) around player {player} raised to Torment {depth} ({} raised in all)", self.depths.raised.len());
        }
    }

    fn window(&self, world: &World, client: u64, page: Page) -> ToClient {
        let mut content = Vec::new();
        let player = self.players.get(&client).copied();
        let account = player.and_then(|p| world.player_account(p));
        match (player, account) {
            (None, _) => content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() }),
            (Some(_), None) => content.push(Node::Text { text: "Your shroudlink addon is too old to keep Ascension: please update it.".into() }),
            (Some(p), Some(account)) => {
                let a = self.accounts.get(&account).cloned().unwrap_or_default();
                match page {
                    Page::Ascension => self.ascension_page(world, p, &a, &mut content),
                    Page::Paragon => self.paragon_page(&a, &mut content),
                }
            }
        }
        let title = match page {
            Page::Ascension => "Ascension",
            Page::Paragon => "Paragon",
        };
        ToClient::Open { owner: ID.into(), window: Window { id: page.id().into(), title: title.into(), content } }
    }

    fn ascension_page(&self, world: &World, player: u32, a: &Account, content: &mut Vec<Node>) {
        content.push(Node::Heading { text: format!("Ascension {}", a.level) });
        let at_cap = matches!((world.attribute(player, attr::LEVEL), world.attribute(player, attr::LEVEL_MAX)), (Some(l), Some(m)) if l >= m);
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
            content.push(Node::Text { text: "Torment unlocks with Ascension 1: enemies around you get stronger, your Ascension XP grows faster.".into() });
        } else {
            let depth = a.depth(&self.settings);
            content.push(Node::Heading { text: format!("Torment {depth} of {unlocked}") });
            content.push(Node::Text {
                text: if depth == 0 {
                    "Enemies as the game made them.".into()
                } else {
                    format!(
                        "Enemies around you: at least level {:.0}{}. Loot: item level at least {:.0}{}, more rare, epic and legendary gear. Ascension XP: +{:.0} %.",
                        self.settings.torment_min_level,
                        if depth > 1 { format!(", then +{} levels", (depth - 1) * self.settings.levels_per_depth) } else { String::new() },
                        self.settings.torment_min_level,
                        if depth > 1 { format!(" +{}", (depth - 1) * self.settings.levels_per_depth) } else { String::new() },
                        self.settings.depth_xp_bonus * depth as f64 * 100.0
                    )
                },
            });
            content.push(Node::Row {
                children: vec![Node::Button { id: "depth:-".into(), label: "Torment -1".into() }, Node::Button { id: "depth:+".into(), label: "Torment +1".into() }],
            });
        }
    }

    fn paragon_page(&self, a: &Account, content: &mut Vec<Node>) {
        content.push(Node::Heading { text: format!("Paragon points to spend: {}", a.paragon_unspent(&self.settings)) });
        content.push(Node::Text { text: format!("Every Ascension level gives {} Paragon point(s). Ranks improve what your character already does.", self.settings.paragon_per_level) });
        content.push(Node::Separator);
        for (i, skill) in SKILLS.iter().enumerate() {
            content.push(Node::Row {
                children: vec![
                    Node::Button { id: format!("learn:{}", skill.key), label: "+1".into() },
                    Node::Text { text: format!("{}  {}/{}{}", skill.name, a.paragon[i], skill.max_rank, if skill.verified { "" } else { "  (experimental)" }) },
                ],
            });
            // the game's font has no filled box glyph: the ranks as text
            content.push(Node::Text { text: format!("{} per rank", skill.effect) });
        }
        content.push(Node::Separator);
        content.push(Node::Row { children: vec![Node::Button { id: "paragon_reset".into(), label: "Take all Paragon ranks back".into() }] });
    }

    /// The Paragon skills as a skill tree for the game's SKILLS page: a start node in the
    /// middle, the skills on a ring around it.
    fn paragon_tree(&self, account: Option<&Account>) -> ToClient {
        let (cx, cy) = TREE_CENTRE;
        let unspent = account.map_or(0, |a| a.paragon_unspent(&self.settings));
        let mut nodes = vec![TreeNode { id: "start".into(), x: cx, y: cy, name: "Paragon".into(), description: format!("{unspent} Paragon point(s) to spend."), rank: 0, max_rank: 0, icon: 0 }];
        let mut links = Vec::new();
        for (i, skill) in SKILLS.iter().enumerate() {
            let angle = std::f32::consts::TAU * i as f32 / SKILLS.len() as f32 - std::f32::consts::FRAC_PI_2;
            let rank = account.map_or(0, |a| a.paragon[i]);
            nodes.push(TreeNode {
                id: skill.key.into(),
                x: cx + TREE_RADIUS * angle.cos(),
                y: cy + TREE_RADIUS * angle.sin(),
                name: skill.name.into(),
                description: format!("{} per rank.{}", skill.effect, if skill.verified { "" } else { " (experimental)" }),
                rank,
                max_rank: skill.max_rank,
                icon: skill.icon,
            });
            links.push(TreeLink { from: "start".into(), to: skill.key.into() });
        }
        let spent = account.map_or(0, |a| a.paragon.iter().sum());
        ToClient::SkillTree { owner: ID.into(), id: TREE.into(), nodes, links, points: Some([spent, unspent]), tint: Some(PARAGON_TINT) }
    }

    fn send_tree(&mut self, host: &mut Host, client: u64) {
        let account = self.players.get(&client).and_then(|&p| host.world().player_account(p));
        let tree = self.paragon_tree(account.and_then(|a| self.accounts.get(&a)));
        host.send(client, &tree.to_message());
        if account.is_some() && !self.trees_sent.contains(&client) {
            self.trees_sent.push(client);
        }
    }

    fn refresh(&self, host: &mut Host) {
        for (&client, &page) in &self.open {
            let window = self.window(&host.world(), client, page);
            host.send(client, &window.to_message());
        }
    }

    /// Shows `page` to a client and closes the other window there.
    fn show(&mut self, host: &mut Host, client: u64, page: Page) {
        let other = if page == Page::Ascension { Page::Paragon } else { Page::Ascension };
        host.send(client, &ToClient::Close { owner: ID.into(), id: other.id().into() }.to_message());
        self.open.insert(client, page);
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
                if let Some(&page) = self.open.get(&client) {
                    events.send(client, &self.window(&world, client, page).to_message());
                }
            }
        }
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        self.clients.push(client);
        let entries = ToClient::Entries {
            owner: ID.into(),
            entries: vec![MenuEntry { id: "open".into(), label: "Ascension".into() }, MenuEntry { id: "paragon".into(), label: "Paragon".into() }],
        };
        host.send(client, &entries.to_message());
        // Paragon also as a tab of its own next to MODS (older addons ignore it and keep the entry)
        host.send(client, &ToClient::Tab { owner: ID.into(), id: "paragon".into(), label: "Paragon".into() }.to_message());
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.clients.retain(|&c| c != client);
        self.trees_sent.retain(|&c| c != client);
        self.open.remove(&client);
        if let Some(player) = self.players.remove(&client) {
            self.applied.remove(&player);
        }
        self.save();
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { window, node, event }) = UiEvent::from_message(message) else { return };
        if matches!(event, Event::Closed) {
            self.open.remove(&client);
            return;
        }
        let mut page = if window == Page::Paragon.id() { Page::Paragon } else { Page::Ascension };
        let world = host.world();
        let player = world.player_of(client);
        if let Some(p) = player {
            self.players.insert(client, p);
        }
        let account = player.and_then(|p| world.player_account(p));
        drop(world);
        match (node.as_str(), &event, account) {
            ("open", Event::Menu, _) => {
                page = Page::Ascension;
                self.show(host, client, page);
            }
            ("paragon", Event::Menu, _) => {
                page = Page::Paragon;
                self.show(host, client, page);
            }
            ("depth:-" | "depth:+", Event::Click { .. }, Some(account)) => {
                let a = self.accounts.entry(account).or_default();
                let now = a.depth(&self.settings);
                let want = if node == "depth:+" { now + 1 } else { now.saturating_sub(1) };
                let chosen = a.choose_depth(want, &self.settings);
                log!("ascension: player {:?} chose Torment {chosen}", player);
                self.dirty = true;
            }
            ("paragon_reset", Event::Click { .. }, Some(account)) => {
                self.accounts.entry(account).or_default().reset_paragon();
                self.dirty = true;
            }
            ("reset", Event::Click { .. }, Some(account)) => {
                self.accounts.entry(account).or_default().reset();
                self.dirty = true;
            }
            (id, Event::Click { .. }, Some(account)) => {
                let a = self.accounts.entry(account).or_default();
                let done = if let Some(i) = id.strip_prefix("add:").and_then(|k| STATS.iter().position(|s| s.0 == k)) {
                    a.spend(i, &self.settings)
                } else if let Some(i) = id.strip_prefix("learn:").and_then(|k| SKILLS.iter().position(|s| s.key == k)) {
                    a.learn(i, &self.settings)
                } else if let Some(i) = SKILLS.iter().position(|s| s.key == id).filter(|_| window == TREE) {
                    // invested in a node of the Paragon tree
                    a.learn(i, &self.settings)
                } else {
                    false
                };
                if !done {
                    return;
                }
                self.dirty = true;
            }
            _ => return,
        }
        if let (Some(p), Some(account)) = (player, account) {
            if self.sync(&host.world(), p, account) {
                self.save();
            }
        }
        self.send_tree(host, client);
        if window == TREE {
            return;
        }
        let window = self.window(&host.world(), client, page);
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
        // the Paragon tree goes out once a client's account is known, before its tab is opened
        let waiting: Vec<u64> = self.clients.iter().copied().filter(|c| !self.trees_sent.contains(c) && self.players.get(c).is_some_and(|&p| world.player_account(p).is_some())).collect();
        for client in waiting {
            self.send_tree(host, client);
        }
        if wrote {
            self.save();
            self.refresh(host);
        } else if self.dirty && self.ticks % SAVE_EVERY == 0 {
            self.save();
        }
    }
}

export_mod!(Ascension);
