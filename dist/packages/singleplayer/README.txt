shroudlink for singleplayer
===========================
The shroudlink mods without a dedicated server: chests that sort themselves by name,
locked slots, chest search, storage, assistants, damage meter and gemforge, in your
own world. When you play alone, your game runs the world itself, so the mods run
inside your game.

WHAT'S IN THIS ZIP
  dinput8.dll           the player addon (the same as in shroudlink-client.zip)
  shroudlink_core.dll   the shroudlink core, loaded by the addon
  shroudlink_mods\      the mods: chestsort, storage, meter, assistants, gemforge

INSTALL (1 minute)
1. In Steam: right-click Enshrouded > Manage > Browse local files.
2. Copy dinput8.dll, shroudlink_core.dll and the shroudlink_mods folder into that
   folder (next to enshrouded.exe).
3. Start the game and load your world. Press K for the MODS tab.

Back up your saves before you try it: chest sorting and the assistants move items.
Steam keeps them in C:\Program Files (x86)\Steam\userdata\<your id>\1203620\remote.

HOW TO USE IT
- Name a chest after what belongs in it ("Food", "Ores & Gems", "Weapons, Armor").
  Quick stack (R at a chest, or a Quick Stack Station) puts those items into it.
  Which name takes which items: MODS tab > Chest categories, or the file
  chestsort_categories.json in the game folder (created on the first start).
- Hover a backpack slot and press L to lock it: locked slots never go into chests.
- In a chest, press / to search it.
- Everything else is in the MODS tab (K).

MORE MODS
Put a mod's .dll into shroudlink_mods and it is loaded within seconds, even while
you play. Remove the file and restart the game to get rid of it.

GOOD TO KNOW
- Joining a server: the core in your game folder stays silent; on a modded server
  you get that server's mods, on any other server nothing changes.
- Hosting a game for friends: the mods run on your PC, and friends only need
  dinput8.dll. This is new and not tested yet; please tell us how it goes.
- Bigger stacks and bigger chests are not part of this zip. They change the game's
  data files, which works differently (see shroudlink-bigger-stacks-chests.zip).
- After a game update the mods may switch themselves off until a new version is
  out; the game keeps working normally.
- If something seems off: shroudlink.log (the core and the mods) and
  shroudlink_client.log (the addon) in the game folder show what happened.

UNINSTALL
Delete dinput8.dll, shroudlink_core.dll and the shroudlink_mods folder. Your world
keeps everything; chest names stay as they are.
