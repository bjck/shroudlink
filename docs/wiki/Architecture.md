# Architecture

```
 server (enshrouded_server.exe)                         player (enshrouded.exe)
 ┌──────────────────────────────────┐                  ┌───────────────────────────────┐
 │ steam_api64.dll  core            │  game netcode    │ dinput8.dll  addon (optional) │
 │  ├─ hook registry (hooks.rs)     │◀────────────────▶│  ├─ channel (link.rs)         │
 │  ├─ inventory events, game API   │ custom-string    │  ├─ native UI (native.rs,     │
 │  ├─ channel host (link_host.rs)  │ replication      │  │   ui.rs): MODS tab, windows│
 │  └─ mod loader, hot reload       │                  │  ├─ building blocks (slots.rs)│
 │      shroudlink_mods/*.dll       │                  │  └─ item search, input.rs     │
 │      (chestsort, hello, …)       │                  └───────────────────────────────┘
 └──────────────────────────────────┘
```

## Workspace (`mod/`)

| Crate | Output | Role |
|---|---|---|
| `server` | `shroudlink_server.dll` → `steam_api64.dll` | core |
| `mods/chestsort` | `chestsort.dll` | the chestsort mod |
| `client` | `shroudlink_client.dll` → `dinput8.dll` | addon |
| `link` | | protocol: packets, frames, chunking, UI and building block messages |
| `sdk` | | mod SDK (C ABI and Rust trait) |
| `common` | | logging, signature scanning, DirectInput proxy |
| `examples/hello_mod` | `hello_mod.dll` | example mod |
| `eml/` | | EML data mods (chests ×2, stacks ×10) |

## The channel

Enshrouded replicates **custom player strings**: the names given with the Rename button.
shroudlink rides on that:

- **Client → server.** The addon sends a rename whose text is a shroudlink packet. The
  server's `set_string` hook consumes it before it reaches the name table.
- **Server → client.** The server's send loop asks `next_pending` for the next string
  per player slot. The core hands out its queued packets first, and `mark_sent` confirms
  them. On the client, the `store` hook consumes them.
- **Packets** are `~sl1~` + base64url, at most 256 characters. A frame is
  `Hello` / `WhoAmI{nonce, slot}` / `Claim{nonce, slot}` / `Data{nonce, msg, chunk, count, bytes}`.
  Messages are JSON, chunked at 166 bytes and reassembled.
- **Sessions.** The server beacons `WhoAmI{0, slot}` to slots without a session every
  30 s. The addon claims the slot with a random nonce. Vanilla clients store the beacon
  once as a harmless string.
- **Routing.** `"mod": "<id>"` routes a message to that mod, and `"mod": "ui"` routes it
  to the addon's renderer.

## Inventory events

The core hooks the game's quick stack (`FUN_1401ad1c0`, both directions, also once per
chest for a Quick Stack Station) and move all (`FUN_1401ad940`). Around each one it:

1. collects `keep` slots from the mods, plus the player's equipment;
2. calls the mods' `on_transfer`;
3. hides kept stacks from the game by zeroing their counts for the duration of the call,
   then restores them.

The game API (`game_api.rs`) reads inventories through the game's own component
accessors, found as calls inside the quick stack function.

## Game events

Events are reserved with `FUN_1405f6bb0(queue, typeHash, size, extra)`. It appends a
record to a per-thread queue `{data, capacity, used}`:
`[u32 typeHash][u16][u8 size][u8 0][payload padded to 8]`. The caller fills the payload
afterwards.

Once per frame, `FUN_1406ba490(scheduler)` walks every queue (array at `scheduler+0xeb9ef0`),
calls the handlers registered per type and empties the queues. The core hooks it and
reads the queues on entry, when every record is complete. Type hashes are the
`qualifiedHash` of the reflected type names.

The dispatcher was found with a hardware write watchpoint on a queue's `used` field: the
only code that sets it to 0.

## Hook registry

All server hook points are in `server/src/hooks.rs`, grouped by feature: `channel`,
`inventory`, `move_all`, `selftest`. Each is a unique byte signature at a function
entry; helpers are taken from the calls inside known functions. A missing point switches
off only its feature. The client uses the same technique (`SIG_*` in `client/src`).

## Addon UI

The addon hooks the game's UI pass and draws with the game's own widget functions:
frames, text, item slots, backgrounds. It inserts a MODS tab into the player menu, rows
into the chest screen's backpack panel, and rims and dimming into the slot renderer.
Mouse clicks are detected against layout rectangles, and keyboard input comes from a
subclassed window procedure.
