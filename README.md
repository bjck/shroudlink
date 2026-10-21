# shroudlink

A mod framework for Enshrouded 1.0 dedicated servers: server mods talk to an optional
client addon over the game's own netcode and show native in-game UI. Players without the
addon play normally. Ships with **chestsort**, the mod it was built for.

## chestsort

**Server (`steam_api64.dll` proxy)**
- Chests sort by name: rename a chest to e.g. "Food, Wood" and the deposit button or a
  Quick Stack Station puts every item of those categories into it
  (`chestsort_categories.json`, reloaded live).
- Equipped items and the item in hand are never deposited.
- Optional EML data mods: chests x2 slots, stacks x10.

**Client addon (`dinput8.dll`)**
- MODS tab with the server's mod windows (chest categories, item search).
- Item search in chests (`/`).
- Lock backpack slots with `L` (gold rim): deposits never take locked items.

## Downloads
See the releases: `shroudlink-client.zip` (players) and `chestsort-enshrouded.zip`
(server owner: mod, update script, data patches).

## Layout
| Path | Content |
|---|---|
| `mod/` | Rust workspace: `server`, `client`, `link` (protocol), `sdk` + `examples/hello_mod` (plugin API), `common`, `eml` (data mods) |
| `mod/MODDING.md` | How to write your own shroudlink mod |
| `dist/chestsort-enshrouded/` | Server package: README, `UPDATE-SERVER.bat` (safe game updates), `repatch/` |
| `research/scripts/` | Reverse-engineering helpers (Ghidra scripts, signatures, hot reload, screenshots) |

## Building
```
cd mod
cargo build --release
# target/release/chestsort.dll         -> server, install as steam_api64.dll (original renamed to steam_api64_o.dll)
# target/release/shroudlink_client.dll -> client, install as dinput8.dll next to enshrouded.exe
```
Tested with server build 26083076 / client 26082781. Functions are located by byte
signatures, so most game updates need no rebuild; the EML data patches have to be
re-applied for every build (`UPDATE-SERVER.bat`).

**Game updates:** never start the server with your world on a new build before the
patched data files are uploaded - the game deletes everything above the vanilla
stack size. `UPDATE-SERVER.bat` does it in the safe order.

## License
MIT, see [LICENSE](LICENSE). Not affiliated with Keen Games.

## Credits
The data mods (stacks, chest slots) are applied with **EML** by Brabb3l
([kfc-parser](https://github.com/Brabb3l/kfc-parser), GPL-3.0). The scripts download its
`emm.exe` v0.2.0 from the official release and check its SHA-256; it is not bundled here.
