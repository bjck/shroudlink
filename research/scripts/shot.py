"""Screenshot of the game window (screen capture of its client area).

usage: python shot.py <out.png> [window title, default Enshrouded] [max width]
"""
import ctypes
import ctypes.wintypes as wt
import sys

from PIL import ImageGrab

user32 = ctypes.WinDLL("user32")
ctypes.windll.shcore.SetProcessDpiAwareness(2)


def main():
    out = sys.argv[1]
    title = sys.argv[2] if len(sys.argv) > 2 else "Enshrouded"
    max_w = int(sys.argv[3]) if len(sys.argv) > 3 else 1600
    hwnd = user32.FindWindowW(None, title)
    if not hwnd:
        raise SystemExit(f"window '{title}' not found")
    rect = wt.RECT()
    user32.GetClientRect(hwnd, ctypes.byref(rect))
    pt = wt.POINT(0, 0)
    user32.ClientToScreen(hwnd, ctypes.byref(pt))
    box = (pt.x, pt.y, pt.x + rect.right, pt.y + rect.bottom)
    img = ImageGrab.grab(bbox=box, all_screens=True)
    if img.width > max_w:
        img = img.resize((max_w, img.height * max_w // img.width))
    img.save(out)
    print(f"{out}: {img.width}x{img.height} from {box}")


main()
