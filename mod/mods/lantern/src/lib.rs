//! Lantern: a light around the player while the hands stay free for weapons. MODS ->
//! Lantern switches it on and off. The light is the game's own: the hidden buff of the
//! Ring of Light, which attaches a light effect to the chest. The server keeps it up the
//! way the ring does, and every client shows it with its own vanilla data, so nobody
//! needs new files.
//! Built only from the SDK: `player_of`, `apply_buff` and `on_tick`.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use shroudlink_sdk::ui::{Event, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{export_mod, log, Host, Mod, Value};

const ID: &str = "lantern";
/// `BuffType` of the Ring of Light's visual (`Visuals_Ring_Of_Light`): slot Buff, apply
/// type ResetTime, 0.3 s lifetime (the ring re-applies it), a light effect on the chest.
const LIGHT_BUFF: &str = "b2f8db72-a69d-43db-baf1-c69937ec0264";
/// How long the server keeps the light up per request ...
const KEEP: f64 = 30.0;
/// ... and how often the request is renewed, well before it runs out.
const REFRESH: Duration = Duration::from_secs(10);

/// A player with the light on.
struct Lit {
    player: u32,
    /// None: apply on the next tick
    applied: Option<Instant>,
}

#[derive(Default)]
pub struct Lantern {
    lit: HashMap<u64, Lit>,
    status: HashMap<u64, String>,
}

impl Lantern {
    fn window(&self, host: &Host, client: u64) -> ToClient {
        let mut content = vec![Node::Text { text: "A light around you, with your hands free for weapons and tools.".into() }];
        match (host.world().player_of(client), self.lit.contains_key(&client)) {
            (None, _) => content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() }),
            (Some(_), true) => content.push(Node::Button { id: "toggle".into(), label: "Light off".into() }),
            (Some(_), false) => content.push(Node::Button { id: "toggle".into(), label: "Light on".into() }),
        }
        if let Some(status) = self.status.get(&client) {
            content.push(Node::Text { text: status.clone() });
        }
        ToClient::Open { owner: ID.into(), window: Window { id: "main".into(), title: "Lantern".into(), content } }
    }

    fn toggle(&mut self, host: &Host, client: u64) -> String {
        if let Some(lit) = self.lit.remove(&client) {
            host.world().apply_buff(lit.player, LIGHT_BUFF, 0.0);
            return "Light off.".into();
        }
        let Some(player) = host.world().player_of(client) else { return "Unknown player: open a chest first.".into() };
        if !host.world().apply_buff(player, LIGHT_BUFF, KEEP) {
            return "This server cannot give lights (its shroudlink is too old, or the game changed).".into();
        }
        log!("lantern: light on for player {player}");
        self.lit.insert(client, Lit { player, applied: Some(Instant::now()) });
        "Light on.".into()
    }
}

impl Mod for Lantern {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        let entries = ToClient::Entries { owner: ID.into(), entries: vec![MenuEntry { id: "open".into(), label: "Lantern".into() }] };
        host.send(client, &entries.to_message());
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        match (node.as_str(), event) {
            ("open", Event::Menu) => {}
            ("toggle", Event::Click { .. }) => {
                let status = self.toggle(host, client);
                self.status.insert(client, status);
            }
            _ => return,
        }
        let window = self.window(host, client);
        host.send(client, &window.to_message());
    }

    fn on_leave(&mut self, host: &mut Host, client: u64) {
        if let Some(lit) = self.lit.remove(&client) {
            host.world().apply_buff(lit.player, LIGHT_BUFF, 0.0);
        }
        self.status.remove(&client);
    }

    fn on_tick(&mut self, host: &mut Host) {
        let world = host.world();
        for lit in self.lit.values_mut() {
            if lit.applied.is_none_or(|t| t.elapsed() >= REFRESH) {
                world.apply_buff(lit.player, LIGHT_BUFF, KEEP);
                lit.applied = Some(Instant::now());
            }
        }
    }
}

export_mod!(Lantern);

#[cfg(test)]
mod tests {
    use super::*;
    use shroudlink_sdk::testing::MockHost;

    #[test]
    fn without_a_known_player_there_is_no_switch() {
        let mut m = Lantern::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        assert_eq!(host.take_sent()[0].1["op"], "entries");
        let menu = UiEvent { window: String::new(), node: "open".into(), event: Event::Menu };
        m.on_message(&mut host.host(), 1, &menu.to_message(ID));
        let window = host.take_sent()[0].1["window"].to_string();
        assert!(window.contains("Open any chest once") && !window.contains("Light on"));
        let toggle = UiEvent { window: "main".into(), node: "toggle".into(), event: Event::Click { index: None } };
        m.on_message(&mut host.host(), 1, &toggle.to_message(ID));
        assert!(host.take_sent()[0].1["window"].to_string().contains("Unknown player"));
        assert!(m.lit.is_empty());
    }

    #[test]
    fn the_buff_guid_is_well_formed() {
        assert_eq!(shroudlink_sdk::guid_bytes(LIGHT_BUFF).map(|g| shroudlink_sdk::guid_hash(&g)), Some(2348168016));
    }
}
