shroudlink - mod framework for Enshrouded dedicated servers
============================================================
This is the framework itself. It loads the mods in the folder shroudlink_mods into your
dedicated server and gives them a MODS tab in the game's player menu. On its own it
does nothing visible: add the mods you want (shroudlink-mod-*.zip).

INSTALL (server, once)
1. Stop the server and copy its "savegame" folder somewhere safe.
2. In the server folder (where enshrouded_server.exe is), rename steam_api64.dll
   (Steam's original, about 300 KB) to steam_api64_o.dll.
   Skip this if steam_api64_o.dll is there already.
3. Copy core\steam_api64.dll from this zip into the server folder (about 1 MB).
4. Unpack the mods you want into the server folder: they go into shroudlink_mods.
5. Start the server. shroudlink.log in the server folder should say
   "shroudlink server ... starting" and "mod '...' registered" for every mod.

UPDATE: stop the server, replace steam_api64.dll with the new core\steam_api64.dll,
start it. Never replace steam_api64_o.dll.

PLAYERS: the player addon (shroudlink-client.zip, dinput8.dll next to enshrouded.exe)
adds the MODS tab, search in chests and slot locks. It is optional.

Game host with SFTP: upload a file as name.new, rename the old one, then rename
name.new into place. Nothing has to be deleted.

More: https://github.com/bjck/shroudlink  -  https://www.nexusmods.com/enshrouded/mods/278
