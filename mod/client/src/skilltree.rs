//! Mod skill trees on the game's own SKILLS page (`ToClient::SkillTree`; the research is in
//! docs/research/skilltree-ui.md).
//!
//! The page keeps its own lists, built when it opens from the game's runtime skill tree
//! (`FUN_7ff7ee8610e0(state, tree)`): node entries of 0x50 bytes at state +0x78 (count u16
//! at +0x5078), start node pointers at +0x5080 (count u8 at +0x5180) and link index pairs at
//! +0x5188 (count u16 at +0x5988). Every entry points (+0x48) at a runtime node of 0x44
//! bytes, and the page draws a node from that alone: +0 id, +4 type (0 start, 1 attribute,
//! 2 active skill with an icon, 3 passive), +8 / +0xc name and description keys, +0x14 /
//! +0x18 position, +0x1c cost, +0x1e class (frame colour), +0x20 / +0x30 icons, +0x40 max
//! level, +0x41 level, +0x43 unlocked.
//!
//! While a mod's tree is shown, the per-frame step of the page (`FUN_7ff7ee8ad280(state)`)
//! swaps those lists for ones built from the mod's nodes, pointing at runtime nodes this
//! module owns; when the game's tree is shown again it rebuilds the game's lists with the
//! page's own builder. Texts come through a detour on the text lookup (`FUN_7ff7ee610f20`)
//! for keys of mod nodes. The invest key on a mod node (`FUN_7ff7ee85c870(entry, flag)`,
//! which would queue the game's unlock) becomes a `ui_event` click for the mod instead.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use chestsort_common::log;
use chestsort_common::scan::Module;
use retour::GenericDetour;
use shroudlink::ui::{Event, TreeLink, TreeNode, UiEvent};

use crate::native::UiStr;

/// `FUN_7ff7ee8610e0(state, tree)`: builds the page's lists from a runtime tree.
const SIG_BUILD: &str = "48 89 5C 24 10 48 89 6C 24 18 56 57 41 54 41 56 41 57 48 83 EC 70 45 33 FF 48 8B F9 66 44 89 B9 78 50 00 00 4C 8B F2 44 88 B9 80 51 00 00";
/// `FUN_7ff7ee8ad280(state)`: per frame, marks the nodes that can be invested in next.
const SIG_FRAME: &str = "48 89 7C 24 20 41 56 48 83 EC 20 33 FF 4C 8B F1 66 3B B9 88 59 00 00 0F 83 ?? ?? ?? ?? 48 89 5C 24 30 48 89 6C 24 38 48 8D A9 8A 51 00 00";
/// `FUN_7ff7ee85c870(entry, flag)`: the selected node; on the invest key it queues the unlock.
const SIG_UNLOCK: &str = "40 55 56 57 48 83 EC 30 48 8B 35 ?? ?? ?? ?? 48 8B F9 0F B6 EA 48 8B 86 D0 00 00 00 48 8B 08 48 85 C9 0F 84 ?? ?? ?? ??";
/// `FUN_7ff7ee874870(kind, consume)`: a pending UI input of `kind` (1: invest), taken.
const SIG_INPUT: &str = "48 89 5C 24 08 48 8B 05 ?? ?? ?? ?? 44 0F B6 C9 41 BB 00 40 01 00 0F B6 DA B9 E0 3C 01 00 4C 8B 80 D0 00 00 00 4D 8B 10";
/// `FUN_7ff7ee685320(state)`: the class labels around the game's tree ("ClassHints").
const SIG_CLASS_HINTS: &str = "40 55 41 55 48 8D AC 24 98 FC FF FF 48 81 EC 68 04 00 00 48 83 79 18 00 4C 8B E9 75 ?? 48 83 79 20 00 0F 84 ?? ?? ?? ??";
/// `FUN_7ff7ee7c69b0(state)`: the page's top layer ("chromeLayer") with the skill point
/// counter at the top left. It reads the player's points straight from the player data:
/// total, in use and available (u32 at +0x72c / +0x730 / +0x734; player data =
/// `*(*(*(game_ui + 0xd0)) + 0x38)`), and draws the counter (`FUN_7ff7ee7d26c0`) with the
/// skill point item's icon as its emblem.
const SIG_CHROME: &str = "40 55 53 41 56 41 57 48 8D AC 24 A8 DF FF FF B8 58 21 00 00 E8 ?? ?? ?? ?? 48 2B E0 48 8B 05 ?? ?? ?? ?? 45 33 FF 0F 29 B4 24 40 21 00 00";
/// `FUN_7ff7ee70fae0(item, colour_and_scale, ..)`: draws an item's icon; the second argument
/// packs the colour (low u32, 0xAABBGGRR, white 0xffffffff) and the scale (high f32).
const SIG_ITEM_ICON: &str = "48 89 5C 24 08 48 89 74 24 10 48 89 7C 24 18 4C 89 74 24 20 55 48 8D 6C 24 E0 48 81 EC 20 01 00 00 48 8B FA 8B D9 8B D1";
/// The skill point item whose icon is the counter's emblem.
const SKILL_POINT_ITEM: u32 = 0xf2ad_8315;
const POINTS_TOTAL: usize = 0x72c;
/// The text lookup (`native::SIG_LOCA_TEXT`), `fn(out, key) -> out`.
const SIG_LOCA: &str = "48 89 5C 24 10 48 89 74 24 18 55 57 41 55 41 56 41 57 48 8D AC 24 F0 F8 FF FF";

type BuildFn = unsafe extern "system" fn(state: *mut u8, tree: *mut u8);
type FrameFn = unsafe extern "system" fn(state: *mut u8);
type UnlockFn = unsafe extern "system" fn(entry: *mut u8, flag: u64);
type InputFn = unsafe extern "system" fn(kind: u8, consume: u8) -> u64;
type LocaFn = unsafe extern "system" fn(out: *mut UiStr, key: u32) -> *mut UiStr;
type ItemIconFn = unsafe extern "system" fn(item: u32, colour_and_scale: u64, a: u64, b: u64, c: u64, d: u64) -> u64;

/// The tint to give item icons drawn now (inside the point counter of a mod tree), 0: none.
static ICON_TINT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

// page state
const ENTRIES: usize = 0x78;
const ENTRY_SIZE: usize = 0x50;
const ENTRY_COUNT: usize = 0x5078;
const MAX_ENTRIES: usize = 0x100;
const START_PTRS: usize = 0x5080;
const START_COUNT: usize = 0x5180;
const MAX_STARTS: usize = 0x20;
const PAIRS: usize = 0x5188;
const PAIR_COUNT: usize = 0x5988;
const MAX_PAIRS: usize = 0x200;
/// hovered / pressed / selected entry indices (u64, 0xffff: none)
const SELECTION: [usize; 3] = [0x58, 0x60, 0x68];
/// the page's list of nodes reachable from the selection (`FUN_7ff7ee648220`), count u16
const REACH_COUNT: usize = 0x5d8c;
const TREE: usize = 0x5df8;
// entry
const ENTRY_NEIGHBOURS: usize = 0x30;
const ENTRY_NEIGHBOUR_COUNT: usize = 0x40;
const MAX_NEIGHBOURS: usize = 8;
const ENTRY_NODE: usize = 0x48;
// runtime node
const NODE_SIZE: usize = 0x44;

struct Hooks {
    build: BuildFn,
    frame: GenericDetour<FrameFn>,
    unlock: GenericDetour<UnlockFn>,
    input: InputFn,
    loca: GenericDetour<LocaFn>,
    class_hints: GenericDetour<FrameFn>,
    chrome: GenericDetour<FrameFn>,
    item_icon: GenericDetour<ItemIconFn>,
    game_ui: usize,
}
unsafe impl Send for Hooks {}
unsafe impl Sync for Hooks {}

static HOOKS: OnceLock<Hooks> = OnceLock::new();

/// A game node's look, kept from the game's lists: type, class and both icons.
#[derive(Clone, Copy)]
struct Look {
    kind: u8,
    class: u8,
    icons: [u8; 32],
}

/// The tree being shown, built for the page.
struct Built {
    owner: String,
    tree: String,
    #[allow(dead_code)]
    version: u64,
    /// runtime nodes (0x44 bytes each), owned here; the page points into them
    nodes: Box<[[u8; NODE_SIZE]]>,
    node_ids: Vec<String>,
    pairs: Vec<(u16, u16)>,
    /// text key -> text, for the lookup detour
    texts: HashMap<u32, Box<str>>,
}

struct State {
    looks: HashMap<u32, Look>,
    built: Option<Built>,
    /// the page state our lists were put into, and the tree version
    installed: Option<(usize, u64)>,
    catalog_logged: bool,
}

static STATE: std::sync::LazyLock<Mutex<State>> = std::sync::LazyLock::new(|| Mutex::new(State { looks: HashMap::new(), built: None, installed: None, catalog_logged: false }));

pub fn install(module: &Module, game_ui: usize) -> Result<(), String> {
    let find = |sig: &str| module.find(sig);
    unsafe {
        let hooks = Hooks {
            build: std::mem::transmute::<usize, BuildFn>(find(SIG_BUILD)?),
            frame: GenericDetour::new(std::mem::transmute::<usize, FrameFn>(find(SIG_FRAME)?), frame_hook).map_err(|e| e.to_string())?,
            unlock: GenericDetour::new(std::mem::transmute::<usize, UnlockFn>(find(SIG_UNLOCK)?), unlock_hook).map_err(|e| e.to_string())?,
            input: std::mem::transmute::<usize, InputFn>(find(SIG_INPUT)?),
            loca: GenericDetour::new(std::mem::transmute::<usize, LocaFn>(find(SIG_LOCA)?), loca_hook).map_err(|e| e.to_string())?,
            class_hints: GenericDetour::new(std::mem::transmute::<usize, FrameFn>(find(SIG_CLASS_HINTS)?), class_hints_hook).map_err(|e| e.to_string())?,
            chrome: GenericDetour::new(std::mem::transmute::<usize, FrameFn>(find(SIG_CHROME)?), chrome_hook).map_err(|e| e.to_string())?,
            item_icon: GenericDetour::new(std::mem::transmute::<usize, ItemIconFn>(find(SIG_ITEM_ICON)?), item_icon_hook).map_err(|e| e.to_string())?,
            game_ui,
        };
        let hooks = HOOKS.get_or_init(|| hooks);
        hooks.frame.enable().map_err(|e| e.to_string())?;
        hooks.unlock.enable().map_err(|e| e.to_string())?;
        hooks.loca.enable().map_err(|e| e.to_string())?;
        hooks.class_hints.enable().map_err(|e| e.to_string())?;
        hooks.chrome.enable().map_err(|e| e.to_string())?;
        hooks.item_icon.enable().map_err(|e| e.to_string())?;
    }
    log!("skill trees: mod trees can use the SKILLS page");
    Ok(())
}

pub fn uninstall() {
    if let Some(h) = HOOKS.get() {
        unsafe {
            let _ = h.frame.disable();
            let _ = h.unlock.disable();
            let _ = h.loca.disable();
            let _ = h.class_hints.disable();
            let _ = h.chrome.disable();
            let _ = h.item_icon.disable();
        }
    }
}

fn key(text: &str) -> u32 {
    // never 0 (no text) and clear of the game's keys only by chance: the prefix keeps it so in practice
    (shroudlink::string_id(format!("shroudlink-tree:{text}").as_bytes()) as u32) | 1
}

/// Builds the runtime nodes of a mod tree; `looks` lends icons of game nodes.
fn build(owner: &str, tree: &str, version: u64, nodes: &[TreeNode], links: &[TreeLink], looks: &HashMap<u32, Look>) -> Built {
    let nodes = &nodes[..nodes.len().min(MAX_ENTRIES)];
    let mut texts = HashMap::new();
    let mut runtime = vec![[0u8; NODE_SIZE]; nodes.len()].into_boxed_slice();
    for (n, raw) in nodes.iter().zip(runtime.iter_mut()) {
        let scope = format!("{owner}/{tree}/{}", n.id);
        let id = key(&scope);
        let look = looks.get(&n.icon).copied();
        let kind = match (n.max_rank, look) {
            (0, _) => 0,
            (_, Some(l)) if l.kind == 2 => 2,
            _ => 1,
        };
        let (name_key, desc_key) = (key(&format!("{scope}#name")), key(&format!("{scope}#description")));
        texts.insert(name_key, n.name.clone().into_boxed_str());
        texts.insert(desc_key, n.description.clone().into_boxed_str());
        raw[0..4].copy_from_slice(&id.to_le_bytes());
        raw[4] = kind;
        raw[8..12].copy_from_slice(&if n.name.is_empty() { 0 } else { name_key }.to_le_bytes());
        raw[0xc..0x10].copy_from_slice(&if n.description.is_empty() { 0 } else { desc_key }.to_le_bytes());
        raw[0x14..0x18].copy_from_slice(&n.x.to_le_bytes());
        raw[0x18..0x1c].copy_from_slice(&n.y.to_le_bytes());
        // cost 0: the game's own point check never stops a mod node; the mod decides
        raw[0x1e] = look.map_or(3, |l| l.class);
        if let Some(l) = look.filter(|_| kind == 2) {
            raw[0x20..0x40].copy_from_slice(&l.icons);
        }
        raw[0x40] = n.max_rank.min(255) as u8;
        raw[0x41] = n.rank.min(n.max_rank).min(255) as u8;
        raw[0x43] = (n.rank > 0 || n.max_rank == 0) as u8;
    }
    let index = |id: &str| nodes.iter().position(|n| n.id == id).map(|i| i as u16);
    let pairs = links.iter().filter_map(|l| Some((index(&l.from)?, index(&l.to)?))).take(MAX_PAIRS).collect();
    Built { owner: owner.into(), tree: tree.into(), version, nodes: runtime, node_ids: nodes.iter().map(|n| n.id.clone()).collect(), pairs, texts }
}

/// Keeps the look of every game node from the game's lists (and logs them once, for mod authors).
unsafe fn learn_game_looks(state: *mut u8, s: &mut State) {
    let count = (*(state.add(ENTRY_COUNT) as *const u16) as usize).min(MAX_ENTRIES);
    let mut catalog = Vec::new();
    for i in 0..count {
        let node = *(state.add(ENTRIES + i * ENTRY_SIZE + ENTRY_NODE) as *const *const u8);
        if node.is_null() {
            continue;
        }
        let id = *(node as *const u32);
        let mut icons = [0u8; 32];
        std::ptr::copy_nonoverlapping(node.add(0x20), icons.as_mut_ptr(), 32);
        let look = Look { kind: *node.add(4), class: *node.add(0x1e), icons };
        s.looks.insert(id, look);
        if !s.catalog_logged && look.kind == 2 {
            let name = crate::native::text_of(*(node.add(8) as *const u32)).unwrap_or_default();
            catalog.push(format!("{id:#010x} {name}"));
        }
    }
    if !s.catalog_logged && !catalog.is_empty() {
        s.catalog_logged = true;
        log!("skill tree catalog (game skills with an icon, for TreeNode.icon): {}", catalog.join("; "));
    }
}

/// Puts the built tree's lists into the page state.
unsafe fn put(state: *mut u8, b: &mut Built) {
    std::ptr::write_bytes(state.add(ENTRIES), 0, MAX_ENTRIES * ENTRY_SIZE);
    let mut starts = 0usize;
    for (i, node) in b.nodes.iter_mut().enumerate() {
        let e = state.add(ENTRIES + i * ENTRY_SIZE);
        std::ptr::copy_nonoverlapping(node.as_ptr(), e, 4);
        *(e.add(0x18) as *mut u32) = 0x7f7f_ffff;
        *(e.add(0x28) as *mut u16) = 0xff00;
        *(e.add(ENTRY_NODE) as *mut *mut u8) = node.as_mut_ptr();
        if node[4] == 0 && starts < MAX_STARTS {
            *(state.add(START_PTRS + starts * 8) as *mut *mut u8) = e;
            starts += 1;
        }
    }
    *(state.add(ENTRY_COUNT) as *mut u16) = b.nodes.len() as u16;
    *state.add(START_COUNT) = starts as u8;
    for (i, &(a, z)) in b.pairs.iter().enumerate() {
        *(state.add(PAIRS + i * 4) as *mut u16) = a;
        *(state.add(PAIRS + i * 4 + 2) as *mut u16) = z;
        for (from, to) in [(a, z), (z, a)] {
            let e = state.add(ENTRIES + from as usize * ENTRY_SIZE);
            let n = *e.add(ENTRY_NEIGHBOUR_COUNT) as usize;
            if n < MAX_NEIGHBOURS {
                *(e.add(ENTRY_NEIGHBOURS + n * 2) as *mut u16) = to;
                *e.add(ENTRY_NEIGHBOUR_COUNT) = n as u8 + 1;
            }
        }
    }
    *(state.add(PAIR_COUNT) as *mut u16) = b.pairs.len() as u16;
    for at in SELECTION {
        *(state.add(at) as *mut u64) = 0xffff;
    }
    *(state.add(REACH_COUNT) as *mut u16) = 0;
}

/// The page's lists point into our nodes.
unsafe fn ours(state: *mut u8, b: &Built) -> bool {
    let count = *(state.add(ENTRY_COUNT) as *const u16) as usize;
    let range = b.nodes.as_ptr() as usize..b.nodes.as_ptr() as usize + b.nodes.len() * NODE_SIZE;
    count > 0 && range.contains(&(*(state.add(ENTRIES + ENTRY_NODE) as *const usize)))
}

unsafe extern "system" fn frame_hook(state: *mut u8) {
    let hooks = HOOKS.get().unwrap();
    let wanted = crate::ui::shown_tree();
    {
        let mut s = STATE.lock().unwrap();
        let shown = s.installed.filter(|(at, _)| *at == state as usize);
        match wanted {
            Some((owner, tree, version, nodes, links)) => {
                if shown.map(|(_, v)| v) != Some(version) || !s.built.as_ref().is_some_and(|b| ours(state, b)) {
                    if s.installed.is_none() || shown.is_none() {
                        // the game's lists are still in place: keep their looks first
                        learn_game_looks(state, &mut s);
                    }
                    let mut built = build(&owner, &tree, version, &nodes, &links, &s.looks);
                    put(state, &mut built);
                    log!("skill trees: {owner} shows '{tree}' ({} nodes, {} links)", built.nodes.len(), built.pairs.len());
                    s.built = Some(built);
                    s.installed = Some((state as usize, version));
                }
            }
            None => {
                if s.installed.is_some() {
                    // the game's tree again: the page's own builder restores its lists
                    let tree = *(state.add(TREE) as *const *mut u8);
                    if shown.is_some() && !tree.is_null() {
                        (hooks.build)(state, tree);
                    }
                    s.installed = None;
                } else if !s.catalog_logged {
                    learn_game_looks(state, &mut s);
                }
            }
        }
    }
    hooks.frame.call(state)
}

/// The player's skill data (`*(*(*(game_ui + 0xd0)) + 0x38)`), if the game has one now.
unsafe fn player_skill_data(game_ui: usize) -> Option<*mut u8> {
    let ui = *(game_ui as *const usize);
    let holder = *((ui.checked_add(0xd0)?) as *const *const usize);
    if holder.is_null() || *holder == 0 {
        return None;
    }
    let data = *((*holder + 0x38) as *const *mut u8);
    (!data.is_null()).then_some(data)
}

/// The page's point counter shows the mod tree's points (swapped in for this call only) and
/// its emblem in the mod's tint.
unsafe extern "system" fn chrome_hook(state: *mut u8) {
    let hooks = HOOKS.get().unwrap();
    let shown = STATE.lock().unwrap().installed.is_some_and(|(at, _)| at == state as usize);
    let extras = if shown { crate::ui::shown_tree_extras() } else { None };
    let (points, tint) = extras.unwrap_or((None, None));
    let data = points.and_then(|_| player_skill_data(hooks.game_ui));
    let mut saved = [0u32; 3];
    if let (Some(data), Some([in_use, available])) = (data, points) {
        let p = data.add(POINTS_TOTAL) as *mut u32;
        for (i, v) in [in_use + available, in_use, available].into_iter().enumerate() {
            saved[i] = *p.add(i);
            *p.add(i) = v;
        }
    }
    ICON_TINT.store(tint.unwrap_or(0), std::sync::atomic::Ordering::Relaxed);
    hooks.chrome.call(state);
    ICON_TINT.store(0, std::sync::atomic::Ordering::Relaxed);
    if let (Some(data), Some(_)) = (data, points) {
        let p = data.add(POINTS_TOTAL) as *mut u32;
        for (i, v) in saved.into_iter().enumerate() {
            *p.add(i) = v;
        }
    }
}

unsafe extern "system" fn item_icon_hook(item: u32, colour_and_scale: u64, a: u64, b: u64, c: u64, d: u64) -> u64 {
    let tint = ICON_TINT.load(std::sync::atomic::Ordering::Relaxed);
    let colour_and_scale = if tint != 0 && item == SKILL_POINT_ITEM { (colour_and_scale & !0xffff_ffff) | tint as u64 } else { colour_and_scale };
    HOOKS.get().unwrap().item_icon.call(item, colour_and_scale, a, b, c, d)
}

/// The game's class labels belong to its own tree: not drawn over a mod tree.
unsafe extern "system" fn class_hints_hook(state: *mut u8) {
    if STATE.lock().unwrap().installed.is_some_and(|(at, _)| at == state as usize) {
        return;
    }
    HOOKS.get().unwrap().class_hints.call(state)
}

unsafe extern "system" fn unlock_hook(entry: *mut u8, flag: u64) {
    let hooks = HOOKS.get().unwrap();
    let node = if entry.is_null() { 0 } else { *(entry.add(ENTRY_NODE) as *const usize) };
    let target = {
        let s = STATE.lock().unwrap();
        s.built.as_ref().and_then(|b| {
            let start = b.nodes.as_ptr() as usize;
            (node >= start && node < start + b.nodes.len() * NODE_SIZE).then(|| (b.owner.clone(), b.tree.clone(), b.node_ids[(node - start) / NODE_SIZE].clone()))
        })
    };
    match target {
        // a mod node: the invest key goes to the mod, never to the game's unlock
        Some((owner, tree, node)) => {
            if (hooks.input)(1, 1) & 0xff != 0 {
                log!("skill trees: {owner} '{tree}': invest in {node}");
                crate::link::send(UiEvent { window: tree, node, event: Event::Click { index: None } }.to_message(&owner));
            }
        }
        None => hooks.unlock.call(entry, flag),
    }
}

unsafe extern "system" fn loca_hook(out: *mut UiStr, key: u32) -> *mut UiStr {
    let hooks = HOOKS.get().unwrap();
    if let Ok(s) = STATE.try_lock() {
        if let Some(text) = s.built.as_ref().and_then(|b| b.texts.get(&key)) {
            *out = UiStr::hashed(text);
            return out;
        }
    }
    hooks.loca.call(out, key)
}
