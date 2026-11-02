Bigger stacks (x10) and bigger chests (x2) as EML mods
======================================================
Chests get twice the slots (max 128), and items that stack hold ten times as many.
These are two mods for EML, the Enshrouded Mod Loader by Brabb3l. EML changes the
game's data while the game or server starts; there is no DLL and no script in this zip.

!!! READ THIS BEFORE YOU USE IT !!!
The bigger stacks and chests only exist while EML is running. If your world is ever
loaded without them (EML missing, or broken by a game update), the game DELETES
everything above the normal stack size and everything in the extra chest slots.
Characters that join such a server can lose the same from their backpack.
  - Keep the host's AUTO UPDATE OFF.
  - Before every game update: stop the server and back up the "savegame" folder.
  - After a game update, check that EML still works (see "Did it work?") before you
    let anyone load the world.
  - Players: before joining after an update, back up your characters
    (Steam\userdata\<your id>\1203620\remote\).

STEP 1: INSTALL EML
EML itself: https://github.com/Brabb3l/kfc-parser/releases (tested with v0.2.0)
Beginner's guide on Nexus (players and dedicated servers):
  https://www.nexusmods.com/enshrouded/mods/135
In short, on a dedicated server:
  1. Copy EML's dbghelp.dll next to enshrouded_server.exe.
  2. Create a folder named "mods" there (EML's mods folder).
  3. Linux host (Wine/Proton): start the server with
     WINEDLLOVERRIDES="dbghelp=native,builtin"
Already loading EML another way (e.g. ModLoader Proxy,
https://www.nexusmods.com/enshrouded/mods/162)? Keep it; only step 2 matters.

STEP 2: ADD THESE MODS
Copy the two folders chestsort_bigger_stacks and chestsort_bigger_storage into EML's
"mods" folder, next to your other EML mods. Do NOT put them into shroudlink_mods: that
folder is only for shroudlink's own .dll mods.

DID IT WORK?
Start the server and open the newest *.eml.log in its "logs" folder. It should say
  Bigger Stacks: multiplied the stack size of ... items
  Applied ... patches
Tested on a dedicated server with game build 26319703, together with shroudlink.

SINGLEPLAYER
Same steps in the game folder (next to enshrouded.exe). If you also use shroudlink there,
take EML's dbghelp.dll, never EML's dinput8.dll: the shroudlink addon is dinput8.dll.
Not tested in singleplayer yet.

NO EML ON YOUR HOST?
The same two mods can be patched into the server's data files once per game build,
with a safe update script for game hosts with SFTP. That script downloads tools from
the internet, so it is only on GitHub: shroudlink-bigger-stacks-chests.zip at
https://github.com/bjck/shroudlink/releases/latest

Optional: shroudlink and its mods work the same without bigger stacks and chests.
