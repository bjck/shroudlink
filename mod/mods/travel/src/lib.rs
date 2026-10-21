//! Travel: a MODS tab window that teleports the player to the marker they set on the map,
//! from anywhere, the shroud included. Built only from the SDK: `player_of` (which player
//! clicked), `component` (their `PlayerWaypoint`: enabled + WorldPosition) and `teleport`.

use std::collections::HashMap;

use shroudlink_sdk::components::world_position;
use shroudlink_sdk::ui::{Event, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{export_mod, log, Host, Mod, Value};

const ID: &str = "travel";
/// Height added to the marker so the player lands on, not in, the ground.
const LANDING_HEIGHT: f64 = 2.0;

#[derive(Default)]
pub struct Travel {
    /// last result shown per client
    status: HashMap<u64, String>,
}

/// The map marker of a player: (enabled, position in metres).
fn marker(host: &Host, player: u32) -> Option<(bool, [f64; 3])> {
    let data = host.world().component(player, "PlayerWaypoint")?;
    Some((*data.first()? != 0, world_position(&data, 0x8)?))
}

impl Travel {
    fn window(&self, host: &Host, client: u64) -> ToClient {
        let world = host.world();
        let mut content = vec![Node::Text { text: "Set a marker on the map, then travel there - also from inside the shroud.".into() }];
        match world.player_of(client) {
            None => content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() }),
            Some(player) => {
                let here = world.position(player).map_or("?".into(), |p| format!("{:.0}, {:.0}, {:.0}", p[0], p[1], p[2]));
                content.push(Node::Text { text: format!("You are at {here}") });
                match marker(host, player) {
                    Some((true, m)) => {
                        content.push(Node::Text { text: format!("Map marker at {:.0}, {:.0}, {:.0}", m[0], m[1], m[2]) });
                        content.push(Node::Button { id: "go".into(), label: "Travel to the marker".into() });
                    }
                    _ => content.push(Node::Text { text: "No map marker set.".into() }),
                }
            }
        }
        if let Some(status) = self.status.get(&client) {
            content.push(Node::Text { text: status.clone() });
        }
        content.push(Node::Button { id: "refresh".into(), label: "Refresh".into() });
        ToClient::Open { owner: ID.into(), window: Window { id: "main".into(), title: "Travel".into(), content } }
    }

    fn travel(&mut self, host: &Host, client: u64) -> String {
        let world = host.world();
        let Some(player) = world.player_of(client) else { return "Unknown player: open a chest first.".into() };
        let Some((true, m)) = marker(host, player) else { return "No map marker set.".into() };
        let to = [m[0], m[1] + LANDING_HEIGHT, m[2]];
        if world.teleport(player, to) {
            log!("travel: player {player} to the map marker {to:?}");
            format!("Travelled to {:.0}, {:.0}, {:.0}.", to[0], to[1], to[2])
        } else {
            "The teleport failed.".into()
        }
    }
}

impl Mod for Travel {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        let entries = ToClient::Entries { owner: ID.into(), entries: vec![MenuEntry { id: "open".into(), label: "Travel".into() }] };
        host.send(client, &entries.to_message());
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        match (node.as_str(), event) {
            ("open", Event::Menu) | ("refresh", Event::Click { .. }) => {}
            ("go", Event::Click { .. }) => {
                let status = self.travel(host, client);
                self.status.insert(client, status);
            }
            _ => return,
        }
        let window = self.window(host, client);
        host.send(client, &window.to_message());
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.status.remove(&client);
    }
}

export_mod!(Travel);

#[cfg(test)]
mod tests {
    use super::*;
    use shroudlink_sdk::testing::MockHost;

    #[test]
    fn without_a_known_player_the_window_says_how_to_fix_it() {
        let mut m = Travel::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        assert_eq!(host.take_sent()[0].1["op"], "entries");
        let menu = UiEvent { window: String::new(), node: "open".into(), event: Event::Menu };
        m.on_message(&mut host.host(), 1, &menu.to_message(ID));
        let open = &host.take_sent()[0].1;
        assert!(open["window"].to_string().contains("Open any chest once"));
        let go = UiEvent { window: "main".into(), node: "go".into(), event: Event::Click { index: None } };
        m.on_message(&mut host.host(), 1, &go.to_message(ID));
        assert!(host.take_sent()[0].1["window"].to_string().contains("Unknown player"));
    }
}
