"""Create a new shroudlink server mod from the template.

usage: python newmod.py <name>     (lower case, e.g. keep_torches)

Creates mod/mods/<name>/ with a mod that has a MODS tab window, inventory event stubs
(keep, on_transfer) and a unit test, and adds it to the workspace. Build it with
`cargo build --release -p <name>` and copy target/release/<name>.dll into the server's
shroudlink_mods/ (hot-reloaded while players are connected).
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2] / "mod"

CARGO = """[package]
name = "{name}"
version.workspace = true
edition.workspace = true
license.workspace = true

[lib]
# shroudlink_mods/{name}.dll next to the server
crate-type = ["cdylib", "rlib"]

[dependencies]
shroudlink-sdk = {{ path = "../../sdk" }}
"""

LIB = """//! {title}: a shroudlink server mod (see mod/MODDING.md and mod/docs/sdk-v2.md).

use shroudlink_sdk::ui::{{Event, MenuEntry, Node, ToClient, UiEvent, Window}};
use shroudlink_sdk::{{export_mod, log, Game, Host, InventoryEvent, Mod, Value}};

const ID: &str = "{name}";

#[derive(Default)]
pub struct {type_name};

impl {type_name} {{
    fn window(&self) -> ToClient {{
        let content = vec![Node::Text {{ text: "Hello from {name}.".into() }}];
        ToClient::Open {{ owner: ID.into(), window: Window {{ id: "main".into(), title: "{title}".into(), content }} }}
    }}
}}

impl Mod for {type_name} {{
    fn id(&self) -> &'static str {{
        ID
    }}

    fn on_load(&mut self) {{
        log!("{name} loaded");
    }}

    // a player with the client addon joined: offer an entry in the MODS tab
    fn on_join(&mut self, host: &mut Host, client: u64) {{
        let entries = ToClient::Entries {{ owner: ID.into(), entries: vec![MenuEntry {{ id: "open".into(), label: "{title}".into() }}] }};
        host.send(client, &entries.to_message());
    }}

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {{
        if let Some(UiEvent {{ event: Event::Menu, .. }}) = UiEvent::from_message(message) {{
            host.send(client, &self.window().to_message());
        }}
    }}

    // slots of `event.from` that must not move in this quick stack / move all
    fn keep(&mut self, _game: &Game, _event: &InventoryEvent) -> Vec<u64> {{
        Vec::new()
    }}

    // move stacks yourself (game.move_stack) before the game's own transfer
    fn on_transfer(&mut self, _game: &Game, _event: &InventoryEvent) -> u32 {{
        0
    }}
}}

export_mod!({type_name});

#[cfg(test)]
mod tests {{
    use super::*;
    use shroudlink_sdk::testing::MockHost;

    #[test]
    fn menu_opens_the_window() {{
        let mut m = {type_name}::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        assert_eq!(host.take_sent()[0].1["op"], "entries");
        let menu = UiEvent {{ window: String::new(), node: "open".into(), event: Event::Menu }};
        m.on_message(&mut host.host(), 1, &menu.to_message(ID));
        assert_eq!(host.take_sent()[0].1["op"], "open");
    }}
}}
"""


def main():
    if len(sys.argv) != 2 or not re.fullmatch(r"[a-z][a-z0-9_]*", sys.argv[1]):
        raise SystemExit(__doc__)
    name = sys.argv[1]
    target = ROOT / "mods" / name
    if target.exists():
        raise SystemExit(f"{target} exists already")
    words = name.split("_")
    fields = {"name": name, "title": " ".join(words).capitalize(), "type_name": "".join(w.capitalize() for w in words)}
    (target / "src").mkdir(parents=True)
    (target / "Cargo.toml").write_text(CARGO.format(**fields), encoding="utf-8", newline="\n")
    (target / "src" / "lib.rs").write_text(LIB.format(**fields), encoding="utf-8", newline="\n")

    manifest = ROOT / "Cargo.toml"
    text = manifest.read_text(encoding="utf-8")
    members = re.search(r"members = \[(.*?)\]", text)
    items = sorted(set(re.findall(r'"([^"]+)"', members.group(1))) | {f"mods/{name}"})
    text = text[: members.start(1)] + ", ".join(f'"{m}"' for m in items) + text[members.end(1):]
    manifest.write_text(text, encoding="utf-8", newline="\n")
    print(f"created {target}")
    print(f"build: cargo build --release -p {name}  ->  target/release/{name}.dll  ->  server's shroudlink_mods/")


if __name__ == "__main__":
    main()
