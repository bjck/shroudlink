# Server installation

shroudlink runs inside `enshrouded_server.exe` on Windows and on Linux hosts that use Wine
or Proton (tested on Game Host Bros, which runs Pterodactyl with Wine).

## What goes where

All paths are relative to the folder that contains `enshrouded_server.exe`.

| File | What it is |
|---|---|
| `steam_api64.dll` | the shroudlink core (replaces Steam's DLL and passes every call on to it) |
| `steam_api64_o.dll` | Steam's original `steam_api64.dll`, renamed |
| `shroudlink_mods/chestsort.dll` | the chestsort mod (and any other mods) |
| `chestsort_categories.json` | the categories; created with the defaults if missing |
| `enshrouded_server.kfc`, `enshrouded_server.kfc_resources` | optional data mods (chests ×2, stacks ×10), **tied to one game build** |

## Steps

1. **Stop the server** and **back up the `savegame` folder**.
2. Rename the server's `steam_api64.dll` (Steam's original, about 300 KB) to
   `steam_api64_o.dll`. Skip this if `steam_api64_o.dll` already exists.
3. Upload the core from the server package as `steam_api64.dll` (about 1 MB).
4. Create the folder `shroudlink_mods/` and upload `chestsort.dll` into it.
5. Optional: upload the data mod files for **exactly your server's build**. See
   [[Game updates|Server-Game-Updates]] for how to build them.
6. Start the server and check `shroudlink.log`. It should contain:
   ```
   inventory hooks installed
   shroudlink host ready (protocol 1) ...
   shroudlink: loaded mod 'chestsort' from ...
   features: channel on, inventory on, move_all on, selftest on
   ```

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
