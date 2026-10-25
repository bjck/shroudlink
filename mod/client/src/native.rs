//! The game's immediate-mode UI (keen UI), called directly so mod windows look
//! and behave like the game's own.
//!
//! The game builds its UI every frame: a layer (`begin_layer`) holds a tree of
//! frames that are opened and closed like a stack (`open_*` / `close_frame`).
//! Widgets are frames with content, e.g. a text widget (`text_new` + `text_emit`).
//! All objects live on the caller's stack; we give them generous zeroed buffers
//! because only the fields the game's own callers initialise are known.
//!
//! Globals: `UI_BUILDER` (`DAT_7ff624e2b3f8`, +0x18 current frame builder) and
//! `GAME_UI` (`DAT_7ff624e2b498`, +0xc8 theme with fonts and text styles).

use std::ffi::c_void;
use std::sync::OnceLock;

use chestsort_common::log;
use chestsort_common::scan::Module;
use iced_x86::{Decoder, DecoderOptions, OpKind, Register};
use retour::GenericDetour;

/// `FUN_7ff62325e0c0(void)`: builds the whole in-game UI each frame (HUD, then the active screen).
const SIG_UI_DISPATCH: &str = "4C 8B DC 55 49 8D AB E8 FD FF FF 48 81 EC 10 03 00 00";
const SIG_OPEN_ROW: &str = "40 53 48 83 EC 30 0F 29 74 24 20 48 8B D9 0F 28 F2 E8 ?? ?? ?? ?? 48 8B 4B 08 48 8D 54 24 40 45 33 C0 F3 0F 11 74 24 40";
const SIG_BACKGROUND: &str = "48 8B C4 55 41 56 48 8D A8 B8 FE FF FF";
const SIG_SLOT_DESC_INIT: &str = "48 83 EC 28 0F 10 05 ?? ?? ?? ?? 48 C7 41 18 FF FF FF FF";
/// `FUN_7ff6230ff3d0(&text, locaId)`: localised text of a loca tag id.
/// `FUN_7ff623150780(panel, index, ...)`: an inventory panel of the chest screen (title, then
/// the slot grid); index 0 is the backpack.
const SIG_PANEL_HEADER: &str = "40 55 41 54 48 8D AC 24 68 D9 FF FF";
/// `FUN_7ff6231f9000(panel, inventory, state)`: the slot grid of an inventory panel; the
/// backpack panel (`FUN_7ff623150780`) calls it right after its title, in a column layout.
const SIG_SLOT_GRID: &str = "48 89 74 24 18 55 57 41 56 48 8D AC 24 20 FF FF FF";
const SIG_LOCA_TEXT: &str = "48 89 5C 24 10 48 89 74 24 18 55 57 41 55 41 56 41 57 48 8D AC 24 F0 F8 FF FF";
/// `FUN_7ff62348a870(itemId, registry)`: the item's `ItemInfo` resource.
const SIG_ITEM_INFO: &str = "89 4C 24 08 48 83 EC 28 85 C9 74 ?? 48 8D 4A 38 48 8D 54 24 30 E8 ?? ?? ?? ?? 48 85 C0 74 ??";
/// `ItemInfo.name` (a 16 byte loca tag, see the reflection data).
const ITEM_INFO_NAME: usize = 0x48;
/// Item registry in the game state (`doItemSlot` reads it there).
const STATE_ITEM_REGISTRY: usize = 0x4667b8;
/// `FUN_7ff623315950(&render, desc, state, drag)`: turns a slot descriptor into render parameters.
const SIG_SLOT_RENDER: &str = "48 8B C4 48 89 58 10 48 89 68 18 48 89 70 20 57 41 54 41 55 41 56 41 57 48 81 EC B0 00 00 00 0F 29 70 C8";
/// Render parameters: icon tint (ARGB colour + f32) and its "active" flag; the game uses a
/// grey tint (0xe6808080) for unavailable items.
const RENDER_TINT: usize = 0x38;
const RENDER_TINT_ON: usize = 0x40;
/// Tint of slots that do not match the search: dark grey, mostly transparent.
const DIMMED_TINT: u64 = 0x3f80_0000_4c50_5050;
/// Rim around locked slots: width in UI units and panel style (10: gold rim).
const LOCK_RIM: f32 = 3.0;
const LOCK_RIM_STYLE: u32 = 10;
const SIG_ITEM_SLOT: &str = "48 8B C4 4C 89 40 18 48 89 48 08 55 53 56 57 41 55";
/// `FUN_7ff623320400(menuId, &list)`: fills the tab entries of a menu (player menu: id 3).
/// `FUN_7ff6231e7700(menu, mode)`: the player menu (tab row, header, current page).
const SIG_PLAYER_MENU: &str = "89 54 24 10 88 4C 24 08 55 53 56 57";
/// Page index the player menu dispatches on (game state + 0x31e70; 0 social .. 6 skills).
const PLAYER_PAGE_OFFSET: usize = 0x31e70;
/// A page index without a page: the player menu then draws only its frame.
const NO_PAGE: u64 = 7;
const SIG_TAB_LIST: &str = "48 8B C4 48 89 58 08 48 89 50 10 55 56 57 41 54 41 55 41 56 41 57 48 8D 68 A1";
/// `FUN_7ff6232de670(&entry, &rect)`: draws one tab of a tab row; true when it is the active tab.
const SIG_TAB_WIDGET: &str = "40 55 53 56 48 8D AC 24 60 FD FF FF";
const SIG_BEGIN_LAYER: &str = "48 89 5C 24 08 48 89 74 24 10 57 48 83 EC 50 48 8B 05 ?? ?? ?? ?? 45 8B D9";
const SIG_END_LAYER: &str = "40 53 48 83 EC 20 48 89 74 24 38 48 8B D9";
const SIG_OPEN_ROOT: &str = "40 53 48 83 EC 20 48 8B D9 48 C7 41 10 00 00 00 00 48 C7 01 00 00 00 00";
const SIG_OPEN_CHILD: &str = "40 53 48 83 EC 20 48 8B 05 ?? ?? ?? ?? 48 8B D9 45 33 C0";
const SIG_OPEN_COLUMN: &str = "40 53 48 83 EC 30 0F 29 74 24 20 48 8B D9 0F 28 F2 E8 ?? ?? ?? ?? 48 8B 4B 08 48 8D 54 24 40 45 33 C0 F3 0F 11 74 24 44";
const SIG_CLOSE_FRAME: &str = "48 89 5C 24 18 48 89 74 24 20 57 48 83 EC 60 80 79 20 00";
const SIG_TEXT_NEW: &str = "48 89 5C 24 08 48 89 74 24 10 48 89 7C 24 18 41 56 48 83 EC 50 48 8B DA 41 0F B6 F9";
const SIG_TEXT_EMIT: &str = "40 55 56 57 48 8D 6C 24 B9 48 81 EC F0 00 00 00 0F 29 BC 24 B0 00 00 00";
/// `doQrCodeViewerPopup`, only used to find the `GAME_UI` global (`mov rax, [rip+X]` at +0x13).
/// `FUN_7ff62311def0`: makes the inventory screen's state (default slot records).
const SIG_TIP_STATE_INIT: &str = "48 8B 05 ?? ?? ?? ?? 33 D2 48 89 41 08";
/// `FUN_7ff62311e580(tooltip, slot record, comparison)`: builds an item tooltip.
const SIG_TIP_INIT: &str = "48 89 5C 24 08 48 89 6C 24 10 48 89 74 24 18 57 48 83 EC 40 48 8B 05 ?? ?? ?? ??";
/// `FUN_7ff623212e00(tooltip, rect)`: draws it next to the slot's rectangle.
const SIG_TIP_DRAW: &str = "4C 8B DC 49 89 5B 10 57 48 81 EC 00 01 00 00";
const SIG_QR_POPUP: &str = "4C 8B DC 55 57 49 8D AB 38 F8 FF FF";

/// Text style used by `doSimpleTextTabs` (theme + 0x11b94).
pub const STYLE_TAB_TEXT: usize = 0x11b94;
/// Label style of the player menu tabs (theme + 0x11b3c).
pub const STYLE_TOP_TAB: usize = 0x11b3c;
/// Title style of the QR popup (theme + 0x10a0c).
pub const STYLE_TITLE: usize = 0x10a0c;
/// Layer kinds (z-order): the HUD passes 5, the chest screen 6, popups above menus 0x1d.
pub const LAYER_HUD: u32 = 5;
pub const LAYER_POPUP: u32 = 0x1d;
/// The MODS page: above the player menu's content (the inventory screen uses 6) and below
/// the game's item tooltips (0xe, opened by `FUN_7ff623212e00`), which must cover it.
pub const LAYER_PAGE: u32 = 0xd;

/// Name/text argument: pointer, length and a 64 bit key (FNV-1a of the name for frame ids).
#[repr(C)]
pub struct UiStr {
    ptr: *const u8,
    len: usize,
    key: u64,
}

impl UiStr {
    pub fn name(s: &'static str) -> UiStr {
        UiStr { ptr: s.as_ptr(), len: s.len(), key: shroudlink::string_id(s.as_bytes()) }
    }
    /// A frame name made unique by `index`: the game identifies frames across
    /// frames by the key, so siblings with the same name would merge into one.
    pub fn keyed(s: &'static str, index: u64) -> UiStr {
        let key = shroudlink::string_id(s.as_bytes()) ^ index.wrapping_mul(0x9e37_79b9_7f4a_7c15);
        UiStr { ptr: s.as_ptr(), len: s.len(), key }
    }
    pub fn id(&self) -> u64 {
        self.key
    }
    pub fn text(s: &str) -> UiStr {
        UiStr { ptr: s.as_ptr(), len: s.len(), key: 0 }
    }
}

type DispatchFn = unsafe extern "system" fn();
type PlayerMenuFn = unsafe extern "system" fn(menu: u8, mode: i32) -> u64;
type TabListFn = unsafe extern "system" fn(menu: u8, list: *mut TabList) -> *mut c_void;
type TabWidgetFn = unsafe extern "system" fn(entry: *mut u8, rect: *mut f32) -> u64;

/// The game's small vector of 0x40 byte tab entries.
#[repr(C)]
pub struct TabList {
    pub data: *mut u8,
    pub len: usize,
}

/// Social, World, Journal, Crafting, Backpack, Character, Skills.
const PLAYER_MENU_TABS: usize = 7;
const TAB_ENTRY_SIZE: usize = 0x40;
type OpenRowFn = OpenColumnFn;
type BackgroundFn = unsafe extern "system" fn(frame: *mut u8, desc: *const u8);
/// `FUN_7ff62304a800(frame, rect, colour)`: a solid rectangle (screen UI units; colour
/// 0xAABBGGRR, then f32 1.0). The 10th and 17th call in the background function.
type FillFn = unsafe extern "system" fn(frame: *mut u8, rect: *const [f32; 4], colour: *const [u32; 2]);
type SlotDescInitFn = unsafe extern "system" fn(desc: *mut u8) -> *mut u8;
type TipStateInitFn = unsafe extern "system" fn(state: *mut u8) -> *mut u8;
type TipInitFn = unsafe extern "system" fn(tip: *mut u8, slot: *const u8, compare: *const u8) -> *mut u8;
type TipDrawFn = unsafe extern "system" fn(tip: *mut u8, rect: *const [f32; 4]);
type ItemSlotFn = unsafe extern "system" fn(out: *mut u8, desc: *mut u8, compare: *mut u8, drag: usize) -> *mut u8;
type SlotRenderFn = unsafe extern "system" fn(render: *mut u8, desc: *mut u8, state: usize, drag: usize);
type PanelHeaderFn = unsafe extern "system" fn(panel: *mut c_void, index: u8, extra: u64);
type SlotGridFn = unsafe extern "system" fn(panel: *mut c_void, inventory: u64, state: *mut c_void);
type LocaTextFn = unsafe extern "system" fn(out: *mut UiStr, id: u32) -> *mut UiStr;
type ItemInfoFn = unsafe extern "system" fn(item: u32, registry: usize) -> *const u8;

type BeginLayerFn = unsafe extern "system" fn(layer: *mut u8, name: *const UiStr, a: u32, kind: u32, b: u32, c: u32) -> *mut u8;
type EndLayerFn = unsafe extern "system" fn(layer: *mut u8);
type OpenRootFn = unsafe extern "system" fn(frame: *mut u8, root: *mut c_void) -> *mut u8;
type OpenChildFn = unsafe extern "system" fn(frame: *mut u8, id: u64) -> *mut u8;
type OpenColumnFn = unsafe extern "system" fn(frame: *mut u8, name: *const UiStr, spacing: f32) -> *mut u8;
type CloseFrameFn = unsafe extern "system" fn(frame: *mut u8);
type TextNewFn = unsafe extern "system" fn(text: *mut u8, s: *const UiStr, style: *const c_void, flag: u8) -> *mut u8;
type TextEmitFn = unsafe extern "system" fn(text: *mut u8);

pub struct Api {
    dispatch: GenericDetour<DispatchFn>,
    tab_list: GenericDetour<TabListFn>,
    player_menu: GenericDetour<PlayerMenuFn>,
    tab_widget: GenericDetour<TabWidgetFn>,
    open_row: OpenRowFn,
    background: BackgroundFn,
    fill: FillFn,
    slot_desc_init: SlotDescInitFn,
    item_slot: GenericDetour<ItemSlotFn>,
    slot_render: GenericDetour<SlotRenderFn>,
    panel_header: GenericDetour<PanelHeaderFn>,
    slot_grid: GenericDetour<SlotGridFn>,
    loca_text: LocaTextFn,
    item_info: ItemInfoFn,
    begin_layer: BeginLayerFn,
    end_layer: EndLayerFn,
    open_root: OpenRootFn,
    open_child: OpenChildFn,
    open_column: OpenColumnFn,
    close_frame: CloseFrameFn,
    text_new: TextNewFn,
    text_emit: TextEmitFn,
    game_ui: usize,
    tips: Option<ItemTips>,
}

/// The game's item tooltip, as the inventory screen (`FUN_7ff62319c4c0`) draws it: its
/// slots record the hovered item in a screen state (0xbc8 bytes, made by
/// `FUN_7ff62311def0`; the slot record at +0x50, its rectangle at +0xb58, comparison data
/// at +0x248), and after the slots it builds the tooltip (`FUN_7ff62311e580`) and draws it
/// at that rectangle (`FUN_7ff623212e00`).
struct ItemTips {
    state: *mut u8,
    init: TipInitFn,
    draw: TipDrawFn,
}

const TIP_STATE_SIZE: usize = 0xbc8;
/// Where the tooltip of this frame opens at the earliest (f32 bits; 0: at its slot).
static TIP_BESIDE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
const TIP_SLOT: usize = 0x50;
const TIP_RECT: usize = 0xb58;
const TIP_COMPARE: usize = 0x248;

unsafe impl Send for Api {}
unsafe impl Sync for Api {}

static API: OnceLock<Api> = OnceLock::new();

/// Address of the first `[rip + disp]` operand in the first `len` bytes at `code`.
fn rip_global(code: usize, len: usize) -> Option<usize> {
    let bytes = unsafe { std::slice::from_raw_parts(code as *const u8, len) };
    let mut decoder = Decoder::with_ip(64, bytes, code as u64, DecoderOptions::NONE);
    for insn in &mut decoder {
        for i in 0..insn.op_count() {
            if insn.op_kind(i) == OpKind::Memory && insn.memory_base() == Register::RIP {
                return Some(insn.memory_displacement64() as usize);
            }
        }
    }
    None
}

/// Stack storage for game UI objects.
#[repr(C, align(16))]
pub struct Obj<const N: usize>([u8; N]);

impl<const N: usize> Obj<N> {
    pub fn new() -> Self {
        Obj([0; N])
    }
    pub fn ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }
    /// The frame handle (second word of every frame object).
    pub fn handle(&self) -> *mut u8 {
        unsafe { *(self.0.as_ptr().add(8) as *const *mut u8) }
    }
}

pub type Frame = Obj<0x100>;
pub type Text = Obj<0x400>;
pub type Layer = Obj<0x400>;

impl Api {
    fn theme(&self) -> *const u8 {
        unsafe { *((*(self.game_ui as *const usize) + 0xc8) as *const *const u8) }
    }

    pub unsafe fn begin_layer(&self, layer: &mut Layer, name: &UiStr, kind: u32) -> *mut c_void {
        (self.begin_layer)(layer.ptr(), name, 1, kind, 0xffff_ffff, 0);
        *(layer.ptr().add(0x10) as *const *mut c_void)
    }

    pub unsafe fn end_layer(&self, layer: &mut Layer) {
        (self.end_layer)(layer.ptr());
    }

    pub unsafe fn open_root(&self, frame: &mut Frame, root: *mut c_void) {
        (self.open_root)(frame.ptr(), root);
    }


    /// Named frame stacking its children vertically.
    pub unsafe fn open_column(&self, frame: &mut Frame, name: &UiStr, spacing: f32) {
        (self.open_column)(frame.ptr(), name, spacing);
    }

    /// Alignment of a frame inside its parent: low byte horizontal, high byte vertical (1 start, 2 centre, 3 end).
    pub unsafe fn set_alignment(&self, frame: &Frame, align: u16) {
        *(frame.handle().add(0x6c) as *mut u16) = align;
    }

    pub unsafe fn close_frame(&self, frame: &mut Frame) {
        (self.close_frame)(frame.ptr());
    }

    pub unsafe fn text(&self, s: &str, style: usize) {
        let mut text = Text::new();
        let arg = UiStr::text(s);
        (self.text_new)(text.ptr(), &arg, self.theme().add(style) as *const c_void, 0);
        (self.text_emit)(text.ptr());
    }

    /// Text that reacts to the mouse like the game's tab labels; true when clicked.
    /// `id` must be unique among the frames of a parent and stable across frames:
    /// the game tracks hover and press state per frame id.
    pub unsafe fn text_button(&self, s: &str, style: usize, id: u64) -> bool {
        let mut frame = Frame::new();
        (self.open_child)(frame.ptr(), id);
        let mut text = Text::new();
        let arg = UiStr::text(s);
        (self.text_new)(text.ptr(), &arg, self.theme().add(style) as *const c_void, 0);
        (self.text_emit)(text.ptr());
        // ask the button frame, not the text: texts are created with id 0, so all
        // button texts would share one press/release state
        let clicked = mouse_click_in(self.rect(&frame));
        (self.close_frame)(frame.ptr());
        clicked
    }

    /// Named frame stacking its children horizontally.
    pub unsafe fn open_row(&self, frame: &mut Frame, name: &UiStr, spacing: f32) {
        (self.open_row)(frame.ptr(), name, spacing);
    }

    /// Fixed size of a frame in UI units.
    pub unsafe fn set_size(&self, frame: &Frame, w: f32, h: f32) {
        let n = frame.handle();
        for off in [0x78, 0x80, 0x88] {
            *(n.add(off) as *mut f32) = w;
            *(n.add(off + 4) as *mut f32) = h;
        }
        *(n.add(0x90) as *mut u64) = 0;
    }

    /// Layout rectangle of a frame from the last layout pass (node + 0x114, 4 floats).
    pub unsafe fn rect(&self, frame: &Frame) -> [f32; 4] {
        *(frame.handle().add(0x114) as *const [f32; 4])
    }


    /// Inner padding: left, top, right, bottom.
    pub unsafe fn set_padding(&self, frame: &Frame, padding: [f32; 4]) {
        std::ptr::copy_nonoverlapping(padding.as_ptr(), frame.handle().add(0xa8) as *mut f32, 4);
    }

    /// Panel background in one of the game's styles (0..=10; the QR popup uses 8).
    /// Descriptor (FUN_7ff6232fd910): +0 style, +4 opacity of the whole background, +8 the
    /// highlight look, +0x68 two {rect, colour} rectangles, +0x98 their count. Styles 4–6
    /// reach past the frame on one side, and every style ends with a texture that masks
    /// what it drew (style 7 fades towards the top): solid boxes use `fill`.
    pub unsafe fn background(&self, frame: &mut Frame, style: u32, alpha: f32) {
        let mut desc = [0u32; 0x40];
        desc[0] = style;
        desc[1] = alpha.to_bits();
        (self.background)(frame.ptr(), desc.as_ptr() as *const u8);
    }

    /// A solid rectangle on `frame`, drawn without any panel texture (whose mask would fade
    /// it at the edges). `rect` is on screen in UI units, e.g. a frame's last layout rect.
    pub unsafe fn fill(&self, frame: &mut Frame, rect: [f32; 4], colour: u32) {
        (self.fill)(frame.ptr(), &rect, &[colour, 1.0f32.to_bits()]);
    }

    /// An inventory slot showing `count` x `item` (icon, rarity frame): (clicked, mouse over
    /// it). The game's own tooltip needs the inventory screen's context and does not show.
    pub unsafe fn item_slot(&self, item: u32, count: u32, size: f32) -> (bool, bool) {
        // client inventory slot record (0x48 bytes): slot id (0 = none), item id at +0x8,
        // stack count at +0x10 (the slot's render reads it there for the number)
        let mut slot = [0u64; 9];
        slot[1] = item as u64;
        slot[2] = count as u64;
        let mut desc = Obj::<0x200>::new();
        (self.slot_desc_init)(desc.ptr());
        let mut highlight = 0u8;
        *(desc.ptr() as *mut *const u64) = slot.as_ptr();
        *(desc.ptr().add(0x10) as *mut *mut u8) = &mut highlight;
        *(desc.ptr().add(0x6c) as *mut f32) = size;
        let mut compare = Obj::<0x500>::new();
        let mut out = Obj::<0x40>::new();
        // with a screen state the slot records the hovered item for its tooltip
        let state = self.tips.as_ref().map_or(0, |t| t.state as usize);
        self.item_slot.call(out.ptr(), desc.ptr(), compare.ptr(), state);
        // out starts with the slot rectangle
        let rect = *(out.ptr() as *const [f32; 4]);
        (mouse_click_in(rect), mouse_in(rect))
    }

    /// Displayed (localised) name of an item, lower case (for searching).
    pub unsafe fn item_name(&self, item: u32) -> Option<String> {
        self.item_display_name(item).map(|n| n.to_lowercase())
    }

    /// Displayed (localised) name of an item, as the game shows it; cached.
    pub unsafe fn item_display_name(&self, item: u32) -> Option<String> {
        static NAMES: std::sync::Mutex<Option<std::collections::HashMap<u32, String>>> = std::sync::Mutex::new(None);
        let mut names = NAMES.lock().unwrap();
        let names = names.get_or_insert_with(Default::default);
        if let Some(n) = names.get(&item) {
            return Some(n.clone());
        }
        let holder = self.holder()?;
        let state = *((*(holder as *const usize)) as *const u8).add(0x38).cast::<usize>();
        if state == 0 {
            return None;
        }
        let registry = *((state + STATE_ITEM_REGISTRY) as *const usize);
        let info = (self.item_info)(item, registry);
        if info.is_null() {
            return None;
        }
        // loca tag id: FNV-1a 32 over the 16 tag bytes (FUN_7ff622a37650)
        let tag = std::slice::from_raw_parts(info.add(ITEM_INFO_NAME), 16);
        let id = tag.iter().fold(0x811c9dc5u32, |h, &b| (h ^ b as u32).wrapping_mul(0x0100_0193));
        let mut out = UiStr { ptr: std::ptr::null(), len: 0, key: 0 };
        (self.loca_text)(&mut out, id);
        if out.ptr.is_null() || out.len == 0 || out.len > 256 {
            return None;
        }
        let name = String::from_utf8_lossy(std::slice::from_raw_parts(out.ptr, out.len)).into_owned();
        names.insert(item, name.clone());
        Some(name)
    }

    /// Starts a frame of item slots with tooltips: no item hovered yet.
    pub unsafe fn begin_item_tips(&self) {
        if let Some(t) = &self.tips {
            std::ptr::write_bytes(t.state.add(TIP_RECT), 0, 0x20);
        }
        TIP_BESIDE.store(0, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether item slots show the game's tooltip (else a mod page names the item itself).
    pub fn has_item_tips(&self) -> bool {
        self.tips.is_some()
    }

    /// The tooltip opens right of `x` (the grid's right edge): item icons are drawn after
    /// the UI, so a tooltip over the grid would sit under them.
    pub fn item_tip_beside(&self, x: f32) {
        TIP_BESIDE.store(x.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }

    /// The game's tooltip for the item slot under the cursor (drawn by `item_slot` since
    /// `begin_item_tips`), with the stat comparison against what the player wears.
    pub unsafe fn draw_item_tip(&self) {
        let Some(t) = &self.tips else { return };
        let mut rect = *(t.state.add(TIP_RECT) as *const [f32; 4]);
        if rect[2] <= 0.0 || rect[3] <= 0.0 {
            return;
        }
        let beside = f32::from_bits(TIP_BESIDE.load(std::sync::atomic::Ordering::Relaxed));
        if beside > rect[0] + rect[2] {
            rect[0] = beside - rect[2];
        }
        let mut tip = Obj::<0x800>::new();
        (t.init)(tip.ptr(), t.state.add(TIP_SLOT), t.state.add(TIP_COMPARE));
        // as the inventory screen sets them (offsets in its tooltip descriptor)
        *(tip.ptr().add(0xbc) as *mut u32) = 2;
        *(tip.ptr().add(0xc0) as *mut u32) = 1;
        let colour = tip.ptr().add(0xcc) as *mut u32;
        *colour = *colour & 0x00ff_ffff | 0xe500_0000;
        *(tip.ptr().add(0xdc) as *mut f32) = 250.0;
        *(tip.ptr().add(0x168) as *mut usize) = self.local_player().unwrap_or(0);
        (t.draw)(tip.ptr(), &rect);
    }

    /// The local player's record in the game state, for the tooltip's comparison: the
    /// entry of the player list (`state + 0x3060`, count at +0x3068, 0x8f70 bytes each)
    /// whose id (+0) is the local player's (`state + 0x2c50`).
    unsafe fn local_player(&self) -> Option<usize> {
        let holder = self.holder()?;
        let state = *((*(holder as *const usize)) as *const u8).add(0x38).cast::<usize>();
        if state == 0 {
            return None;
        }
        let id = *((state + 0x2c50) as *const i32);
        let list = *((state + 0x3060) as *const usize);
        let count = *((state + 0x3068) as *const usize);
        (0..count.min(64)).map(|i| list + i * 0x8f70).find(|&p| *(p as *const i32) == id)
    }

    /// The game state holder (`*(GAME_UI + 0xd0)`), if a game is running.
    unsafe fn holder(&self) -> Option<usize> {
        let ui = *(self.game_ui as *const usize);
        let holder = *((ui + 0xd0) as *const *const usize);
        (!holder.is_null() && *holder != 0).then_some(holder as usize)
    }

    /// Index of the active player menu tab (holder + 0x32a90), if a game is running.
    pub unsafe fn player_tab(&self) -> Option<u64> {
        self.holder().map(|h| *((h + 0x32a90) as *const u64))
    }

    /// The active game screen (byte at `gameState + 0x1cd8`); 0 when none.
    pub unsafe fn screen(&self) -> u8 {
        let ui = *(self.game_ui as *const usize);
        let holder = *((ui + 0xd0) as *const *const usize);
        if holder.is_null() || *holder == 0 {
            return 0;
        }
        let state = *((*holder + 0x38) as *const usize);
        if state == 0 {
            return 0;
        }
        *((state + 0x1cd8) as *const u8)
    }
}


/// Key (entry +0x28) of the last player menu tab, after which the MODS tab is drawn.
static LAST_PLAYER_TAB: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

unsafe extern "system" fn tab_list_hook(menu: u8, list: *mut TabList) -> *mut c_void {
    let api = API.get().unwrap();
    let r = api.tab_list.call(menu, list);
    // the player menu passes the id of its current page; it is the only tab row with 7 tabs
    if !list.is_null() && (*list).len == PLAYER_MENU_TABS {
        let last = (*list).data.add(((*list).len - 1) * TAB_ENTRY_SIZE);
        LAST_PLAYER_TAB.store(*(last.add(0x28) as *const u64), std::sync::atomic::Ordering::Relaxed);
    }
    r
}

unsafe extern "system" fn tab_widget_hook(entry: *mut u8, rect: *mut f32) -> u64 {
    let api = API.get().unwrap();
    let key = *(entry.add(0x28) as *const u64);
    let last = LAST_PLAYER_TAB.load(std::sync::atomic::Ordering::Relaxed);
    let mut active = api.tab_widget.call(entry, rect);
    if last != 0 && key == last {
        // right after the last game tab: the MODS tab, in the same tab row
        crate::ui::draw_mods_tab(api);
    }
    // while the MODS page is open no game tab is shown as active
    if active & 0xff != 0 && crate::ui::mods_page_open() {
        active &= !0xff;
    }
    active
}

unsafe extern "system" fn player_menu_hook(menu: u8, mode: i32) -> u64 {
    let api = API.get().unwrap();
    let page = api.holder().map(|h| (h + PLAYER_PAGE_OFFSET) as *mut u64);
    match page {
        // the MODS page replaces the game page: hide it for this call only
        Some(page) if crate::ui::mods_page_open() => {
            let saved = *page;
            *page = NO_PAGE;
            let r = api.player_menu.call(menu, mode);
            *page = saved;
            r
        }
        _ => api.player_menu.call(menu, mode),
    }
}

/// Mouse state in UI units (1920 x 1080 reference, like frame rectangles), updated
/// once per UI frame. The game's own per-frame interaction state does not cope
/// with windows whose content is replaced from outside, so mod buttons test the
/// cursor against their layout rectangle themselves.
struct Mouse {
    pos: (f32, f32),
    down: bool,
    pressed_at: Option<(f32, f32)>,
    /// set in the frame the button was released; consumed by the first widget hit
    click: Option<((f32, f32), (f32, f32))>,
}

static MOUSE: std::sync::Mutex<Mouse> = std::sync::Mutex::new(Mouse { pos: (0.0, 0.0), down: false, pressed_at: None, click: None });

/// UI reference height; widths scale with the window's aspect ratio.
const UI_HEIGHT: f32 = 1080.0;

unsafe fn update_mouse() {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowThreadProcessId};
    let mut m = MOUSE.lock().unwrap();
    m.click = None;
    let hwnd = GetForegroundWindow();
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid != std::process::id() {
        m.down = false;
        m.pressed_at = None;
        return;
    }
    let mut p = POINT { x: 0, y: 0 };
    GetCursorPos(&mut p);
    ScreenToClient(hwnd, &mut p);
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(hwnd, &mut r);
    if r.bottom <= 0 {
        return;
    }
    let scale = UI_HEIGHT / r.bottom as f32;
    m.pos = (p.x as f32 * scale, p.y as f32 * scale);
    let down = GetAsyncKeyState(VK_LBUTTON as i32) as u16 & 0x8000 != 0;
    if down && !m.down {
        m.pressed_at = Some(m.pos);
    }
    if !down && m.down {
        if let Some(at) = m.pressed_at.take() {
            m.click = Some((at, m.pos));
        }
    }
    m.down = down;
}

/// The game window's client size in pixels (0, 0 when unknown).
pub fn client_size() -> (i32, i32) {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetClientRect};
    let title: Vec<u16> = "Enshrouded ".encode_utf16().collect();
    unsafe {
        let hwnd = FindWindowW(std::ptr::null(), title.as_ptr());
        let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if !hwnd.is_null() {
            GetClientRect(hwnd, &mut r);
        }
        (r.right, r.bottom)
    }
}

/// Cursor position in UI units.
pub fn mouse_pos() -> (f32, f32) {
    MOUSE.lock().unwrap().pos
}

/// True when a click (press + release) happened this frame and was not taken by a widget yet.
pub fn mouse_clicked() -> bool {
    MOUSE.lock().unwrap().click.is_some()
}

/// While the left button is held: where it was pressed and where the cursor is now.
pub fn mouse_held() -> Option<((f32, f32), (f32, f32))> {
    let m = MOUSE.lock().unwrap();
    m.pressed_at.filter(|_| m.down).map(|at| (at, m.pos))
}

/// True when the cursor is inside `rect` (x, y, w, h).
pub fn mouse_in(rect: [f32; 4]) -> bool {
    let (x, y) = mouse_pos();
    x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3]
}

/// True (once) when the button was pressed and released inside `rect` (x, y, w, h).
pub fn mouse_click_in(rect: [f32; 4]) -> bool {
    let inside = |(x, y): (f32, f32)| x >= rect[0] && x < rect[0] + rect[2] && y >= rect[1] && y < rect[1] + rect[3];
    let mut m = MOUSE.lock().unwrap();
    match m.click {
        Some((from, to)) if inside(from) && inside(to) => {
            m.click = None;
            true
        }
        _ => false,
    }
}

/// Every inventory slot the game draws: slots whose item does not match the
/// active item search are drawn faded.
unsafe extern "system" fn item_slot_hook(out: *mut u8, desc: *mut u8, compare: *mut u8, drag: usize) -> *mut u8 {
    let api = API.get().unwrap();
    let slot = *(desc as *const *const u8);
    let item = if slot.is_null() { 0 } else { *(slot.add(8) as *const u32) };
    // the player's backpack slots: mods mark them and act on them with slot keys; the chest
    // screen's backpack panel tells where each page sits, other views look that up
    let id = if slot.is_null() { 0 } else { crate::slots::slot_id(*(slot as *const u64)) };
    let position = if id & 0xffff_ffff == 0 {
        None
    } else if BACKPACK_SLOTS.load(std::sync::atomic::Ordering::Relaxed) {
        Some(crate::slots::observe(id))
    } else {
        crate::slots::position_of(id)
    };
    let mark = position.and_then(|_| crate::slots::mark_of(id));
    let rim_key = position.map_or(0, |(page, index)| (page as u64) << 8 | index as u64);
    let dim = item != 0
        && crate::ui::with_search(|query| api.item_name(item).is_some_and(|name| !query.split_whitespace().all(|w| name.contains(w))))
            .unwrap_or(false);
    // the render parameters are built inside doItemSlot (slot_render_hook)
    let dim = dim || mark == Some(shroudlink::ui::MarkStyle::Dim);
    SLOT_TINT.store(if dim { DIMMED_TINT } else { 0 }, std::sync::atomic::Ordering::Relaxed);
    let r = if mark == Some(shroudlink::ui::MarkStyle::Rim) {
        // rim: the slot sits in a gold-rimmed frame of its own size, shrunk by the rim
        let size_ptr = desc.add(0x6c) as *mut f32;
        let size = *size_ptr;
        let mut rim = Frame::new();
        api.open_column(&mut rim, &UiStr::keyed("shroudlinkLockRim", rim_key), 0.0);
        api.set_size(&rim, size, size);
        api.set_padding(&rim, [LOCK_RIM; 4]);
        api.background(&mut rim, LOCK_RIM_STYLE, 1.0);
        *size_ptr = size - 2.0 * LOCK_RIM;
        let r = api.item_slot.call(out, desc, compare, drag);
        *size_ptr = size;
        api.close_frame(&mut rim);
        r
    } else {
        api.item_slot.call(out, desc, compare, drag)
    };
    SLOT_TINT.store(0, std::sync::atomic::Ordering::Relaxed);
    if let Some(position) = position {
        if crate::automation::enabled() {
            let mark = mark.map(|m| if m == shroudlink::ui::MarkStyle::Rim { "rim" } else { "dim" });
            crate::automation::record_slot(id, position, item, *(out as *const [f32; 4]), mark);
        }
        if let Some(key) = crate::input::slot_key_pressed_over(*(out as *const [f32; 4]), mouse_pos()) {
            crate::slots::key_pressed(key, id, position, item);
        }
    }
    r
}

/// Tint for the slot the game is drawing right now (0: none).
static SLOT_TINT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Set while the chest screen draws the backpack panel: its slots are the player's own.
static BACKPACK_SLOTS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

unsafe extern "system" fn panel_header_hook(panel: *mut c_void, index: u8, extra: u64) {
    let api = API.get().unwrap();
    IN_BACKPACK_PANEL.store(index == 0, std::sync::atomic::Ordering::Relaxed);
    BACKPACK_SLOTS.store(index == 0, std::sync::atomic::Ordering::Relaxed);
    api.panel_header.call(panel, index, extra);
    IN_BACKPACK_PANEL.store(false, std::sync::atomic::Ordering::Relaxed);
    BACKPACK_SLOTS.store(false, std::sync::atomic::Ordering::Relaxed);
    if index == 0 {
        crate::slots::panel_done();
    }
    // still inside the panel's frame: whatever we open now becomes part of the panel layout
    crate::ui::after_panel_header(api, index);
}

/// Set while the game draws the backpack panel of the chest screen.
static IN_BACKPACK_PANEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Right before the backpack's slot grid, inside the panel's column: whatever we open here
/// sits below the title and pushes the grid down (the open item search).
unsafe extern "system" fn slot_grid_hook(panel: *mut c_void, inventory: u64, state: *mut c_void) {
    let api = API.get().unwrap();
    if IN_BACKPACK_PANEL.swap(false, std::sync::atomic::Ordering::Relaxed) {
        crate::ui::before_backpack_grid(api);
    }
    api.slot_grid.call(panel, inventory, state);
}

unsafe extern "system" fn slot_render_hook(render: *mut u8, desc: *mut u8, state: usize, drag: usize) {
    let api = API.get().unwrap();
    api.slot_render.call(render, desc, state, drag);
    let tint = SLOT_TINT.load(std::sync::atomic::Ordering::Relaxed);
    if tint != 0 {
        *(render.add(RENDER_TINT) as *mut u64) = tint;
        *render.add(RENDER_TINT_ON) = 1;
    }
}

unsafe extern "system" fn dispatch_hook() {
    update_mouse();
    crate::input::ensure();
    let api = API.get().unwrap();
    api.dispatch.call();
    crate::ui::draw(api);
}

/// The tooltip functions, if this game build has them (else item grids go without).
fn item_tips(find: &dyn Fn(&str) -> Result<usize, String>) -> Option<ItemTips> {
    let (state_init, init, draw) = match (find(SIG_TIP_STATE_INIT), find(SIG_TIP_INIT), find(SIG_TIP_DRAW)) {
        (Ok(a), Ok(b), Ok(c)) => (a, b, c),
        _ => {
            log!("item tooltips off: a tooltip function was not found");
            return None;
        }
    };
    unsafe {
        // lives as long as the addon copy: the slots and the tooltip read it every frame
        let state = Box::leak(vec![0u128; TIP_STATE_SIZE / 16 + 1].into_boxed_slice()).as_mut_ptr() as *mut u8;
        std::mem::transmute::<usize, TipStateInitFn>(state_init)(state);
        Some(ItemTips { state, init: std::mem::transmute(init), draw: std::mem::transmute(draw) })
    }
}

pub fn install(module: &Module) -> Result<(), String> {
    let find = |sig: &str| module.find(sig);
    let open_child = find(SIG_OPEN_CHILD)?;
    let qr = find(SIG_QR_POPUP)?;
    let builder = rip_global(open_child, 0x20).ok_or("UI builder global not found")?;
    let game_ui = rip_global(qr, 0x40).ok_or("game UI global not found")?;
    unsafe {
        let api = Api {
            dispatch: GenericDetour::new(std::mem::transmute::<usize, DispatchFn>(find(SIG_UI_DISPATCH)?), dispatch_hook)
                .map_err(|e| e.to_string())?,
            player_menu: GenericDetour::new(std::mem::transmute::<usize, PlayerMenuFn>(find(SIG_PLAYER_MENU)?), player_menu_hook)
                .map_err(|e| e.to_string())?,
            tab_list: GenericDetour::new(std::mem::transmute::<usize, TabListFn>(find(SIG_TAB_LIST)?), tab_list_hook)
                .map_err(|e| e.to_string())?,
            tab_widget: GenericDetour::new(std::mem::transmute::<usize, TabWidgetFn>(find(SIG_TAB_WIDGET)?), tab_widget_hook)
                .map_err(|e| e.to_string())?,
            open_row: std::mem::transmute::<usize, OpenRowFn>(find(SIG_OPEN_ROW)?),
            background: std::mem::transmute::<usize, BackgroundFn>(find(SIG_BACKGROUND)?),
            fill: {
                let calls = chestsort_common::scan::call_targets(find(SIG_BACKGROUND)?, 0x1000);
                match (calls.get(9), calls.get(16)) {
                    (Some(&a), Some(&b)) if a == b => std::mem::transmute::<usize, FillFn>(a),
                    _ => return Err("background: no rectangle fill at calls 10 and 17".into()),
                }
            },
            slot_desc_init: std::mem::transmute::<usize, SlotDescInitFn>(find(SIG_SLOT_DESC_INIT)?),
            item_slot: GenericDetour::new(std::mem::transmute::<usize, ItemSlotFn>(find(SIG_ITEM_SLOT)?), item_slot_hook)
                .map_err(|e| e.to_string())?,
            slot_render: GenericDetour::new(std::mem::transmute::<usize, SlotRenderFn>(find(SIG_SLOT_RENDER)?), slot_render_hook)
                .map_err(|e| e.to_string())?,
            panel_header: GenericDetour::new(std::mem::transmute::<usize, PanelHeaderFn>(find(SIG_PANEL_HEADER)?), panel_header_hook)
                .map_err(|e| e.to_string())?,
            slot_grid: GenericDetour::new(std::mem::transmute::<usize, SlotGridFn>(find(SIG_SLOT_GRID)?), slot_grid_hook)
                .map_err(|e| e.to_string())?,
            loca_text: std::mem::transmute::<usize, LocaTextFn>(find(SIG_LOCA_TEXT)?),
            item_info: std::mem::transmute::<usize, ItemInfoFn>(find(SIG_ITEM_INFO)?),
            begin_layer: std::mem::transmute::<usize, BeginLayerFn>(find(SIG_BEGIN_LAYER)?),
            end_layer: std::mem::transmute::<usize, EndLayerFn>(find(SIG_END_LAYER)?),
            open_root: std::mem::transmute::<usize, OpenRootFn>(find(SIG_OPEN_ROOT)?),
            open_child: std::mem::transmute::<usize, OpenChildFn>(open_child),
            open_column: std::mem::transmute::<usize, OpenColumnFn>(find(SIG_OPEN_COLUMN)?),
            close_frame: std::mem::transmute::<usize, CloseFrameFn>(find(SIG_CLOSE_FRAME)?),
            text_new: std::mem::transmute::<usize, TextNewFn>(find(SIG_TEXT_NEW)?),
            text_emit: std::mem::transmute::<usize, TextEmitFn>(find(SIG_TEXT_EMIT)?),
            game_ui,
            tips: item_tips(&find),
        };
        let api = API.get_or_init(|| api);
        api.dispatch.enable().map_err(|e| e.to_string())?;
        api.item_slot.enable().map_err(|e| e.to_string())?;
        api.slot_render.enable().map_err(|e| e.to_string())?;
        api.panel_header.enable().map_err(|e| e.to_string())?;
        api.slot_grid.enable().map_err(|e| e.to_string())?;
        api.tab_list.enable().map_err(|e| e.to_string())?;
        api.player_menu.enable().map_err(|e| e.to_string())?;
        api.tab_widget.enable().map_err(|e| e.to_string())?;
        log!("native UI ready: game ui +{:#x}, builder +{:#x}", module.rva(game_ui), module.rva(builder));
    }
    Ok(())
}

pub fn uninstall() {
    if let Some(api) = API.get() {
        unsafe {
            let _ = api.dispatch.disable();
            let _ = api.item_slot.disable();
            let _ = api.slot_render.disable();
            let _ = api.panel_header.disable();
            let _ = api.slot_grid.disable();
            let _ = api.tab_list.disable();
            let _ = api.player_menu.disable();
            let _ = api.tab_widget.disable();
        }
    }
}
