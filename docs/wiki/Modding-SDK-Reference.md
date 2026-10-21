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

Mods export `shroudlink_mod_v2() -> *const ModDecl`. `HostApi` and `ModDecl` are
`#[repr(C)]` structs in `sdk/src/lib.rs`. Messages cross the boundary as UTF-8 JSON with
a length, and game calls are plain C functions taking the opaque `game` pointer of the
current callback.

- v2 appended its fields to the v1 layout, and the host still loads v1 mods
  (`shroudlink_mod_v1`); they get no inventory events.
- New host functions are only ever appended.
