# Server installation

shroudlink runs inside `enshrouded_server.exe` on Windows and on Linux hosts that use Wine
or Proton (tested on Game Host Bros, which runs Pterodactyl with Wine).

## What goes where

All paths are relative to the folder that contains `enshrouded_server.exe`.

| File | What it is |
|---|---|
| `steam_api64.dll` | the shroudlink core (replaces Steam's DLL and passes every call on to it) |
| `steam_api64_o.dll` | Steam's original `steam_api64.dll`, renamed |
| `shroudlink_mods/*.dll` | the mods (chestsort, storage, meter, assistants, gemforge); remove one to switch it off |
| `chestsort_categories.json` | the categories; created with the defaults if missing |
| `enshrouded_server.kfc`, `enshrouded_server.kfc_resources` | optional data mods (chests ×2, stacks ×10), **tied to one game build** |

## Steps

1. **Stop the server** and **back up the `savegame` folder**.
2. Rename the server's `steam_api64.dll` (Steam's original, about 300 KB) to
   `steam_api64_o.dll`. Skip this if `steam_api64_o.dll` already exists.
3. Upload the core as `steam_api64.dll` (about 1 MB): `core\steam_api64.dll` from
   `shroudlink-core.zip`, or `repatch\steam_api64.dll` from the full package
   `chestsort-enshrouded.zip`.
4. Upload the mods you want into the folder `shroudlink_mods/`: each
   `shroudlink-mod-<name>.zip` holds one, the full package all of them. On a first install
   the full package's `chestsort_categories.json` can go along; the mods create it with the
   defaults otherwise, and an update keeps your own.
5. Optional: upload the data mod files for **exactly your server's build**. See
   [[Game updates|Server-Game-Updates]] for how to build them.
6. Start the server and check `shroudlink.log`. It should contain:
   ```
   shroudlink server 0.7.1 starting (loaded as steam_api64.dll)
   inventory hooks installed
   shroudlink: loaded mod 'chestsort' from ...
   shroudlink: mod 'chestsort' registered
   ... (one pair for every mod)
   features: channel on, inventory on, move_all on, game_events on, ...
   ```
   A feature that says `off` lost its hook point in a game update: only that feature
   is switched off (see `shroudlink_hooks.txt`).

On a host where you upload by SFTP, replace a running file by uploading it as `x.new`,
renaming the old one, then renaming `x.new` into place. Nothing has to be deleted.

## Why `steam_api64.dll`?

The core must be loaded into the server process. The game ships `steam_api64.dll`, and
the core takes its place and forwards every Steam call to the original. That works on
Windows and under Wine or Proton without DLL overrides. On plain Windows the core also
works as `crashhandler64.dll` (Steam probes for it) or `dinput8.dll`.

## Auto update

**Keep the host's auto update switched OFF.** A game update replaces the data files with
vanilla ones. If the server then loads your world, the game deletes everything above the
vanilla stack size. See [[Game updates|Server-Game-Updates]].
