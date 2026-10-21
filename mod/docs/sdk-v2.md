# shroudlink SDK v2: server events and game API

Status: phase 1 (server events and game API) and phase 2 (client building blocks: slot
keys, slot marks, per-server storage, backpack layout; see `link/src/ui.rs`) are done.
Next: panel sections and notifications on the client, and a hook registry so features
switch off individually when a game update breaks one hook.

## Goal

A server mod should say *what* it wants ("keep these slots", "move these stacks into
this chest") without knowing game addresses, signatures or struct offsets. The core
(`shroudlink_server.dll`, installed as `steam_api64.dll`) owns every hook; mods are
DLLs in `shroudlink_mods/` written against the SDK. chestsort itself becomes such a mod.

## Layers

```
 game  <-- hooks -->  core (shroudlink_server.dll)  <-- C ABI v2 -->  mods (shroudlink_mods/*.dll)
                      - channel to client addons                      - chestsort.dll
                      - inventory events                              - hello_mod.dll
                      - game API (inventories, items, names)          - your mod
                      - equipment protection (always on)
```

## Inventory events

The core hooks every way the game moves a whole inventory:

| Event | Game trigger | from -> to |
|---|---|---|
| `QuickStack` | hold R / F at a chest, Quick Stack Station (once per chest) | player -> chest **or** chest -> player |
| `MoveAll` | tap R / F ("move all") | either direction |

For each event the core runs, in order:

1. **`keep`**: every mod may name slots of `from` that must not move (chestsort: the
   player's locked slots). The core always adds the equipped items and the item in hand
   when `from` is a player.
2. **`on_transfer`**: every mod may move stacks itself through the game API before the
   game does (chestsort: category sorting into named chests). Kept slots cannot be moved
   by any mod.
3. The game's own transfer runs. Kept slots are hidden from it.

Mods see the event as `{ kind, from, to }` plus a `Game` handle that is only valid during
the callback.

## Game API (`Game`, valid inside inventory callbacks)

| Call | Returns |
|---|---|
| `slots(entity)` | every inventory slot of an entity: `Slot { id, item, count }`; empty slots have `count == 0` |
| `is_player(entity)` | whether the entity is a player |
| `chest_name(entity)` | the name given with the Rename button |
| `max_stack(item)` | the stack limit of an item (after data patches) |
| `move_stack(slot, to_entity)` | moves one stack into the entity's inventory, filling partial stacks first; returns how many items moved; refuses kept slots |
| `send(client, json)` | queues a message to a client addon (delivered after the event) |

A slot id is `page entity | slot index << 32`, the same on client and server within
a session.

## Messages and lifecycle (unchanged from v1)

`on_join` / `on_message` / `on_leave` per client addon session, `Host::send`,
declarative UI (`shroudlink::ui`). New: `on_load` (once, before anything else) and a
global `shroudlink_sdk::log!` that writes to the server log.

## ABI

- Mods export `shroudlink_mod_v2` returning a `ModDecl` (v1 fields first, then
  `on_load`, `keep`, `on_transfer`). The host still loads `shroudlink_mod_v1` DLLs;
  they simply get no inventory events.
- `HostApi` v2 keeps the v1 fields first and adds the game functions, so a v1 mod that
  receives a v2 table keeps working.
- All game calls take the opaque `game` pointer of the current callback; calling them
  outside a callback is an error (they return nothing).

## Developer loop

- **Hot reload:** the core loads each mod from a shadow copy, so the DLL in
  `shroudlink_mods/` can be overwritten at any time. Within two seconds the core loads the
  new build, replaces the old one and re-runs `on_join` for every connected client. The old
  copy stays loaded but unused, so a game restart reclaims its memory.
- **Tests without the game:** `shroudlink_sdk::testing::MockHost` captures sent messages,
  so a mod's UI logic can be unit-tested (see `mods/chestsort`).
- **Self-test in the real server:** `research/scripts/selftest.py` starts the local
  dedicated server with a marker file. Once the world has loaded, the core builds a game
  context from a world system's descriptor and runs its tests on the game thread. The tests
  cover a fake addon session, and quick stack plus move all between two inventories with test
  stacks, a test chest name and a lock. The report is `shroudlink_selftest.txt`, and the
  savegame is restored afterwards.

## Files

| Path | What |
|---|---|
| `sdk/` | the SDK (`Mod`, `Game`, `export_mod!`, `testing`) |
| `server/src/events.rs` | inventory hooks and dispatch |
| `server/src/game_api.rs` | the C functions behind `Game` |
| `server/src/plugins.rs` | loader (v1 + v2), shadow copies, hot reload |
| `mods/chestsort/` | chestsort as a mod: categories, sorting, locks, UI |
