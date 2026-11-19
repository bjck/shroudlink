//! Index of the custom names players give objects (chest "Rename" button).
//!
//! The server keeps every custom name in records of the form
//! `{ u64 key; char* text = &key + 0x28; u32 len; u32 pad; u64 capacity = 0x100; ...; char text[256] }`
//! (save-layer "CustomStrings"). We have no stable path to the owning
//! table, so a background thread scans the heap for that self-describing layout.
//! The same key can exist in several copies (live table and save snapshot);
//! the copy whose text changed most recently wins.
//!
//! The scan must not get in the game's way: on a 4-thread CPU with the page file
//! on a hard drive, reading all private memory every 15 s took 2.2-2.5 s of a
//! core and paged the game's memory back in, and the game stuttered at every scan.
//! So it reads only pages in the working set (a page outside it has not been
//! written since it was paged out, so its records are kept from the scan before),
//! and the background thread runs in background mode and rests between short
//! stretches of work.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use chestsort_common::log;
use windows_sys::Win32::System::Diagnostics::Debug::ReadProcessMemory;
use windows_sys::Win32::System::Memory::{VirtualQuery, MEMORY_BASIC_INFORMATION, MEM_COMMIT, MEM_PRIVATE, PAGE_READWRITE};
use windows_sys::Win32::System::ProcessStatus::{K32QueryWorkingSetEx, PSAPI_WORKING_SET_EX_INFORMATION};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetCurrentThread, SetThreadPriority, THREAD_MODE_BACKGROUND_BEGIN};

const TEXT_OFFSET: usize = 0x28;
const CAPACITY: u64 = 0x100;
const SCAN_INTERVAL: Duration = Duration::from_secs(15);
const MIN_RESCAN_GAP: Duration = Duration::from_secs(3);
const CHUNK: usize = 1 << 20;
const PAGE: usize = 0x1000;
/// bytes of one record, header and text
const RECORD: usize = TEXT_OFFSET + 0x100;
/// The background scan works at most `WORK` at a time, then sleeps `REST`.
const WORK: Duration = Duration::from_millis(4);
const REST: Duration = Duration::from_millis(4);

struct Record {
    key: u64,
    text: String,
    /// scan number in which this copy got its current text
    changed: u64,
}

#[derive(Default)]
struct Index {
    by_addr: HashMap<usize, Record>,
    scans: u64,
    last_scan: Option<Instant>,
}

static INDEX: Mutex<Option<Index>> = Mutex::new(None);

pub fn start() {
    std::thread::spawn(|| {
        // lower CPU, I/O and memory priority: the game's threads come first
        unsafe { SetThreadPriority(GetCurrentThread(), THREAD_MODE_BACKGROUND_BEGIN) };
        loop {
            rescan(true);
            std::thread::sleep(SCAN_INTERVAL);
        }
    });
}

/// Current name for `key`, if any object carries it.
pub fn lookup(key: u64) -> Option<String> {
    let guard = INDEX.lock().unwrap();
    let index = guard.as_ref()?;
    index
        .by_addr
        .values()
        .filter(|r| r.key == key)
        .max_by_key(|r| r.changed)
        .map(|r| r.text.clone())
}

/// Like [`lookup`], but rescans immediately (rate limited) when the key is unknown,
/// e.g. right after a chest was renamed.
pub fn lookup_fresh(key: u64) -> Option<String> {
    if let Some(name) = lookup(key) {
        return Some(name);
    }
    let recent = INDEX.lock().unwrap().as_ref().and_then(|i| i.last_scan).is_some_and(|t| t.elapsed() < MIN_RESCAN_GAP);
    if recent {
        return None;
    }
    // the caller (the game thread) waits for this scan: no rests
    rescan(false);
    lookup(key)
}

/// `background`: the scan thread's own scan, which rests between chunks.
fn rescan(background: bool) {
    let started = Instant::now();
    let mut stats = ScanStats::default();
    let found = scan_heap(&mut stats, background);
    let mut guard = INDEX.lock().unwrap();
    let index = guard.get_or_insert_with(Index::default);
    index.scans += 1;
    let scan = index.scans;
    let prev = std::mem::take(&mut index.by_addr);
    let had = prev.len();
    let mut next = HashMap::with_capacity(found.len());
    for (addr, key, text) in found {
        let changed = match prev.get(&addr) {
            Some(old) if old.key == key && old.text == text => old.changed,
            _ => scan,
        };
        next.insert(addr, Record { key, text, changed });
    }
    let kept = keep_skipped(&mut next, prev, &stats.skipped);
    let count = next.len();
    index.by_addr = next;
    index.last_scan = Some(Instant::now());
    if count != had || scan == 1 {
        log!(
            "name index: {count} name record(s), scan took {} ms (read {} MiB, skipped {} MiB not in memory, kept {kept} record(s) from the scan before)",
            started.elapsed().as_millis(),
            stats.bytes >> 20,
            stats.skipped.iter().map(|(a, b)| b - a).sum::<usize>() >> 20
        );
    }
}

/// What a scan read: the bytes, and the ranges it skipped (pages not in the
/// working set) in address order.
#[derive(Default)]
struct ScanStats {
    bytes: usize,
    skipped: Vec<(usize, usize)>,
}

/// Puts the records of the scan before that lie (even in part) on a skipped page
/// into `next` as they were. Returns how many.
fn keep_skipped(next: &mut HashMap<usize, Record>, prev: HashMap<usize, Record>, skipped: &[(usize, usize)]) -> usize {
    let mut kept = 0;
    for (addr, record) in prev {
        if !next.contains_key(&addr) && overlaps(skipped, addr, addr + RECORD) {
            next.insert(addr, record);
            kept += 1;
        }
    }
    kept
}

/// Whether `[start, end)` meets one of `ranges` (sorted, not overlapping).
fn overlaps(ranges: &[(usize, usize)], start: usize, end: usize) -> bool {
    let i = ranges.partition_point(|&(_, e)| e <= start);
    i < ranges.len() && ranges[i].0 < end
}

/// The runs of resident pages in `[start, start + len)` as `(address, length)`,
/// clipped to the range. `resident[i]`: page `i` from `start` (page aligned).
fn resident_runs(resident: &[bool], start: usize, len: usize) -> Vec<(usize, usize)> {
    let end = start + len;
    let mut runs = Vec::new();
    let mut i = 0;
    while i < resident.len() {
        if !resident[i] {
            i += 1;
            continue;
        }
        let first = i;
        while i < resident.len() && resident[i] {
            i += 1;
        }
        let a = start + first * PAGE;
        let b = (start + i * PAGE).min(end);
        if a < b {
            runs.push((a, b - a));
        }
    }
    runs
}

/// Adds the pages of `[start, start + len)` that are not resident to `skipped`,
/// merging with its last range.
fn add_skipped(skipped: &mut Vec<(usize, usize)>, resident: &[bool], start: usize, len: usize) {
    let end = start + len;
    for (i, &r) in resident.iter().enumerate() {
        let a = start + i * PAGE;
        if a >= end {
            break;
        }
        if r {
            continue;
        }
        let b = (a + PAGE).min(end);
        match skipped.last_mut() {
            Some(last) if last.1 == a => last.1 = b,
            _ => skipped.push((a, b)),
        }
    }
}

/// Which of the `pages` pages from `start` are in this process's working set
/// (`PSAPI_WORKING_SET_EX_BLOCK.Valid`, bit 0). All of them when Windows cannot
/// say, so the scan then reads everything as before.
unsafe fn resident_pages(start: usize, pages: usize, ws: &mut Vec<PSAPI_WORKING_SET_EX_INFORMATION>) -> Vec<bool> {
    ws.clear();
    ws.resize_with(pages, || std::mem::zeroed());
    for (i, e) in ws.iter_mut().enumerate() {
        e.VirtualAddress = (start + i * PAGE) as *mut _;
    }
    let bytes = (pages * std::mem::size_of::<PSAPI_WORKING_SET_EX_INFORMATION>()) as u32;
    if K32QueryWorkingSetEx(GetCurrentProcess(), ws.as_mut_ptr() as *mut _, bytes) == 0 {
        return vec![true; pages];
    }
    ws.iter().map(|e| e.VirtualAttributes.Flags & 1 != 0).collect()
}

/// All `(address, key, text)` name records on resident pages of committed private
/// read/write memory; `stats` gets the bytes read and the ranges skipped.
fn scan_heap(stats: &mut ScanStats, background: bool) -> Vec<(usize, u64, String)> {
    let mut found = Vec::new();
    let mut buf = vec![0u8; CHUNK + 0x200];
    let mut ws = Vec::new();
    let mut working = Instant::now();
    let mut addr = 0usize;
    unsafe {
        let process = GetCurrentProcess();
        let mut info: MEMORY_BASIC_INFORMATION = std::mem::zeroed();
        while VirtualQuery(addr as *const _, &mut info, std::mem::size_of::<MEMORY_BASIC_INFORMATION>()) != 0 {
            let base = info.BaseAddress as usize;
            let size = info.RegionSize;
            if info.State == MEM_COMMIT && info.Type == MEM_PRIVATE && info.Protect == PAGE_READWRITE {
                let mut offset = 0;
                while offset < size {
                    // overlap chunks so records crossing a boundary are seen whole
                    let len = (size - offset).min(CHUNK + 0x200);
                    let start = base + offset;
                    let resident = resident_pages(start, len.div_ceil(PAGE), &mut ws);
                    // the overlap is counted with the next chunk
                    add_skipped(&mut stats.skipped, &resident, start, (size - offset).min(CHUNK));
                    for (run, run_len) in resident_runs(&resident, start, len) {
                        let mut read = 0usize;
                        let ok = ReadProcessMemory(process, run as *const _, buf.as_mut_ptr() as *mut _, run_len, &mut read);
                        if ok != 0 {
                            stats.bytes += read;
                            scan_chunk(&buf[..read], run, &mut found);
                        }
                    }
                    if background && working.elapsed() >= WORK {
                        std::thread::sleep(REST);
                        working = Instant::now();
                    }
                    offset += CHUNK;
                }
            }
            addr = base + size;
        }
    }
    found
}

fn scan_chunk(data: &[u8], base: usize, found: &mut Vec<(usize, u64, String)>) {
    let mut i = 0;
    while i + TEXT_OFFSET + 0x100 <= data.len() {
        let addr = base + i;
        let ptr = u64::from_le_bytes(data[i + 8..i + 16].try_into().unwrap());
        if ptr == (addr + TEXT_OFFSET) as u64
            && u64::from_le_bytes(data[i + 0x18..i + 0x20].try_into().unwrap()) == CAPACITY
        {
            let key = u64::from_le_bytes(data[i..i + 8].try_into().unwrap());
            let len = u32::from_le_bytes(data[i + 0x10..i + 0x14].try_into().unwrap()) as usize;
            let text = &data[i + TEXT_OFFSET..i + TEXT_OFFSET + 0x100];
            if key != 0 && (1..0x100).contains(&len) && text[len] == 0 {
                if let Ok(s) = std::str::from_utf8(&text[..len]) {
                    found.push((addr, key, s.to_owned()));
                }
            }
        }
        i += 8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(key: u64, text: &str, changed: u64) -> Record {
        Record { key, text: text.into(), changed }
    }

    #[test]
    fn ranges_and_runs() {
        let r = [(0x1000, 0x3000), (0x8000, 0x9000)];
        assert!(overlaps(&r, 0x2f00, 0x3100) && overlaps(&r, 0x0, 0x1001) && overlaps(&r, 0x8fff, 0x9100));
        assert!(!overlaps(&r, 0x3000, 0x8000) && !overlaps(&r, 0x0, 0x1000) && !overlaps(&r, 0x9000, 0xa000) && !overlaps(&[], 0, 9));
        // pages 0 and 1 resident, 2 not, 3 resident; the range ends inside page 3
        let resident = [true, true, false, true];
        assert_eq!(resident_runs(&resident, 0x10000, 3 * PAGE + 0x200), vec![(0x10000, 2 * PAGE), (0x13000, 0x200)]);
        assert_eq!(resident_runs(&[false, false], 0x10000, 2 * PAGE), vec![]);
        let mut skipped = vec![(0xe000, 0x10000)];
        add_skipped(&mut skipped, &[false, true, false, false], 0x10000, 4 * PAGE);
        assert_eq!(skipped, vec![(0xe000, 0x11000), (0x12000, 0x14000)]);
        // only the chunk's own part counts: the overlap page is not added
        let mut own = Vec::new();
        add_skipped(&mut own, &[true, false], 0x20000, PAGE);
        assert!(own.is_empty());
    }

    #[test]
    fn records_on_skipped_pages_are_kept() {
        let mut prev = HashMap::new();
        prev.insert(0x10100, record(7, "Ores", 1)); // on a skipped page: kept
        prev.insert(0x11f80, record(8, "Food", 1)); // starts on a read page, ends on a skipped one: kept
        prev.insert(0x30000, record(9, "Gone", 1)); // on a read page and not found again: dropped
        prev.insert(0x40000, record(10, "Again", 1)); // found again: the new scan's copy wins
        let mut next = HashMap::new();
        next.insert(0x40000, record(10, "Again", 5));
        let skipped = [(0x10000, 0x11000), (0x12000, 0x13000)];
        assert_eq!(keep_skipped(&mut next, prev, &skipped), 2);
        assert_eq!(next.len(), 3);
        assert_eq!(next[&0x10100].text, "Ores");
        assert_eq!(next[&0x11f80].text, "Food");
        assert_eq!(next[&0x40000].changed, 5);
        assert!(!next.contains_key(&0x30000));
    }

    #[test]
    fn touched_pages_are_resident_untouched_ones_not() {
        use windows_sys::Win32::System::Memory::{VirtualAlloc, VirtualFree, MEM_RELEASE, MEM_RESERVE};
        const SIZE: usize = 16 << 20;
        unsafe {
            let touched = VirtualAlloc(std::ptr::null(), SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) as *mut u8;
            let untouched = VirtualAlloc(std::ptr::null(), SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) as *mut u8;
            assert!(!touched.is_null() && !untouched.is_null());
            for i in (0..SIZE).step_by(PAGE) {
                touched.add(i).write_volatile(1);
            }
            let mut ws = Vec::new();
            let t = resident_pages(touched as usize, SIZE / PAGE, &mut ws);
            let u = resident_pages(untouched as usize, SIZE / PAGE, &mut ws);
            let (t_in, u_in) = (t.iter().filter(|&&r| r).count(), u.iter().filter(|&&r| r).count());
            assert_eq!(t_in, t.len(), "every touched page is resident");
            assert!(u_in < u.len() / 2, "most untouched pages are not resident ({u_in} of {})", u.len());
            let mut skipped = Vec::new();
            add_skipped(&mut skipped, &u, untouched as usize, SIZE);
            assert!(skipped.iter().map(|(a, b)| b - a).sum::<usize>() > SIZE / 2);
            VirtualFree(touched as *mut _, 0, MEM_RELEASE);
            VirtualFree(untouched as *mut _, 0, MEM_RELEASE);
        }
    }

    #[test]
    fn a_scan_finds_a_record_in_this_process() {
        // a record laid out as the game's, page aligned in heap memory that was written (resident)
        let mut mem = vec![0u8; 3 * PAGE].into_boxed_slice();
        let at = (mem.as_ptr() as usize + PAGE) & !(PAGE - 1);
        let off = at - mem.as_ptr() as usize;
        let key: u64 = 0x5eed_0000_0000_0101;
        mem[off..off + 8].copy_from_slice(&key.to_le_bytes());
        mem[off + 8..off + 0x10].copy_from_slice(&((at + TEXT_OFFSET) as u64).to_le_bytes());
        mem[off + 0x10..off + 0x14].copy_from_slice(&4u32.to_le_bytes());
        mem[off + 0x18..off + 0x20].copy_from_slice(&CAPACITY.to_le_bytes());
        mem[off + TEXT_OFFSET..off + TEXT_OFFSET + 4].copy_from_slice(b"Gems");
        assert_eq!(unsafe { resident_pages(at, 1, &mut Vec::new()) }, vec![true]);
        let mut stats = ScanStats::default();
        let found = scan_heap(&mut stats, true);
        assert!(found.iter().any(|(a, k, t)| *a == at && *k == key && t == "Gems"));
        assert!(stats.bytes > 0);
        std::hint::black_box(&mem);
    }
}
