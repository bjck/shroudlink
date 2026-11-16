//! Damage meter: damage dealt, damage taken, healing done and healing received per player,
//! for the last fight and the session, plus "what killed me" (the last killing blows and
//! the hits before them). Counted on the server from the game's own hit events, so players
//! without the addon are counted too; the addon shows it (MODS -> Damage meter) and a short
//! message after each fight, and a small panel on the HUD with the fight's damage per
//! player as bars (can be switched off on the meter's page). Built from the SDK: game events, `players`, `component`,
//! `player_of`, `on_tick`.

mod tally;

use std::collections::HashSet;
use std::time::Instant;

use shroudlink_sdk::ui::{Event, FromClient, HudRow, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{export_mod, log, Events, GameEvent, Host, Mod, Value, World};

use tally::{damage_type_names, Death, Fight, Hit, Tally};

const ID: &str = "meter";
/// `keen::ecs::HitEvent` (0xf8 bytes)
const HIT_ROOT_SOURCE: usize = 0x90;
const HIT_TARGET: usize = 0x94;
const HIT_FLAGS: usize = 0x98;
const HIT_ROOT_TARGET: usize = 0xa0;
const HIT_HEALTH_CHANGE: usize = 0xbc;
const HIT_DAMAGE_TYPES: usize = 0xdc;
/// `keen::ecs::HitEventFlags` bits
const FLAG_CRIT: u32 = 1 << 3;
const FLAG_KILLING_BLOW: u32 = 1 << 13;
const FLAG_HEALING: u32 = 1 << 14;
/// `keen::ecs::EntityDiedEvent.deadEntityId`
const DIED_ENTITY: usize = 0x8;
/// `keen::FactionType`, in enum order
const FACTIONS: [&str; 16] = [
    "the environment", "a trap", "a Fell creature", "a Vukah", "a Scavenger", "an animal", "a wild beast", "a player", "a player's helper", "a hazard", "a skeleton", "an ally", "a farm animal", "a cyclops", "a Drak", "an Ancient",
];
/// The first hits involving players are logged raw, to check the field reading in game.
const LOGGED_HITS: u32 = 40;
/// Players shown in the HUD panel.
const HUD_ROWS: usize = 5;

#[derive(Default)]
pub struct Meter {
    tally: Tally,
    start: Option<Instant>,
    players: HashSet<u32>,
    clients: Vec<u64>,
    logged: u32,
    /// clients that switched the HUD panel off (stored on their PC)
    hud_off: HashSet<u64>,
    /// what each client's HUD panel shows, to send only changes
    hud_sent: Vec<(u64, Value)>,
}

impl Meter {
    fn now(&mut self) -> f64 {
        self.start.get_or_insert_with(Instant::now).elapsed().as_secs_f64()
    }

    fn name(world: &World, viewer: u64, player: u32) -> String {
        if world.player_of(viewer) == Some(player) {
            "You".into()
        } else {
            player_name(world, player)
        }
    }

    fn window(&self, host: &Host, client: u64) -> ToClient {
        let world = host.world();
        let mut content = Vec::new();
        let rows = self.tally.table();
        content.push(Node::Heading { text: "Last fight".into() });
        if rows.iter().all(|r| r.2.is_none()) {
            content.push(Node::Text { text: "No fight yet.".into() });
        }
        for (id, _, fight) in &rows {
            if let Some(f) = fight {
                content.push(Node::Text {
                    text: format!(
                        "{}: {} damage ({} DPS, top hit {}), {} taken, {} healing, {} healed, {:.0} s",
                        Self::name(&world, client, *id),
                        num(f.totals.damage),
                        num(f.dps() as u64),
                        num(f.totals.top_hit as u64),
                        num(f.totals.taken),
                        num(f.totals.healing),
                        num(f.totals.healed),
                        f.seconds()
                    ),
                });
            }
        }
        content.push(Node::Separator);
        content.push(Node::Heading { text: "This session".into() });
        if rows.is_empty() {
            content.push(Node::Text { text: "Nothing counted yet.".into() });
        }
        for (id, t, _) in &rows {
            content.push(Node::Text {
                text: format!("{}: {} damage, {} taken, {} healing, {} healed", Self::name(&world, client, *id), num(t.damage), num(t.taken), num(t.healing), num(t.healed)),
            });
        }
        content.push(Node::Separator);
        content.push(Node::Heading { text: "What killed me".into() });
        match world.player_of(client) {
            None => content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() }),
            Some(me) => {
                let deaths = self.tally.deaths(me);
                if deaths.is_empty() {
                    content.push(Node::Text { text: "You have not died this session.".into() });
                }
                let now = self.start.map_or(0.0, |s| s.elapsed().as_secs_f64());
                content.extend(deaths.iter().map(|d| Node::Text { text: describe_death(d, now) }));
            }
        }
        content.push(Node::Toggle { id: "hud".into(), label: "Damage panel on the HUD (drag it while a menu is open)".into(), value: !self.hud_off.contains(&client) });
        content.push(Node::Row { children: vec![Node::Button { id: "refresh".into(), label: "Refresh".into() }, Node::Button { id: "reset".into(), label: "Reset session".into() }] });
        ToClient::Open { owner: ID.into(), window: Window { id: "main".into(), title: "Damage meter".into(), content } }
    }

    /// The HUD panel for `client`: the fight going on (or the last one), damage per player
    /// as bars relative to the top damage. None: nothing to show.
    fn hud(&self, world: &World, client: u64) -> Option<ToClient> {
        let mut fights: Vec<(u32, Fight, bool)> = self.tally.fights();
        fights.retain(|(_, f, _)| f.totals.damage > 0);
        if fights.is_empty() {
            return None;
        }
        fights.sort_by(|a, b| b.1.totals.damage.cmp(&a.1.totals.damage).then(a.0.cmp(&b.0)));
        fights.truncate(HUD_ROWS);
        let top = fights[0].1.totals.damage as f32;
        let going_on = fights.iter().any(|(_, _, active)| *active);
        let seconds = fights.iter().map(|(_, f, _)| f.seconds()).fold(0.0, f64::max);
        let title = format!("{} - {seconds:.0} s", if going_on { "Damage" } else { "Last fight" });
        let rows = fights
            .iter()
            .map(|(id, f, _)| HudRow { label: Self::name(world, client, *id), value: format!("{} ({})", num(f.totals.damage), num(f.dps() as u64)), fraction: f.totals.damage as f32 / top })
            .collect();
        Some(ToClient::Hud { owner: ID.into(), id: "dps".into(), title, rows })
    }

    /// Sends each client its HUD panel when it changed (an empty one removes it).
    fn send_huds(&mut self, host: &mut Host) {
        let messages: Vec<(u64, Value)> = {
            let world = host.world();
            self.clients
                .iter()
                .map(|&client| {
                    let hud = if self.hud_off.contains(&client) { None } else { self.hud(&world, client) };
                    (client, hud.unwrap_or_else(|| ToClient::Hud { owner: ID.into(), id: "dps".into(), title: String::new(), rows: Vec::new() }).to_message())
                })
                .collect()
        };
        for (client, message) in messages {
            let sent = self.hud_sent.iter_mut().find(|(c, _)| *c == client);
            match sent {
                Some((_, last)) if *last == message => continue,
                Some((_, last)) => *last = message.clone(),
                // nothing shown yet: an empty panel need not be sent
                None if message["rows"].as_array().is_some_and(|r| r.is_empty()) => continue,
                None => self.hud_sent.push((client, message.clone())),
            }
            host.send(client, &message);
        }
    }

    fn on_hit(&mut self, events: &Events, event: &GameEvent) {
        let (Some(source), Some(target), Some(flags), Some(change), Some(types)) =
            (event.u32_at(HIT_ROOT_SOURCE), event.u32_at(HIT_TARGET), event.u32_at(HIT_FLAGS), event.u32_at(HIT_HEALTH_CHANGE), event.u32_at(HIT_DAMAGE_TYPES))
        else {
            return;
        };
        let target = event.u32_at(HIT_ROOT_TARGET).filter(|&t| t != 0).unwrap_or(target);
        if self.players.is_empty() {
            self.players = events.world().players().into_iter().collect();
        }
        let involves_player = self.players.contains(&source) || self.players.contains(&target);
        if !involves_player {
            return;
        }
        // harvesting (trees, rocks, ore) comes as hits on terrain and props: only creatures,
        // which have a faction, count as damage dealt
        if self.players.contains(&source) && !self.players.contains(&target) && events.world().component(target, "Faction").is_none() {
            return;
        }
        if self.logged < LOGGED_HITS {
            self.logged += 1;
            log!("meter: hit source {source} target {target} flags {flags:#x} healthChange {} types {:#x}", change as i32, types & 0xffff);
        }
        let attacker = if self.players.contains(&target) { describe(&events.world(), &self.players, source) } else { String::new() };
        let hit = Hit {
            source,
            target,
            amount: (change as i32).unsigned_abs(),
            healing: flags & FLAG_HEALING != 0,
            killing_blow: flags & FLAG_KILLING_BLOW != 0,
            crit: flags & FLAG_CRIT != 0,
            damage_types: types as u16,
            attacker,
        };
        let now = self.now();
        let players = &self.players;
        self.tally.hit(now, &hit, |e| players.contains(&e));
    }
}

/// Who `entity` is, for "what killed me": a player, or a faction with a level.
fn describe(world: &World, players: &HashSet<u32>, entity: u32) -> String {
    if entity == 0 {
        return "the world (a fall, drowning …)".into();
    }
    if players.contains(&entity) {
        return player_name(world, entity);
    }
    // `keen::ecs::Faction` (1 byte), `keen::ecs::Level.dataStorage[0]` at +8
    let faction = world.component(entity, "Faction").and_then(|b| b.first().copied()).and_then(|f| FACTIONS.get(f as usize)).copied().unwrap_or("an enemy");
    let level = world.component(entity, "Level").and_then(|b| b.get(8..12).map(|l| u32::from_le_bytes(l.try_into().unwrap()))).filter(|&l| l > 0 && l < 1000);
    match level {
        Some(l) => format!("{faction} (level {l})"),
        None => faction.to_string(),
    }
}

/// A player's Steam name (from their addon), else a number.
fn player_name(world: &World, player: u32) -> String {
    world.player_name(player).unwrap_or_else(|| format!("Player {player}"))
}

fn describe_death(death: &Death, now: f64) -> String {
    let ago = (now - death.at).max(0.0);
    let when = if ago < 90.0 { format!("{ago:.0} s ago") } else { format!("{:.0} min ago", ago / 60.0) };
    match death.hits.first() {
        // falls, drowning and the shroud do not come as hits
        None => format!("{when}: a fall, drowning or the shroud"),
        Some(blow) => {
            let kind = damage_type_names(blow.damage_types).join("/");
            let kind = if kind.is_empty() { String::new() } else { format!(" {kind}") };
            let crit = if blow.crit { " (critical)" } else { "" };
            let before: u64 = death.hits[1..].iter().map(|h| h.amount as u64).sum();
            let before = if death.hits.len() > 1 { format!(", after {} more damage in {} hit(s)", num(before), death.hits.len() - 1) } else { String::new() };
            format!("{when}: {}{kind} damage{crit} from {}{before}", num(blow.amount as u64), blow.attacker)
        }
    }
}

fn summary(fight: &Fight) -> String {
    let t = &fight.totals;
    let mut parts = vec![format!("{} damage ({} DPS)", num(t.damage), num(fight.dps() as u64))];
    if t.taken > 0 {
        parts.push(format!("{} taken", num(t.taken)));
    }
    if t.healing > 0 {
        parts.push(format!("{} healing", num(t.healing)));
    }
    if t.healed > 0 {
        parts.push(format!("{} healed", num(t.healed)));
    }
    format!("Fight over ({:.0} s): {}", fight.seconds(), parts.join(", "))
}

/// 12345 -> "12,345"
fn num(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

impl Mod for Meter {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        if !self.clients.contains(&client) {
            self.clients.push(client);
        }
        let entries = ToClient::Entries { owner: ID.into(), entries: vec![MenuEntry { id: "open".into(), label: "Damage meter".into() }] };
        host.send(client, &entries.to_message());
        // a panel from before a restart or reload of this mod would stay up
        self.hud_sent.retain(|(c, _)| *c != client);
        host.send(client, &ToClient::Hud { owner: ID.into(), id: "dps".into(), title: String::new(), rows: Vec::new() }.to_message());
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        if let Some(FromClient::Stored { data }) = FromClient::from_message(message) {
            if data.get("hud") == Some(&Value::Bool(false)) {
                self.hud_off.insert(client);
            }
            return;
        }
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        match (node.as_str(), event) {
            ("open", Event::Menu) | ("refresh", Event::Click { .. }) => {}
            ("reset", Event::Click { .. }) => self.tally.reset(),
            ("hud", Event::Change { value }) => {
                let on = value.as_bool().unwrap_or(true);
                if on {
                    self.hud_off.remove(&client);
                } else {
                    self.hud_off.insert(client);
                }
                host.send(client, &ToClient::Store { owner: ID.into(), data: shroudlink_sdk::json!({ "hud": on }) }.to_message());
                self.send_huds(host);
            }
            _ => return,
        }
        let window = self.window(host, client);
        host.send(client, &window.to_message());
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.clients.retain(|&c| c != client);
        self.hud_off.remove(&client);
        self.hud_sent.retain(|(c, _)| *c != client);
    }

    fn game_events(&self) -> Vec<&'static str> {
        vec!["HitEvent", "EntityDiedEvent"]
    }

    fn on_game_event(&mut self, events: &Events, event: &GameEvent) {
        match event.name {
            "HitEvent" => self.on_hit(events, event),
            "EntityDiedEvent" => {
                if let Some(dead) = event.u32_at(DIED_ENTITY).filter(|d| self.players.contains(d)) {
                    let now = self.now();
                    self.tally.died(dead, now);
                }
            }
            _ => {}
        }
    }

    fn on_tick(&mut self, host: &mut Host) {
        self.players = host.world().players().into_iter().collect();
        let now = self.now();
        for (player, fight) in self.tally.finished(now) {
            let to: Vec<u64> = self.clients.iter().copied().filter(|&c| host.world().player_of(c) == Some(player)).collect();
            for client in to {
                host.send(client, &ToClient::Notify { owner: ID.into(), text: summary(&fight) }.to_message());
            }
        }
        self.send_huds(host);
    }
}

export_mod!(Meter);

#[cfg(test)]
mod tests {
    use super::*;
    use shroudlink_sdk::testing::MockHost;

    #[test]
    fn numbers_get_thousands_separators() {
        assert_eq!(num(0), "0");
        assert_eq!(num(999), "999");
        assert_eq!(num(12345), "12,345");
        assert_eq!(num(1234567), "1,234,567");
    }

    #[test]
    fn a_fight_summary_lists_what_happened() {
        let fight = Fight { start: 0.0, last: 20.0, totals: tally::Totals { damage: 4000, taken: 300, healing: 0, healed: 50, top_hit: 900 } };
        assert_eq!(summary(&fight), "Fight over (20 s): 4,000 damage (200 DPS), 300 taken, 50 healed");
    }

    #[test]
    fn a_death_names_the_blow_and_what_came_before() {
        let blow = Hit { source: 9, target: 1, amount: 340, healing: false, killing_blow: true, crit: true, damage_types: 1 << 3, attacker: "a Scavenger (level 12)".into() };
        let before = Hit { amount: 60, killing_blow: false, crit: false, ..blow.clone() };
        let death = Death { at: 10.0, hits: vec![blow, before] };
        assert_eq!(describe_death(&death, 40.0), "30 s ago: 340 fire damage (critical) from a Scavenger (level 12), after 60 more damage in 1 hit(s)");
    }

    #[test]
    fn the_hud_shows_the_fight_as_bars_and_can_be_switched_off() {
        let mut m = Meter::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        host.take_sent();
        m.on_tick(&mut host.host());
        assert!(host.take_sent().is_empty(), "no fight, no panel");
        let players = |e: u32| e == 7 || e == 8;
        let hit = |source, amount| Hit { source, target: 99, amount, healing: false, killing_blow: false, crit: false, damage_types: 0, attacker: String::new() };
        m.tally.hit(0.0, &hit(7, 300), players);
        m.tally.hit(2.0, &hit(8, 600), players);
        m.on_tick(&mut host.host());
        let sent = host.take_sent();
        let hud = &sent.last().unwrap().1;
        assert_eq!(hud["op"], "hud");
        assert_eq!(hud["rows"][0]["label"], "Player 8");
        assert_eq!(hud["rows"][0]["fraction"], 1.0);
        assert_eq!(hud["rows"][1]["fraction"], 0.5);
        m.on_tick(&mut host.host());
        assert!(host.take_sent().is_empty(), "unchanged: not sent again");
        let off = UiEvent { window: "main".into(), node: "hud".into(), event: Event::Change { value: Value::Bool(false) } };
        m.on_message(&mut host.host(), 1, &off.to_message(ID));
        let sent = host.take_sent();
        assert!(sent.iter().any(|(_, m)| m["op"] == "store" && m["data"]["hud"] == false));
        assert!(sent.iter().any(|(_, m)| m["op"] == "hud" && m["rows"].as_array().unwrap().is_empty()), "switched off: removed");
    }

    #[test]
    fn the_window_opens_without_any_fight() {
        let mut m = Meter::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        assert_eq!(host.take_sent()[0].1["op"], "entries");
        let open = UiEvent { window: String::new(), node: "open".into(), event: Event::Menu };
        m.on_message(&mut host.host(), 1, &open.to_message(ID));
        let window = host.take_sent()[0].1["window"].to_string();
        assert!(window.contains("No fight yet") && window.contains("What killed me"));
    }
}
