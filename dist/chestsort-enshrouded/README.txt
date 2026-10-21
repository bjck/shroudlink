chestsort - Enshrouded server mod
=================================
- Chests sort by name: rename a chest to e.g. "Food, Wood", "Spells" or "Ores & Gems", then
  press the stack-to-chest button or use a Quick Stack Station. Every item of those
  categories goes into that chest.
- Chests have 2x the slots (max 128), and items that stack hold 10x as many.

Categories and the words that work in chest names are in chestsort_categories.json
on the server. Edits apply within seconds; mistakes are reported in shroudlink.log.


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