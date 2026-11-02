Bigger stacks (x10) and bigger chests (x2) as EML mods
======================================================
Chests get twice the slots (max 128), and items that stack hold ten times as many.
These are two mods for EML, the Enshrouded Mod Loader by Brabb3l. They change the
game's data when EML runs; there is no DLL and no script in this zip.

!!! READ THIS BEFORE YOU USE IT !!!
The bigger stacks and chests only exist while EML is running. If the server ever loads
your world without them (EML missing, or broken by a game update), the game DELETES
everything above the normal stack size and everything in the extra chest slots.
Characters that join such a server can lose the same from their backpack.
  - Keep the host's AUTO UPDATE OFF.
  - Before every game update: stop the server and back up the "savegame" folder.
  - After a game update, make sure EML still works before you start your world.
  - Players: before joining after an update, back up your characters
    (Steam\userdata\<your id>\1203620\remote\).

INSTALL (server with EML)
1. Install EML for the dedicated server (see EML's own instructions).
2. Copy the folders chestsort_bigger_stacks and chestsort_bigger_storage into EML's
   "mods" folder (next to your other EML mods).
3. Start the server. EML's log should say "multiplied the stack size of ... items".
Tested together with shroudlink on a dedicated server (game build 26319703).

NO EML ON YOUR HOST?
The same two mods can be patched into the server's data files once per game build,
with a safe update script for game hosts with SFTP. That script downloads tools from
the internet, so it is only on GitHub: shroudlink-bigger-stacks-chests.zip at
https://github.com/bjck/shroudlink/releases/latest

Optional: shroudlink and its mods work the same without bigger stacks and chests.
