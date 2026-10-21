//! Self-test inside the real dedicated server, without any player (research/scripts/selftest.py
//! runs it). Active only with a `shroudlink_selftest` file next to the server; the result
//! goes to `shroudlink_selftest.txt`.
//!
//! Inventory functions need a game context, which the game only builds while it works on
//! inventories. Without players that happens once: `FUN_14019c480`, a world system that
//! runs after the world has loaded, builds one from world-level storages
//! (`FUN_14003aba0` init + `FUN_1401b2bf0(ctx, descriptor)`). We copy its descriptor, and
//! when the system returns we build our own context from it and run the tests there, on
//! the game thread.
//!
//! The tests change the world (items move): the runner restores the savegame afterwards.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use chestsort_common::log;
use retour::GenericDetour;
use serde_json::{json, Value};

use crate::events;
use crate::hooks::{self, Feature};
use crate::game::{slot_id, Ctx, DepositResult, Game, FAKE_PLAYER, SLOTS_PER_PAGE};
use crate::link_host;
use crate::mods;

pub const MARKER: &str = "shroudlink_selftest";
const REPORT: &str = "shroudlink_selftest.txt";


/// Descriptor bytes read by the context builder (last field at +0xe8).
const DESCRIPTOR: usize = 0xf0;
/// The builder writes up to +0x71a8; the game keeps contexts in ~0x71d8 bytes of stack.
const CTX_BYTES: usize = 0x8000;
/// Entity ids searched for inventories (the lookup is a bounds-checked hash map).
const MAX_ENTITY: u32 = 2_000_000;
/// Player slot used by the fake client addon session.
const FAKE_SLOT: usize = link_host::SLOTS - 1;
const FAKE_CLIENT: u64 = 0x5e1f_7e57;

type SystemFn = unsafe extern "system" fn(system: *mut u8);
type BuildCtxFn = unsafe extern "system" fn(ctx: *mut u8, descriptor: *const u8);
type InitCtxFn = unsafe extern "system" fn(ctx: *mut u8);

struct Hooks {
    boot: GenericDetour<SystemFn>,
    build: GenericDetour<BuildCtxFn>,
    init: InitCtxFn,
}
unsafe impl Send for Hooks {}
unsafe impl Sync for Hooks {}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
static IN_BOOT: AtomicBool = AtomicBool::new(false);
static DONE: AtomicBool = AtomicBool::new(false);
static CAPTURED: Mutex<Option<Vec<u8>>> = Mutex::new(None);
/// A name the test gives one inventory (chests near players only load with a player).
static FAKE_NAME: Mutex<Option<(u32, String)>> = Mutex::new(None);

/// The test name of an entity, if the self-test gave it one.
pub fn fake_name(entity: u32) -> Option<String> {
    FAKE_NAME.lock().unwrap().as_ref().filter(|(e, _)| *e == entity).map(|(_, n)| n.clone())
}

pub fn install() {
    if !chestsort_common::game_dir().join(MARKER).exists() {
        return;
    }
    let _ = std::fs::remove_file(chestsort_common::game_dir().join(REPORT));
    let result = (|| unsafe {
        for feature in [Feature::SelfTest, Feature::Inventory] {
            if let Some(why) = hooks::problem(feature) {
                return Err(format!("feature {} is off ({why})", feature.name()));
            }
        }
        let boot = GenericDetour::<SystemFn>::new(std::mem::transmute::<usize, SystemFn>(hooks::address("boot_system")?), boot_hook).map_err(|e| e.to_string())?;
        let build = GenericDetour::<BuildCtxFn>::new(std::mem::transmute::<usize, BuildCtxFn>(hooks::address("build_ctx")?), build_hook).map_err(|e| e.to_string())?;
        let init = std::mem::transmute::<usize, InitCtxFn>(hooks::address("init_ctx")?);
        let hooks = HOOKS.get_or_init(|| Hooks { boot, build, init });
        hooks.boot.enable().map_err(|e| e.to_string())?;
        hooks.build.enable().map_err(|e| e.to_string())?;
        Ok::<_, String>(())
    })();
    match result {
        Ok(()) => log!("selftest: armed, runs once the world has loaded"),
        Err(e) => write_report(&format!("FAIL setup: {e}\n")),
    }
}

unsafe extern "system" fn build_hook(ctx: *mut u8, descriptor: *const u8) {
    if IN_BOOT.load(Ordering::Relaxed) {
        let mut captured = CAPTURED.lock().unwrap();
        if captured.is_none() {
            *captured = Some(std::slice::from_raw_parts(descriptor, DESCRIPTOR).to_vec());
        }
    }
    HOOKS.get().unwrap().build.call(ctx, descriptor)
}

unsafe extern "system" fn boot_hook(system: *mut u8) {
    let hooks = HOOKS.get().unwrap();
    IN_BOOT.store(true, Ordering::Relaxed);
    hooks.boot.call(system);
    IN_BOOT.store(false, Ordering::Relaxed);
    let descriptor = CAPTURED.lock().unwrap().clone();
    if let Some(descriptor) = descriptor {
        if !DONE.swap(true, Ordering::SeqCst) {
            run(&descriptor);
        }
    }
}

fn write_report(text: &str) {
    let _ = std::fs::write(chestsort_common::game_dir().join(REPORT), text);
    for line in text.lines() {
        log!("selftest: {line}");
    }
}

struct Report {
    text: String,
    failed: usize,
}

impl Report {
    fn check(&mut self, name: &str, result: Result<String, String>) {
        let line = match &result {
            Ok(detail) => format!("PASS {name}: {detail}"),
            Err(detail) => {
                self.failed += 1;
                format!("FAIL {name}: {detail}")
            }
        };
        let _ = writeln!(self.text, "{line}");
    }
    fn skip(&mut self, name: &str, why: &str) {
        let _ = writeln!(self.text, "SKIP {name}: {why}");
    }
}

/// Inventory slots of an entity: (slot id, item, count).
unsafe fn slots(game: &Game, ctx: *mut Ctx, entity: u32) -> Vec<(u64, u32, u32)> {
    let Some(setup) = game.inventory_setup(ctx, entity) else { return Vec::new() };
    let mut out = Vec::new();
    for page in game.pages(ctx, setup).pages().iter().filter(|p| p.entity != 0) {
        for slot in 0..SLOTS_PER_PAGE {
            let stack = &*page.slots.add(slot as usize);
            out.push((slot_id(page.entity, slot), stack.item_id, stack.count));
        }
    }
    out
}

fn items(slots: &[(u64, u32, u32)]) -> u32 {
    slots.iter().map(|s| s.2).sum()
}

/// Messages the fake addon session received since the last call.
fn received() -> Vec<Value> {
    link_host::with_host(|host| host.take_messages(FAKE_SLOT, FAKE_CLIENT))
}

fn send(message: Value) -> Vec<Value> {
    link_host::with_host(|host| host.deliver_test_message(FAKE_CLIENT, &message));
    received()
}

fn ui_event(window: &str, node: &str, event: Value) -> Value {
    json!({ "mod": "chestsort", "op": "ui_event", "event": { "window": window, "node": node, "event": event } })
}

unsafe fn run(descriptor: &[u8]) {
    let hooks = HOOKS.get().unwrap();
    let mut report = Report { text: String::new(), failed: 0 };
    let _ = writeln!(report.text, "shroudlink selftest {}", env!("CARGO_PKG_VERSION"));

    // our own game context, built like the world system built its own
    let mut memory = vec![0u128; CTX_BYTES / 16];
    let ctx = memory.as_mut_ptr() as *mut u8;
    (hooks.init)(ctx);
    hooks.build.call(ctx, descriptor.as_ptr());
    let ctx = ctx as *mut Ctx;
    let game = events::game();

    // ---- hook registry ----
    let off: Vec<String> = hooks::FEATURES.iter().filter_map(|&f| hooks::problem(f).map(|why| format!("{} ({why})", f.name()))).collect();
    let marker = std::fs::read_to_string(chestsort_common::game_dir().join(MARKER)).unwrap_or_default();
    if marker.contains("break ") {
        let _ = writeln!(report.text, "INFO simulated broken hook points: {}", marker.lines().filter_map(|l| l.strip_prefix("break ")).collect::<Vec<_>>().join(", "));
        report.check("only broken features are off", Ok(format!("on: {}; off: {}", hooks::working().join(", "), off.join("; "))));
    } else {
        report.check("hook points", if off.is_empty() { Ok(format!("{} found, features: {}", hooks::POINTS.len(), hooks::working().join(", "))) } else { Err(format!("off: {}", off.join("; "))) });
    }

    // ---- mods ----
    let ids = mods::ids();
    report.check(
        "mods loaded",
        if ids.iter().any(|i| i == "chestsort") { Ok(ids.join(", ")) } else { Err(format!("chestsort missing, loaded: {ids:?}")) },
    );

    // ---- channel: a client addon session on a free player slot ----
    link_host::with_host(|host| host.connect_fake(FAKE_SLOT, FAKE_CLIENT));
    let welcome = received();
    report.check(
        "addon session",
        match welcome.iter().find(|m| m["op"] == "welcome") {
            Some(w) => Ok(format!("welcome lists {}", w["mods"])),
            None => Err(format!("no welcome, got {welcome:?}")),
        },
    );
    report.check(
        "slot key offered",
        if welcome.iter().any(|m| m["op"] == "slot_keys" && m["keys"][0]["key"] == "L") { Ok("chestsort registers L".into()) } else { Err("no slot_keys from chestsort".into()) },
    );
    report.check(
        "MODS menu entry",
        if welcome.iter().any(|m| m["op"] == "entries" && m["owner"] == "chestsort") { Ok("chestsort offers its menu".into()) } else { Err("no chestsort entries".into()) },
    );
    let open = send(ui_event("", "categories", json!({ "kind": "menu" })));
    let lists = open.iter().any(|m| m["op"] == "open" && m["window"]["content"].to_string().contains("\"list\""));
    report.check("chest categories window", if lists { Ok("opens with the category list".into()) } else { Err(format!("got {open:?}")) });
    let found = send(ui_event("categories", "search", json!({ "kind": "submit", "value": "iron bar" })));
    let ores = found.iter().any(|m| m["window"]["content"].to_string().contains("found:Ores"));
    report.check("item search", if ores { Ok("\"iron bar\" belongs to Ores".into()) } else { Err(format!("got {found:?}")) });

    // ---- inventories of the world ----
    let mut inventories: Vec<(u32, Option<String>, Vec<(u64, u32, u32)>)> = Vec::new();
    for entity in 1..=MAX_ENTITY {
        if game.entity_record(ctx, entity).is_none() || game.inventory_setup(ctx, entity).is_none() {
            continue;
        }
        let name = crate::game_api::name_of(game, ctx, entity);
        inventories.push((entity, name, slots(game, ctx, entity)));
    }
    let named: Vec<&(u32, Option<String>, Vec<(u64, u32, u32)>)> = inventories.iter().filter(|i| i.1.is_some()).collect();
    report.check(
        "world inventories",
        if inventories.is_empty() {
            Err("no entity with an inventory found".into())
        } else {
            Ok(format!("{} inventories, {} named: {}", inventories.len(), named.len(), named.iter().map(|i| format!("{} '{}'", i.0, i.1.as_deref().unwrap_or(""))).collect::<Vec<_>>().join(", ")))
        },
    );

    for (entity, name, slots) in &inventories {
        let _ = writeln!(
            report.text,
            "INFO inventory {entity} {:?}: {} slot(s), {} stack(s), {} item(s), player: {}",
            name.as_deref().unwrap_or(""),
            slots.len(),
            slots.iter().filter(|s| s.2 != 0).count(),
            items(slots),
            game.is_player(ctx, *entity)
        );
    }

    // two inventories serve as test player and test chest; chestsort tells us the item ids
    let found = |query: &str| -> Option<u32> {
        let reply = send(ui_event("categories", "search", json!({ "kind": "submit", "value": query })));
        reply.iter().find_map(|m| {
            m["window"]["content"].as_array()?.iter().find(|n| n["type"] == "item_grid" && n["id"].as_str().is_some_and(|id| id.starts_with("found:")))?["items"][0]["id"].as_u64()
        }).map(|id| id as u32)
    };
    match (inventories.len() >= 2, found("iron bar"), found("plant fibre")) {
        (true, Some(ore), Some(fibre)) => inventory_tests(&mut report, game, ctx, inventories[0].0, inventories[1].0, ore, fibre),
        (false, ..) => report.skip("inventory events", "fewer than two inventories in the world"),
        _ => report.skip("inventory events", "chestsort did not name test items"),
    }

    // ---- hot reload ----
    match crate::plugins::touch_for_test("chestsort") {
        Ok(()) => {
            link_host::with_host(|host| crate::plugins::reload_now(host));
            let again = received();
            let reopened = again.iter().any(|m| m["op"] == "entries" && m["owner"] == "chestsort");
            report.check("hot reload", if reopened { Ok("chestsort reloaded and greeted the session again".into()) } else { Err(format!("after reload got {again:?}")) });
        }
        Err(e) => report.skip("hot reload", &e),
    }

    let _ = writeln!(report.text, "{}", if report.failed == 0 { "RESULT ok".to_string() } else { format!("RESULT {} failure(s)", report.failed) });
    write_report(&report.text);
}

/// Writes a stack straight into an inventory slot (test setup only).
unsafe fn put(game: &Game, ctx: *mut Ctx, entity: u32, index: usize, item: u32, count: u32) -> Option<u64> {
    let setup = game.inventory_setup(ctx, entity)?;
    let pages = game.pages(ctx, setup);
    let page = pages.pages().iter().filter(|p| p.entity != 0).nth(index / SLOTS_PER_PAGE as usize)?;
    let stack = page.slots.add(index % SLOTS_PER_PAGE as usize);
    (*stack).item_id = item;
    (*stack).count = count;
    (*stack).pide = 0;
    Some(slot_id(page.entity, (index % SLOTS_PER_PAGE as usize) as u8))
}

fn count(slots: &[(u64, u32, u32)], item: u32) -> u32 {
    slots.iter().filter(|s| s.1 == item).map(|s| s.2).sum()
}

/// Quick stack and move all from `player` (a test player) into `chest` (named "Ores" for
/// the test): chestsort sorts by category, locks hold, the game's own transfer runs.
unsafe fn inventory_tests(report: &mut Report, game: &Game, ctx: *mut Ctx, player: u32, chest: u32, ore: u32, fibre: u32) {
    FAKE_PLAYER.store(player, Ordering::SeqCst);
    *FAKE_NAME.lock().unwrap() = Some((chest, "Ores".into()));
    let max = game.item_info(ctx, ore).map_or(0, |i| i.max_stack as u32);
    let _ = writeln!(report.text, "INFO test player {player}, test chest {chest} named \"Ores\"; iron bar {ore} (max stack {max}), plant fibre {fibre}");
    let setup = (put(game, ctx, player, 0, ore, 5), put(game, ctx, player, 1, ore, 7), put(game, ctx, player, 2, fibre, 3));
    let (Some(locked), Some(_), Some(_)) = setup else {
        report.skip("inventory events", "could not set up test stacks");
        return;
    };
    // the addon reports the backpack layout and presses L over the first test stack
    let mut pages: Vec<u32> = Vec::new();
    for (slot, ..) in slots(game, ctx, player) {
        if !pages.contains(&(slot as u32)) {
            pages.push(slot as u32);
        }
    }
    let position = [pages.iter().position(|&p| p == locked as u32).unwrap_or(0) as u32, (locked >> 32) as u32];
    send(json!({ "mod": "chestsort", "op": "layout", "pages": pages }));
    let reply = send(json!({ "mod": "chestsort", "op": "slot_key", "key": "L", "slot": locked, "position": position, "item": ore }));
    let stored = reply.iter().find(|m| m["op"] == "store").map(|m| m["data"]["locked"].clone());
    let marked = reply.iter().rev().find(|m| m["op"] == "marks").map(|m| m["marks"][0]["slots"].clone());
    report.check("L stores the lock on the PC", if stored == Some(json!([position])) { Ok(format!("stored position {position:?}")) } else { Err(format!("store message: {stored:?}")) });
    report.check("L marks the slot", if marked == Some(json!([locked])) { Ok("rim on the locked slot".into()) } else { Err(format!("marks message: {marked:?}")) });
    let locked_intact = |game: &Game| slots(game, ctx, player).iter().any(|s| *s == (locked, ore, 5));

    let mut out: DepositResult = std::mem::zeroed();
    let result = (game.deposit_into_chest)(&mut out, ctx, chest, player, std::ptr::null_mut());
    let (p, c) = (slots(game, ctx, player), slots(game, ctx, chest));
    let _ = writeln!(report.text, "INFO after quick stack: player ore {} fibre {}, chest ore {} fibre {}, error {}", count(&p, ore), count(&p, fibre), count(&c, ore), count(&c, fibre), result.as_ref().map_or(255, |r| r.error));
    report.check("quick stack sorts by category", if count(&c, ore) == 7 { Ok("7 iron bars went into the chest named Ores".into()) } else { Err(format!("chest holds {} iron bar(s), expected 7", count(&c, ore))) });
    report.check("quick stack leaves other items", if count(&p, fibre) == 3 && count(&c, fibre) == 0 { Ok("plant fibre stayed with the player".into()) } else { Err(format!("player fibre {}, chest fibre {}", count(&p, fibre), count(&c, fibre))) });
    report.check("quick stack keeps locked slot", if locked_intact(game) { Ok("5 locked iron bars stayed".into()) } else { Err("the locked stack changed".into()) });

    match game.move_all {
        None => report.skip("move all", "feature move_all is off"),
        Some(move_all) => {
            let mut out: DepositResult = std::mem::zeroed();
            move_all(&mut out, ctx, player, chest, 0, 0);
            let (p, c) = (slots(game, ctx, player), slots(game, ctx, chest));
            let _ = writeln!(report.text, "INFO after move all: player ore {} fibre {}, chest ore {} fibre {}", count(&p, ore), count(&p, fibre), count(&c, ore), count(&c, fibre));
            report.check("move all moves the rest", if count(&c, fibre) == 3 { Ok("plant fibre went into the chest".into()) } else { Err(format!("chest holds {} plant fibre, expected 3", count(&c, fibre))) });
            report.check("move all keeps locked slot", if locked_intact(game) { Ok("5 locked iron bars stayed".into()) } else { Err("the locked stack changed".into()) });
        }
    }

    let reply = send(json!({ "mod": "chestsort", "op": "slot_key", "key": "L", "slot": locked, "position": position, "item": ore }));
    let cleared = reply.iter().rev().find(|m| m["op"] == "marks").map(|m| m["marks"][0]["slots"].clone());
    report.check("L again unlocks", if cleared == Some(json!([])) { Ok("rim removed".into()) } else { Err(format!("marks message: {cleared:?}")) });
    FAKE_PLAYER.store(0, Ordering::SeqCst);
    *FAKE_NAME.lock().unwrap() = None;
}
