shroudlink - Enshrouded server mods
===================================
Server side (everyone, no addon needed):
- Chests sort by name: rename a chest to e.g. "Food, Wood", "Spells" or "Ores & Gems",
  then press the stack-to-chest button or use a Quick Stack Station. Every item of
  those categories goes into that chest.
- Action bar and equipped items are never put into chests.
- Chests have 2x the slots (max 128), and items that stack hold 10x as many.

With the client addon (shroudlink-client.zip) players also get:
- item search in chests, slot locks (L), working right after joining
- MODS tab: Chest categories, Storage (all chests within 40 m as one box)

Files on the server (next to enshrouded_server.exe):
  steam_api64.dll        the shroudlink core (Steam's original renamed to steam_api64_o.dll)
  shroudlink_mods\*.dll  the mods (chestsort, storage); remove one to switch it off
  chestsort_categories.json   categories; edits apply within seconds
  shroudlink.log         what the core and mods did

WHEN A GAME UPDATE COMES OUT
----------------------------
Keep auto update OFF in the panel. NEVER start the server with your world on the
new build before the patched files are uploaded: the game then cuts every stack
above the normal size and deletes the rest (seen 2026-10-21: 12,000 wood gone).

Run UPDATE-SERVER.bat on a Windows PC. It backs up the savegame, builds the patched
files for the new build, parks your world while the server updates, puts it back,
uploads everything and tells you exactly when to click what in the panel.

If something goes wrong: the backup is in backups\savegame_<date>. Stop the server,
rename its "savegame" folder and upload the backup as "savegame".
