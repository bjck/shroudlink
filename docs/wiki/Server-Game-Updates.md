# Game updates

## What an update changes

| Part | After a game update |
|---|---|
| **Core and mods** (`steam_api64.dll`, `shroudlink_mods/`) | usually keep working: functions are found by byte signatures. If one is no longer found, **only that feature** switches off (see `shroudlink_hooks.txt`). |
| **Data mods** (`.kfc`, `.kfc_resources`) | **must be rebuilt for every build.** Files for another build can crash the server. |
| **Your world** | at risk, see below |

## The danger

If the server loads your world with **vanilla** data files, the game cuts every stack
above the vanilla limit and **deletes the rest**. The server log then shows lines like
`[inventory] Deleted 12084 items from stack of Block_T1_Wood_Rough_CRAFTING`. Items in
the extra chest slots are at risk too.

An update replaces the data files with vanilla ones. So **never start the server with
your world on a new build before the patched data files are uploaded.** Keep the host's
auto update off.

## The safe way: `UPDATE-SERVER.bat`

From the server package, run `UPDATE-SERVER.bat` on a Windows PC. It stops and tells you
whenever something has to be clicked in the panel:

1. You stop the server.
2. It downloads the savegame as a backup and checks the file count and size.
3. It downloads the newest dedicated server with SteamCMD (about 8 GB the first time)
   and builds the patched data files with EML.
4. If your server is older, it **parks your world** (renames `savegame`). You switch
   auto update on, start the server once so that it updates and creates an empty world,
   and stop it again.
5. It puts your world back.
6. It uploads the patched data files, the core and the chestsort mod. Replaced files are
   renamed, never deleted.
7. You switch auto update **off** and start the server. If the console shows "Deleted …
   items", stop it immediately; the backup is in `backups\`.

## Check a new build first (developers)

```
python research/scripts/checksigs.py <new enshrouded_server.exe> [--client <client dump>]
```

This shows which features survive the update before anything is deployed. Then run
`python research/scripts/selftest.py` against a local server on the new build (see
[[Testing|Modding-Testing]]).

## If it went wrong

1. Stop the server.
2. Rename its `savegame` folder, e.g. to `savegame.broken`.
3. Upload your backup as `savegame`.
4. Upload the patched data files for the server's **current** build, then start it.
