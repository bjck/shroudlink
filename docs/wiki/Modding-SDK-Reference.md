# SDK reference

Crate `shroudlink-sdk` (`mod/sdk`). Design notes are in `mod/docs/sdk-v2.md`.

## The `Mod` trait

```rust
pub trait Mod: Send {
    fn id(&self) -> &'static str;                       // routes messages: {"mod": id, ...}
    fn on_load(&mut self) {}                            // once, after the DLL was loaded
    fn on_join(&mut self, host: &mut Host, client: u64) {}
    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {}
    fn on_leave(&mut self, host: &mut Host, client: u64) {}
    fn keep(&mut self, game: &Game, event: &InventoryEvent) -> Vec<u64> { vec![] }
    fn on_transfer(&mut self, game: &Game, event: &InventoryEvent) -> u32 { 0 }
    fn on_tick(&mut self, host: &mut Host) {}          // about once a second (core v5)
}
export_mod!(MyMod);   // MyMod: Mod + Default
```

`client` is an addon session: one player with the addon, for as long as they stay
connected. Players without the addon never appear here, but they do trigger inventory
events.

## Messages: `Host`

| Call | |
|---|---|
| `host.send(client, &message)` | a JSON message to that player's addon. UI messages come from `shroudlink_sdk::ui` (`ToClient::….to_message()`) |
| `host.log(text)` | a line in `shroudlink.log` (or `shroudlink_sdk::log!`) |

## Inventory events

```rust
pub struct InventoryEvent { pub kind: TransferKind, pub from: u32, pub to: u32 }
pub enum TransferKind { QuickStack, MoveAll }
```

| `kind` | Game trigger | Direction |
|---|---|---|
| `QuickStack` | hold R / F at a chest; a Quick Stack Station (once per chest in range) | player → chest **or** chest → player |
| `MoveAll` | tap R / F | either |

Always check `game.is_player(event.from)` before treating `from` as the player.

Order per event:
1. **`keep`** from every mod. The core always adds the player's equipped items, the
   item in hand and the action bar.
2. **`on_transfer`** from every mod. Use `game.move_stack` here; kept slots never move.
3. The game's own transfer runs, with the kept slots hidden from it.

## Game events

Every event the server's ECS raises: 130 types, from `EntityDiedEvent`, `PickupEvent`,
`LevelUpEvent`, `XPGainedEvent`, `CraftRecipeSuccessEvent`, `BuildingPlaceEvent` and
`TeleportPlayerEvent` to `HitEvent`.

```rust
fn game_events(&self) -> Vec<&'static str> {
    vec!["EntityDiedEvent", "LevelUpEvent", "PickupEvent"]
}

fn on_game_event(&mut self, events: &Events, event: &GameEvent) {
    match event.known() {
        Some(Known::LevelUp { player, level }) => { /* … */ }
        Some(Known::Pickup { item, count, by, .. }) => { /* … */ }
        _ => {}
    }
    // events.send(client, &message) queues a message to an addon
}
```

- `event.known()` decodes the common types: `Login`, `EntityDied`, `Pickup`, `LevelUp`,
  `XpGained`, `ConsumeItem`, `CraftRecipeSuccess`, `BuildingPlace`, `BuildingTearDown`,
  `TeleportPlayer`, `RevivedPlayer`, `UnlockSkill`, `TrashItem`.
- Read any other type from `event.data` with `u32_at`, `f32_at` and `slot_at`, using the
  layout from the game's reflection data.
- The full list is `shroudlink_sdk::events::EVENT_TYPES`.
- Events are delivered once per occurrence, on the game thread, before the game's own
  handlers. Only subscribed types are decoded at all.
- Players without the addon cause events too: these are server events, not addon
  messages.

## The world (any callback)

`host.world()`, `game.world()` or `events.world()`; these need a core with HostApi v4.

| Call | Returns |
|---|---|
| `players()` | entity ids of the players in the world |
| `position(entity)` | position in metres `[x, y (up), z]` |
| `teleport(entity, [x, y, z])` | moves it; a player sees itself there right away |
| `player_of(client)` | the player entity of an addon session (known once that player has opened a chest screen this session) |
| `component(entity, "TypeName")` | the raw bytes of any of the 686 component types (`shroudlink_sdk::components::COMPONENT_TYPES`); field offsets are in the reflection data |
| `set_component(entity, "TypeName", &bytes)` | overwrites a component on the server (exact size); whether clients see it depends on the component's replication |
| `inventories_near(entity, radius)` | entities with an inventory (chests, stations) around an entity; only the world around players is loaded |
| `inventory(entity)` | every slot of any entity's inventory |
| `move_stack(from, slot, to)` | moves a stack between two inventories outside any player action; a player's equipment, item in hand and action bar are never taken nor filled |
| `apply_buff(entity, "guid")` | gives an entity a vanilla buff by the Guid of its `BuffType`, the way an item or spell would (core v5, experimental). Clients show its effects from their own data, so no client files are needed |

`components::world_position(&bytes, offset)` reads a `keen::WorldPosition` in metres.
`mods/storage` shows everything in the chests around the player as one grid and fetches
an item from all of them with one click.
`mods/travel` is a complete example: it reads the player's `PlayerWaypoint` (the map
marker) and teleports there. `mods/lantern` keeps the Ring of Light's buff (a light on
the chest) on a player from `on_tick`.

## Game API: `Game` (only inside `keep` / `on_transfer`)

| Call | Returns |
|---|---|
| `game.slots(entity) -> Vec<Slot>` | every inventory slot (pages × 8, empty ones too) |
| `game.is_player(entity) -> bool` | |
| `game.chest_name(entity) -> Option<String>` | the name given with Rename |
| `game.max_stack(item) -> Option<u32>` | the stack limit (after data patches) |
| `game.move_stack(slot, to) -> u32` | moves one stack into `to` (partial stacks first, then empty slots); returns the items moved |
| `game.send(client, &message)` | queues a message to an addon; it is delivered after the event |

```rust
pub struct Slot { pub id: u64, pub item: u32, pub count: u32 }
// slot.page() = page entity, slot.index() = 0..8
```

A **slot id** is `page entity | slot index << 32`. It is the same on server and client
within one session, but entity ids change between sessions. To remember slots across
sessions, store **positions** (see [[UI and building blocks|Modding-UI-and-Building-Blocks]]).

## Files and paths

- `shroudlink_sdk::server_dir()` is the server's folder. Put your mod's files there,
  e.g. `my_mod.json`.
- Mods run in the server's game thread. Keep callbacks short, and do not block.

## The C ABI (other languages)

Mods export `shroudlink_mod_v2() -> *const ModDecl`; `decl.version` says the layout
(3 = with game events, 5 = with `on_tick`). `HostApi` and `ModDecl` are
`#[repr(C)]` structs in `sdk/src/lib.rs`. Messages cross the boundary as UTF-8 JSON with
a length, and game calls are plain C functions taking the opaque `game` pointer of the
current callback.

- v2 appended inventory events to the v1 layout, v3 appended game events, and v5
  appended `on_tick` (`HostApi` v5: `apply_buff`). The host still loads older mods; they
  get no callbacks their version does not know.
- New host functions are only ever appended.
