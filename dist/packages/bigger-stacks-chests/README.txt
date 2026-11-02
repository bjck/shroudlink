Bigger stacks (x10) and bigger chests (x2) for shroudlink servers
==================================================================
Chests get twice the slots (max 128), and items that stack hold ten times as many.
These are patched game data files (made with EML by Brabb3l), not a DLL, and they are
tied to ONE exact game build.

!!! READ THIS BEFORE YOU USE IT !!!
A game update brings back the normal data files. If the server loads your world on them,
the game DELETES everything above the normal stack size and everything in the extra
chest slots. Characters that join such a server can lose the same from their backpack.
  - Keep the host's AUTO UPDATE OFF.
  - Before every game update: stop the server and back up the "savegame" folder.
  - Update with UPDATE-SERVER.bat: it backs up the save, builds the patched files for the
    new build, keeps the world away while the server updates and uploads everything.
  - Safety stop: UPDATE-SERVER.bat also uploads shroudlink_data.txt. If the server then
    starts with your world on other data files, the shroudlink core stops it before the
    world is loaded (only with the shroudlink core installed).
  - Players: before joining after an update, back up your characters
    (Steam\userdata\<your id>\1203620\remote\).

WHAT IS IN HERE
  UPDATE-SERVER.bat  the safe update for a game host with SFTP (written for Game Host
                     Bros; it tells you when to click what in the panel). It also uploads
                     the shroudlink core and mods if they are unpacked next to it
                     (core\steam_api64.dll, shroudlink_mods\*.dll).
  repatch\REPATCH.bat  only builds the patched files for the current build into
                     upload-build-<build>, for uploading by hand.
Both download SteamCMD, the dedicated server (~8 GB) and the EML patcher (checked).
Windows only.

Optional: shroudlink and its mods work the same without bigger stacks and chests.

More: https://github.com/bjck/shroudlink/blob/main/docs/wiki/Server-Game-Updates.md
