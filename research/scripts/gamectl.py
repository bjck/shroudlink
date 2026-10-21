"""Drive the running game for tests: real keyboard and mouse input, plus what the client
addon reports on screen (needs a `shroudlink_automation` file in the game folder).

usage:
  python gamectl.py state                    what the addon sees (JSON)
  python gamectl.py key <KEY>                tap a key (letter, ESC, TAB, ENTER, F1..)
  python gamectl.py click <x> <y>            left click at UI units (1080 high)
  python gamectl.py hover <x> <y>            move the cursor there
  python gamectl.py shot <out.png>           screenshot of the game window
  python gamectl.py test locks               L on a backpack slot: rim on, rim off
  python gamectl.py test search              item search in a chest screen (one must be open)
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
    hover(960, 60)  # away from the slots, so R is not taken as a slot action
    key("R", hold=1.2)
    time.sleep(1.0)
    after = next((x for x in addon.state()["slots"] if x["slot"] == slot["slot"]), None)
    assert after and after["item"] == slot["item"], f"the locked item left the slot: {after}"
    print("PASS hold R (quick stack) keeps the locked item")
    hover(*centre(slot["rect"]))
    key("L")
    addon.wait("the rim to go", lambda s: any(x["slot"] == slot["slot"] and x["mark"] is None for x in s["slots"]))
    print("PASS L again unlocks")
    key("ESC")
    addon.wait("the chest to close", lambda s: s["screen"] not in CHEST_SCREENS)
    print("PASS Esc closes the chest")


def main():
    args = sys.argv[1:]
    if not args:
        raise SystemExit(__doc__)
    cmd = args[0]
    if cmd == "state":
        print(json.dumps(Addon().state(), indent=1))
    elif cmd == "key":
        key(args[1])
    elif cmd == "click":
        click(float(args[1]), float(args[2]))
    elif cmd == "hover":
        hover(float(args[1]), float(args[2]))
    elif cmd == "shot":
        shot(args[1])
    elif cmd == "test":
        {"locks": test_locks, "search": test_search, "chest": test_chest}[args[1]](Addon())
    else:
        raise SystemExit(__doc__)


if __name__ == "__main__":
    main()
