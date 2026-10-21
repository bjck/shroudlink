//! Buffs for mods, through the game's own impact VM.
//!
//! Impact programs (item, buff and ability scripts) call natives registered by name in
//! `FUN_14022e680`; each native is `fn(out, args, vm) -> out`. `args` points at a list of
//! u32 variable indices (0: argument not given), `vm + 0x00` at the variable layout
//! (`keen::ImpactVariable`, 0x18 bytes, `offsetInBytes` at +0xe), `vm + 0x10` at the
//! variable data and `vm + 0x30` at the context of the program that is running (the
//! entity it runs for, component storages). `applyBuff(buffType, target, …)`
//! (`FUN_1401775c0`) hashes the buff's Guid, looks it up in the buff registry and applies
//! it to `target`; `out[8]` is 1 when it was applied.
//!
//! That context exists only while a program runs, so requests are queued and applied from
//! inside `getSelf` (`FUN_140182290`), a native nearly every program calls: with the
//! running program's context and our own arguments, the same way a program would.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chestsort_common::log;
use retour::GenericDetour;

use crate::hooks;

type NativeFn = unsafe extern "system" fn(out: *mut u8, args: *const *const u32, vm: *mut u8) -> *mut u8;

struct Hooks {
    get_self: GenericDetour<NativeFn>,
    apply_buff: NativeFn,
}
unsafe impl Send for Hooks {}
unsafe impl Sync for Hooks {}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
static PENDING: AtomicBool = AtomicBool::new(false);
static QUEUE: Mutex<Vec<Request>> = Mutex::new(Vec::new());
/// getSelf calls seen, for the self-test (does the hook run at all).
static CALLS: AtomicU64 = AtomicU64::new(0);

/// `keen::ImpactVariable`
const VARIABLE_SIZE: usize = 0x18;
const VARIABLE_OFFSET_AT: usize = 0xe;
/// vm fields the natives read: layout, data, running context
const VM_LAYOUT: usize = 0x00;
const VM_DATA: usize = 0x10;
const VM_BYTES: usize = 0x38;
/// `out[8]`: the native succeeded
const OUT_OK: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub target: u32,
    /// `keen::Guid` of the `BuffType`, in memory order
    pub buff: [u8; 16],
}

pub fn install() -> Result<(), String> {
    unsafe {
        let get_self = GenericDetour::<NativeFn>::new(std::mem::transmute::<usize, NativeFn>(hooks::address("impact_get_self")?), get_self_hook).map_err(|e| e.to_string())?;
        let apply_buff = std::mem::transmute::<usize, NativeFn>(hooks::address("impact_apply_buff")?);
        let hooks = HOOKS.get_or_init(|| Hooks { get_self, apply_buff });
        hooks.get_self.enable().map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Queues a buff for `target`; it is applied the next time any impact program runs.
pub fn apply(request: Request) -> bool {
    if HOOKS.get().is_none() {
        return false;
    }
    QUEUE.lock().unwrap().push(request);
    PENDING.store(true, Ordering::Release);
    true
}

pub fn calls() -> u64 {
    CALLS.load(Ordering::Relaxed)
}

unsafe extern "system" fn get_self_hook(out: *mut u8, args: *const *const u32, vm: *mut u8) -> *mut u8 {
    let hooks = HOOKS.get().unwrap();
    let result = hooks.get_self.call(out, args, vm);
    CALLS.fetch_add(1, Ordering::Relaxed);
    // a plain load first: this runs on every program step, on several threads
    if PENDING.load(Ordering::Relaxed) && PENDING.swap(false, Ordering::AcqRel) {
        let requests = std::mem::take(&mut *QUEUE.lock().unwrap());
        for request in requests {
            if !call_apply_buff(hooks.apply_buff, vm, &request) {
                log!("buffs: buff {} refused for entity {}", shroudlink_sdk::guid_hash(&request.buff), request.target);
            }
        }
    }
    result
}

/// Runs `applyBuff(buff, target)` with the running program's context.
unsafe fn call_apply_buff(apply_buff: NativeFn, vm: *mut u8, request: &Request) -> bool {
    // variables 1 and 2 (0 means "not given"): the buff Guid at 0, the target at 16
    let mut layout = [0u8; VARIABLE_SIZE * 3];
    layout[2 * VARIABLE_SIZE + VARIABLE_OFFSET_AT..][..2].copy_from_slice(&16u16.to_le_bytes());
    let mut data = [0u8; 32];
    data[..16].copy_from_slice(&request.buff);
    data[16..20].copy_from_slice(&request.target.to_le_bytes());
    let indices: [u32; 5] = [1, 2, 0, 0, 0];
    let args = indices.as_ptr();
    let mut own_vm = [0u8; VM_BYTES];
    std::ptr::copy_nonoverlapping(vm, own_vm.as_mut_ptr(), VM_BYTES);
    own_vm[VM_LAYOUT..][..8].copy_from_slice(&(layout.as_ptr() as usize).to_le_bytes());
    own_vm[VM_DATA..][..8].copy_from_slice(&(data.as_ptr() as usize).to_le_bytes());
    let mut out = [0u8; 16];
    apply_buff(out.as_mut_ptr(), &args, own_vm.as_mut_ptr());
    out[OUT_OK] == 1
}
