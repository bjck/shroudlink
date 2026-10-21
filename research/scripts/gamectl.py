"""Drive the running game for tests: real keyboard and mouse input, plus what the client
addon reports on screen (needs a `shroudlink_automation` file in the game folder).

usage:
  python gamectl.py launch                   start the game straight into the local server
  python gamectl.py quit                     close the game
  python gamectl.py rejoin                   quit, then launch
  python gamectl.py join                     game already started with boot connect: confirm, wait
  python gamectl.py ensure                   in the world on the local server, rejoining if needed
  python gamectl.py state                    what the addon sees (JSON)
  python gamectl.py key <KEY>                tap a key (letter, ESC, TAB, ENTER, F1..)
  python gamectl.py click <x> <y>            left click at UI units (1080 high)
  python gamectl.py hover <x> <y>            move the cursor there
  python gamectl.py shot <out.png>           screenshot of the game window
  python gamectl.py test locks               L on a backpack slot: rim on, rim off
  python gamectl.py test search              item search in a chest screen (one must be open)
  python gamectl.py goto <name>              teleport to the chest with that name, face it, open it
  python gamectl.py test all [name]          ensure in world, goto the chest (default "food"), test chest
  python gamectl.py test chest               stand in front of a chest: open it, search, lock a
                                             slot, quick stack (hold R), check the lock held

The game window gets focus for input; do not use mouse or keyboard meanwhile.
"""
import ctypes
import ctypes.wintypes as wt
import json
import socket
import sys
import time
from pathlib import Path

ADDRESS = ("127.0.0.1", 47911)
user32 = ctypes.WinDLL("user32", use_last_error=True)
ctypes.windll.shcore.SetProcessDpiAwareness(2)

INPUT_MOUSE, INPUT_KEYBOARD = 0, 1
KEYEVENTF_KEYUP, KEYEVENTF_SCANCODE = 0x0002, 0x0008
MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP = 0x0002, 0x0004


class KEYBDINPUT(ctypes.Structure):
    _fields_ = [("wVk", wt.WORD), ("wScan", wt.WORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class MOUSEINPUT(ctypes.Structure):
    _fields_ = [("dx", wt.LONG), ("dy", wt.LONG), ("mouseData", wt.DWORD), ("dwFlags", wt.DWORD), ("time", wt.DWORD), ("dwExtraInfo", ctypes.c_size_t)]


class _U(ctypes.Union):
    _fields_ = [("ki", KEYBDINPUT), ("mi", MOUSEINPUT), ("pad", ctypes.c_byte * 32)]


class INPUT(ctypes.Structure):
    _fields_ = [("type", wt.DWORD), ("u", _U)]


def send(*inputs):
    arr = (INPUT * len(inputs))(*inputs)
    user32.SendInput(len(inputs), arr, ctypes.sizeof(INPUT))


# ---- the addon's view ---------------------------------------------------------------
class Addon:
    def __init__(self):
        self.sock = socket.create_connection(ADDRESS, timeout=5)
        self.reader = self.sock.makefile("r", encoding="utf-8")

    def ask(self, cmd):
        self.sock.sendall((json.dumps({"cmd": cmd}) + "\n").encode())
        return json.loads(self.reader.readline())

    def state(self):
        return self.ask("state")

    def ask_send(self, message):
        """Has the addon send `message` to the server."""
        self.sock.sendall((json.dumps({"cmd": "send", "message": message}) + "\n").encode())
        return json.loads(self.reader.readline())

    def wait(self, what, check, timeout=5.0):
        end = time.time() + timeout
        while time.time() < end:
            s = self.state()
            if check(s):
                return s
            time.sleep(0.1)
        raise AssertionError(f"timed out waiting for {what}")


# ---- the game window ----------------------------------------------------------------
def window():
    hwnd = user32.FindWindowW(None, "Enshrouded")
    if not hwnd:
        raise SystemExit("game window not found")
    return hwnd


def focus():
    hwnd = window()
    if user32.GetForegroundWindow() != hwnd:
        # Windows only lets the process that got the last input change the foreground
        send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0x12, 0, 0, 0, 0))), INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0x12, 0, KEYEVENTF_KEYUP, 0, 0))))
        user32.SetForegroundWindow(hwnd)
        time.sleep(0.3)
    return hwnd


def to_screen(x, y):
    hwnd = window()
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    scale = rect.bottom / 1080.0
    pt = wt.POINT(int(x * scale), int(y * scale))
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    return pt.x, pt.y


SCAN = {"ESC": 0x01, "TAB": 0x0F, "ENTER": 0x1C, "SPACE": 0x39, "SLASH": 0x35, "BACKSPACE": 0x0E}


def key(name, hold=0.08):
    focus()
    name = name.upper()
    if name in SCAN:
        scan = SCAN[name]
    elif name.startswith("F") and name[1:].isdigit():
        scan = 0x3A + int(name[1:]) if int(name[1:]) <= 10 else {11: 0x57, 12: 0x58}[int(name[1:])]
    else:
        scan = user32.MapVirtualKeyW(ord(name[0]), 0)
    vk = user32.MapVirtualKeyW(scan, 1)
    send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(vk, scan, KEYEVENTF_SCANCODE, 0, 0))))
    time.sleep(hold)
    send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(vk, scan, KEYEVENTF_SCANCODE | KEYEVENTF_KEYUP, 0, 0))))
    time.sleep(0.15)


KEYEVENTF_UNICODE = 0x0004


def type_text(text):
    """Types characters as Unicode input: independent of the keyboard layout."""
    focus()
    for c in text:
        send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0, ord(c), KEYEVENTF_UNICODE, 0, 0))))
        send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0, ord(c), KEYEVENTF_UNICODE | KEYEVENTF_KEYUP, 0, 0))))
        time.sleep(0.08)


def hover(x, y):
    focus()
    sx, sy = to_screen(x, y)
    user32.SetCursorPos(sx, sy)
    time.sleep(0.2)


def click(x, y):
    hover(x, y)
    send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_LEFTDOWN, 0, 0))))
    time.sleep(0.08)
    send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_LEFTUP, 0, 0))))
    time.sleep(0.2)


MOUSEEVENTF_MOVE = 0x0001


def turn(dx, dy=0, steps=10):
    """Turns the camera with relative mouse movement (raw input), in small steps."""
    focus()
    for _ in range(steps):
        send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(int(dx / steps), int(dy / steps), 0, MOUSEEVENTF_MOVE, 0, 0))))
        time.sleep(0.02)
    time.sleep(0.3)


MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP = 0x0008, 0x0010


def right_click(x, y):
    hover(x, y)
    send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_RIGHTDOWN, 0, 0))))
    time.sleep(0.08)
    send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_RIGHTUP, 0, 0))))
    time.sleep(0.3)


def ctrl_click(x, y):
    """Ctrl + left click (in inventories: move between action bar and backpack)."""
    hover(x, y)
    send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0x11, 0x1D, 0, 0, 0))))
    time.sleep(0.05)
    send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_LEFTDOWN, 0, 0))))
    time.sleep(0.08)
    send(INPUT(INPUT_MOUSE, _U(mi=MOUSEINPUT(0, 0, 0, MOUSEEVENTF_LEFTUP, 0, 0))))
    time.sleep(0.05)
    send(INPUT(INPUT_KEYBOARD, _U(ki=KEYBDINPUT(0x11, 0x1D, KEYEVENTF_KEYUP, 0, 0))))
    time.sleep(0.3)


def shot(out):
    import subprocess
    subprocess.run([sys.executable, str(Path(__file__).with_name("shot.py")), out], check=True)


def centre(rect):
    return rect[0] + rect[2] / 2, rect[1] + rect[3] / 2


# ---- scenarios ----------------------------------------------------------------------
def test_locks(addon):
    """L over a backpack slot in the inventory: rim on, rim off."""
    s = addon.state()
    assert s["connected"], "the addon is not connected to a shroudlink server"
    if not s["slots"]:
        key("B")  # backpack tab of the player menu
        s = addon.wait("backpack slots on screen", lambda s: s["slots"])
    slot = next((x for x in s["slots"] if x["item"] and not x["mark"]), None)
    assert slot, "no unlocked backpack slot with an item on screen"
    print(f"slot {slot['slot']:#x} at position {slot['position']}")
    hover(*centre(slot["rect"]))
    key("L")
    addon.wait("the rim", lambda s: any(x["slot"] == slot["slot"] and x["mark"] == "rim" for x in s["slots"]))
    print("PASS L locks the slot (rim drawn)")
    key("L")
    addon.wait("the rim to go", lambda s: any(x["slot"] == slot["slot"] and x["mark"] is None for x in s["slots"]))
    print("PASS L again unlocks it")


def test_search(addon):
    """Item search on an open chest screen: typed term reaches the addon, Esc clears it."""
    s = addon.state()
    assert s["screen"] in (0x16, 0x1C, 0x29), f"no chest screen open (screen {s['screen']:#x})"
    type_text("/")
    addon.wait("the search field", lambda s: s["search_open"])
    type_text("wood")
    addon.wait("the term", lambda s: s["search"] == "wood")
    print("PASS / opens the search and typing reaches it")
    key("ESC")
    addon.wait("a cleared search", lambda s: s["search"] == "")
    key("ESC")
    addon.wait("a closed search", lambda s: not s["search_open"])
    print("PASS Esc clears, Esc again closes")


CHEST_SCREENS = (0x16, 0x1C, 0x29)


def test_chest(addon):
    """The whole chest round trip with real input, starting in front of a chest."""
    s = addon.state()
    assert s["connected"], "the addon is not connected to a shroudlink server"
    if s["screen"] not in CHEST_SCREENS:
        key("E")
        s = addon.wait("a chest screen (stand in front of a chest)", lambda s: s["screen"] in CHEST_SCREENS)
    print("PASS E opens the chest")
    s = addon.wait("backpack slots", lambda s: s["slots"])
    assert any(x == "hello/wave" for x in s["sections"]), "hello's Wave row is missing"
    print("PASS hello's backpack section is there")
    test_search(addon)

    slot = next((x for x in addon.state()["slots"] if x["item"] and not x["mark"]), None)
    assert slot, "no unlocked backpack slot with an item"
    hover(*centre(slot["rect"]))
    key("L")
    addon.wait("the rim", lambda s: any(x["slot"] == slot["slot"] and x["mark"] == "rim" for x in s["slots"]))
    print(f"PASS L locks slot {slot['position']} (item {slot['item']})")
    # the action bar is the last two pages the backpack panel draws
    def action_bar(state):
        last = max(x["position"][0] for x in state["slots"])
        return sorted((x["position"], x["item"]) for x in state["slots"] if x["position"][0] >= last - 1 and x["item"])

    bar_before = action_bar(addon.state())
    hover(960, 60)  # away from the slots, so R is not taken as a slot action
    key("R", hold=1.2)
    time.sleep(1.0)
    after_state = addon.state()
    after = next((x for x in after_state["slots"] if x["slot"] == slot["slot"]), None)
    assert after and after["item"] == slot["item"], f"the locked item left the slot: {after}"
    print("PASS hold R (quick stack) keeps the locked item")
    assert action_bar(after_state) == bar_before, f"the action bar changed: {bar_before} -> {action_bar(after_state)}"
    print(f"PASS the action bar stayed ({len(bar_before)} item(s))")
    # tap R: move all; locked slot and action bar stay, the rest goes into the chest
    hover(960, 60)
    key("R", hold=0.08)
    time.sleep(1.5)
    after_state = addon.state()
    after = next((x for x in after_state["slots"] if x["slot"] == slot["slot"]), None)
    assert after and after["item"] == slot["item"], f"move all took the locked item: {after}"
    assert action_bar(after_state) == bar_before, f"move all changed the action bar: {bar_before} -> {action_bar(after_state)}"
    print("PASS tap R (move all) keeps the locked item and the action bar")
    key("F", hold=0.4)  # take all back from the chest
    time.sleep(1.5)
    hover(*centre(slot["rect"]))
    key("L")
    addon.wait("the rim to go", lambda s: any(x["slot"] == slot["slot"] and x["mark"] is None for x in s["slots"]))
    print("PASS L again unlocks")
    key("ESC")
    addon.wait("the chest to close", lambda s: s["screen"] not in CHEST_SCREENS)
    print("PASS Esc closes the chest")


ROOT = Path(__file__).resolve().parents[2]
STEAM_APP = "1203620"


def steam_exe():
    import winreg
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, r"Software\Valve\Steam") as k:
        return winreg.QueryValueEx(k, "SteamExe")[0]


def local_server():
    """Address and admin password of the local test server (server/enshrouded_server.json)."""
    cfg = json.loads((ROOT / "server" / "enshrouded_server.json").read_text(encoding="utf-8"))
    password = next((g["password"] for g in cfg.get("userGroups", []) if g["name"] == "Admin"), "")
    return f"127.0.0.1:{cfg.get('queryPort', 15637)}", password


def running():
    import subprocess
    out = subprocess.run(["tasklist", "/FI", "IMAGENAME eq enshrouded.exe", "/FO", "CSV", "/NH"], capture_output=True, text=True).stdout
    return "enshrouded.exe" in out.lower()


def quit_game(timeout=60):
    import subprocess
    if not running():
        return
    hwnd = user32.FindWindowW(None, "Enshrouded")
    if hwnd:
        user32.PostMessageW(hwnd, 0x0010, 0, 0)  # WM_CLOSE: the game saves and exits
    end = time.time() + timeout
    while running() and time.time() < end:
        time.sleep(1)
    if running():
        subprocess.run(["taskkill", "/IM", "enshrouded.exe", "/F"], capture_output=True)
        time.sleep(3)
    print("game closed")


def launch(address=None, password=None, timeout=240):
    """Starts the game through Steam with the boot connect arguments and waits until the
    addon reports a connection to the server and the world (screen 0x01)."""
    import subprocess
    if running():
        raise SystemExit("the game is running already (use rejoin)")
    default_address, default_password = local_server()
    address, password = address or default_address, password or default_password
    subprocess.Popen([steam_exe(), "-applaunch", STEAM_APP, "+connect", address, "+password", password])
    print(f"launching, joining {address}")
    join(timeout)


def join(timeout=240):
    """Waits until the addon reports the world; confirms the boot connect dialog meanwhile."""
    end = time.time() + timeout
    confirmed_at = 0.0
    while time.time() < end:
        try:
            s = Addon().state()
        except OSError:
            s = None  # the addon starts with the game
        if s and s.get("in_world", s["connected"]) and s["screen"] == 0x01:
            print("in the world, addon connected")
            return
        # boot connect stops at "About to join: … JOIN SESSION" (already selected): confirm it
        # until the addon reports a session; the addon is up long before the menu
        if s and not s["connected"] and s["screen"] == 0 and time.time() - confirmed_at > 6 and user32.FindWindowW(None, "Enshrouded"):
            key("ENTER")
            confirmed_at = time.time()
        time.sleep(2)
    raise SystemExit("the game did not reach the world in time")


def joined_since_server_start():
    """The local server logged an addon session after its last start (right after a server
    restart the game still ticks for a few seconds before it notices)."""
    lines = SERVER_LOG.read_text(encoding="utf-8", errors="replace").splitlines()
    starts = [i for i, l in enumerate(lines) if "starting (loaded as" in l]
    return bool(starts) and any("joined on slot" in l for l in lines[starts[-1]:])


def ensure_in_world():
    """In the world on the local server, whatever state the game is in (else rejoin)."""
    try:
        s = Addon().state()
        if s.get("in_world") and s["screen"] == 0x01 and joined_since_server_start():
            return
    except OSError:
        pass
    print("not in the world: rejoining")
    quit_game()
    launch()


SERVER_LOG = ROOT / "server" / "shroudlink.log"


def server_debug(addon, message, reply_tag, timeout=8.0):
    """Sends a core debug command (the local server needs a `shroudlink_probe` file) and
    returns the JSON it logged after `reply_tag:` in shroudlink.log."""
    start = SERVER_LOG.stat().st_size
    addon.ask_send(message)
    end = time.time() + timeout
    while time.time() < end:
        with open(SERVER_LOG, "rb") as f:
            f.seek(start)
            for line in f.read().decode("utf-8", "replace").splitlines():
                if f"{reply_tag}: " in line:
                    return json.loads(line.split(f"{reply_tag}: ", 1)[1])
        time.sleep(0.3)
    raise AssertionError(f"no {reply_tag} reply in the server log")


def player_position(addon):
    return server_debug(addon, {"mod": "shroudlink", "op": "debug_players"}, "debug_players")[0]["position"]


def camera_direction(addon):
    """Unit vector (x, z) the camera looks along: W walks the character that way."""
    import math
    before = player_position(addon)
    key("W", hold=0.35)
    time.sleep(0.6)
    after = player_position(addon)
    dx, dz = after[0] - before[0], after[2] - before[2]
    length = math.hypot(dx, dz)
    assert length > 0.2, f"W did not move the character ({length:.2f} m)"
    return dx / length, dz / length


def goto_chest(addon, name, distance=1.6):
    """Teleports the player in front of the chest called `name` (or with that entity id),
    with the chest straight ahead of the camera, and opens it with E."""
    found = server_debug(addon, {"mod": "shroudlink", "op": "debug_inventories", "radius": 200}, "debug_inventories")
    chests = [c for c in found if (c["name"] or "").lower() == name.lower() or str(c["entity"]) == name]
    assert chests, f"no chest named '{name}' near the player"
    x, y, z = chests[0]["position"]
    addon.ask_send({"mod": "shroudlink", "op": "debug_teleport", "x": x + 4, "y": y + 0.2, "z": z + 4})
    time.sleep(1.2)
    dx, dz = camera_direction(addon)
    addon.ask_send({"mod": "shroudlink", "op": "debug_teleport", "x": x - dx * distance, "y": y + 0.2, "z": z - dz * distance})
    time.sleep(1.2)
    for pitch in (100, 100, 100):
        turn(0, pitch)  # look further down step by step: chests are low
        key("E")
        time.sleep(1.0)
        screen = addon.state()["screen"]
        if screen in CHEST_SCREENS:
            print(f"PASS at chest '{name}' ({chests[0]['entity']})")
            return
        if screen != 0x01:
            key("ESC")
            time.sleep(0.6)
    raise AssertionError(f"could not open chest '{name}'")


def main():
    args = sys.argv[1:]
    if not args:
        raise SystemExit(__doc__)
    cmd = args[0]
    if cmd == "launch":
        launch(*args[1:3])
    elif cmd == "ensure":
        ensure_in_world()
    elif cmd == "join":
        join()
    elif cmd == "quit":
        quit_game()
    elif cmd == "rejoin":
        quit_game()
        launch(*args[1:3])
    elif cmd == "state":
        print(json.dumps(Addon().state(), indent=1))
    elif cmd == "key":
        key(args[1])
    elif cmd == "click":
        click(float(args[1]), float(args[2]))
    elif cmd == "hover":
        hover(float(args[1]), float(args[2]))
    elif cmd == "shot":
        shot(args[1])
    elif cmd == "goto":
        goto_chest(Addon(), args[1])
    elif cmd == "test" and args[1] == "all":
        ensure_in_world()
        addon = Addon()
        goto_chest(addon, args[2] if len(args) > 2 else "food")
        test_chest(addon)
    elif cmd == "test":
        {"locks": test_locks, "search": test_search, "chest": test_chest}[args[1]](Addon())
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
