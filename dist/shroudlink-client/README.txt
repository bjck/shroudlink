Enshrouded client addon (shroudlink)
====================================
Adds a few comfort features to the game. Purely optional: you can join our server
with or without it, and everything else (chest sorting, bigger chests and stacks)
works for everyone either way.

WHAT YOU GET
- Search in chests: open any chest, click "SEARCH ( / )" next to the BACKPACK title
  (or just press /) and type. Items that don't match turn grey, live while you type.
  The search stays active while you go from chest to chest.
    Esc  = clear the search (press again to leave the field)
    X    = clear and close the search
- Lock items: hover a slot of your backpack and press L (in a chest or in your
  inventory). Locked slots get a gold rim and are never put into chests - not
  by R, not by holding R, not by a Quick Stack Station. Press L again to unlock.
  The lock belongs to the slot, not the item: move the item and the lock stays.
  Locks are kept per server and work right after joining.
  Your action bar and equipped items are never put into chests either.
- MODS tab in the menu (next to SKILLS, only on our server). Press K to open it,
  also while playing. Long pages scroll with the mouse wheel; item slots show the
  game's own tooltip; messages from the mods pop up at the top (in menus at the
  bottom; click one to make it go away).
    Assistants - your survivors work for you. Stand near them at your base, pick
              one, and use the tabs:
                Orders - click a product and "Make 5" (or type an amount): they
                         run their stations with what is in the chests nearby
                         and put the results back. Missing ingredients that
                         another station at the base makes are made first.
                Jobs   - standing jobs: the Farmer harvests and replants the
                         fields and keeps the animal feeders filled; the Fisher
                         fishes with bait from the chests (choose the bait order)
                Crops  - what grows around the Farmer, and the seedlings he keeps
                Notifications - switch their messages off
    Chest categories - which chest name takes which items, item search
    Storage - everything in the chests within 40 m as one box, page by page;
              click an item to take it from all of them
    Damage meter - damage dealt, damage taken and healing for everyone, for
              the last fight and the whole session, and what killed you.
              Other players see your Steam name there.
              During and after a fight a small panel in the bottom right shows
              everyone's damage as bars. Open any menu (e.g. B) to drag it
              somewhere else; switch it off on the Damage meter page.
  Don't want one of them? Click ON next to it in the list to switch it off on
  your PC: its windows, panels, rims and keys disappear for you (on the server it
  keeps running, e.g. your locks still protect your slots). Click OFF to get it back.

If two players join at the very same moment, the locks of one of them may only
show after opening a chest once.

INSTALL (1 minute)
1. In Steam: right-click Enshrouded > Manage > Browse local files.
2. Copy dinput8.dll into that folder (next to enshrouded.exe).
3. Start the game. That's it.

UNINSTALL
Delete dinput8.dll from the game folder.

GOOD TO KNOW
- On other servers the addon stays completely silent.
- This version needs a server with shroudlink 0.6 or newer, and our 0.6 server needs
  this addon: with an older one some windows do not show, and a message asks you to
  update.
- After a game update the addon switches itself off if the game changed too much;
  the game keeps working normally. A new version will be needed then.
- If something seems off, the file shroudlink_client.log in the game folder shows
  what the addon did.
