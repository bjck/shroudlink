//! Lantern: drinking a Wisp of Light flask also gives a strong light around the player for
//! ten minutes, with the hands free for weapons and tools. The flask's own wisp gives
//! only a faint light. The strong light is the game's own: the hidden buff of the Ring of
//! Light, which attaches a light effect to the chest. The server keeps it up the way the
//! ring does and every client shows it with its own vanilla data, so nobody needs new
//! files, not even the addon. Built from the SDK: game events, `inventory`, `apply_buff`
//! and `on_tick`.

use std::collections::HashMap;

use shroudlink_sdk::{export_mod, log, Events, GameEvent, Host, Known, Mod, Value};

const ID: &str = "lantern";
/// `BuffType` of the Ring of Light's visual (`Visuals_Ring_Of_Light`): slot Buff, apply
/// type ResetTime, 0.3 s lifetime (the ring re-applies it), a light effect on the chest.
const LIGHT_BUFF: &str = "b2f8db72-a69d-43db-baf1-c69937ec0264";
/// `Consumable_Potion_Flask_Wisp_Of_Light`
const WISP_FLASK: u32 = 2239380894;
/// How long one flask lights; drinking another starts the time again.
const LIGHT_SECONDS: f64 = 600.0;

#[derive(Default)]
pub struct Lantern {
    /// Slots that held a flask at the last tick, per player. The consume event only names
    /// the slot, and the last flask of a stack may be gone by the time it arrives.
    flask_slots: HashMap<u32, Vec<u64>>,
}

impl Lantern {
    /// Was the consumed item in `slot` a Wisp of Light flask?
    fn was_flask(&self, events: &Events, player: u32, slot: u64) -> bool {
        let now = events.world().inventory(player).into_iter().find(|s| s.id == slot);
        match now {
            Some(s) if s.count != 0 => s.item == WISP_FLASK,
            _ => self.flask_slots.get(&player).is_some_and(|slots| slots.contains(&slot)),
        }
    }
}

impl Mod for Lantern {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_message(&mut self, _host: &mut Host, _client: u64, _message: &Value) {}

    fn game_events(&self) -> Vec<&'static str> {
        vec!["ConsumeItemEvent"]
    }

    fn on_game_event(&mut self, events: &Events, event: &GameEvent) {
        let Some(Known::ConsumeItem { player, slot }) = event.known() else { return };
        if self.was_flask(events, player, slot) && events.world().apply_buff(player, LIGHT_BUFF, LIGHT_SECONDS) {
            log!("lantern: player {player} drank a Wisp of Light, light for {LIGHT_SECONDS} s");
        }
    }

    fn on_tick(&mut self, host: &mut Host) {
        let world = host.world();
        self.flask_slots = world
            .players()
            .into_iter()
            .map(|p| (p, world.inventory(p).into_iter().filter(|s| s.count != 0 && s.item == WISP_FLASK).map(|s| s.id).collect()))
            .collect();
    }
}

export_mod!(Lantern);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_buff_guid_is_well_formed() {
        assert_eq!(shroudlink_sdk::guid_bytes(LIGHT_BUFF).map(|g| shroudlink_sdk::guid_hash(&g)), Some(2348168016));
    }

    #[test]
    fn listens_to_consumed_items() {
        let m = Lantern::default();
        assert_eq!(m.game_events(), vec!["ConsumeItemEvent"]);
        assert!(shroudlink_sdk::events::hash_of("ConsumeItemEvent").is_some());
    }
}
