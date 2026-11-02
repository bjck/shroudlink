//! Stops the server before it loads a world on the wrong data files.
//!
//! A server that loads a world with x10 stacks on vanilla data cuts every stack to the
//! vanilla size and deletes the rest. A game update brings vanilla data back. So when the
//! data patches are uploaded, `UPDATE-SERVER.ps1` / `repatch.ps1` also write `MARKER`: the
//! SHA-256 of each patched file. If a world is there and a file no longer matches, the core
//! ends the process from `DllMain`, before the game has read any data.
//!
//! Without the marker the guard does nothing (servers without data patches). With EML's
//! proxy installed (`eml.rs`) EML patches the files on every start itself, so the file on
//! disk changes by design and the guard steps aside. Deleting the marker switches it off.

use std::path::Path;

pub const MARKER: &str = "shroudlink_data.txt";
/// The dedicated server's config; its `saveDirectory` is where the world lives.
const CONFIG: &str = "enshrouded_server.json";
const DEFAULT_SAVES: &str = "savegame";

#[derive(Debug, PartialEq)]
pub enum Verdict {
    /// no marker: this server runs without data patches, or the owner switched the guard off
    Off,
    /// a marker that lists no file
    EmptyMarker,
    /// EML's proxy patches on every start
    Eml,
    /// no world to protect (a parked or new world)
    NoWorld,
    /// the data files are the patched ones
    Patched,
    /// `file` is not the patched file (a game update, or a missing upload)
    Mismatch { file: String, expected: String, actual: String },
}

impl Verdict {
    pub fn stops(&self) -> bool {
        matches!(self, Verdict::Mismatch { .. })
    }
}

/// `<file name> <SHA-256 hex>` lines; `#` comments and other keys (`build 123`) are skipped.
fn expected_hashes(marker: &str) -> Vec<(&str, &str)> {
    marker
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let (file, hash) = (parts.next()?, parts.next()?);
            let is_hash = hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit());
            (!file.starts_with('#') && is_hash && parts.next().is_none()).then_some((file, hash))
        })
        .collect()
}

/// The decision, apart from the file system: `hash_of` gives a file's SHA-256 (`None` if it
/// cannot be read).
fn decide(marker: Option<&str>, eml: bool, world: bool, hash_of: impl Fn(&str) -> Option<String>) -> Verdict {
    let Some(marker) = marker else { return Verdict::Off };
    let expected = expected_hashes(marker);
    if expected.is_empty() {
        return Verdict::EmptyMarker;
    }
    if eml {
        return Verdict::Eml;
    }
    if !world {
        return Verdict::NoWorld;
    }
    for (file, hash) in expected {
        let actual = hash_of(file).unwrap_or_else(|| "missing".into());
        if !actual.eq_ignore_ascii_case(hash) {
            return Verdict::Mismatch { file: file.into(), expected: hash.to_ascii_uppercase(), actual };
        }
    }
    Verdict::Patched
}

/// The save folder from the server config (relative to the server folder), as the
/// dedicated server reads it.
fn save_dir(dir: &Path) -> std::path::PathBuf {
    let configured = std::fs::read_to_string(dir.join(CONFIG))
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|json| json.get("saveDirectory")?.as_str().map(str::to_owned));
    dir.join(configured.as_deref().unwrap_or(DEFAULT_SAVES))
}

/// A world exists when the save folder holds any file (world saves sit at its top level,
/// some hosts nest one folder deeper).
fn world_exists(saves: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(saves) else { return false };
    entries.flatten().any(|entry| {
        let path = entry.path();
        path.is_file() || std::fs::read_dir(&path).is_ok_and(|mut inner| inner.any(|e| e.is_ok_and(|e| e.path().is_file())))
    })
}

pub fn check(dir: &Path, eml: bool) -> Verdict {
    let marker = std::fs::read_to_string(dir.join(MARKER)).ok();
    decide(marker.as_deref(), eml, world_exists(&save_dir(dir)), |file| {
        // only plain file names: the marker cannot point outside the server folder
        if file.contains(['/', '\\', ':']) {
            return None;
        }
        chestsort_common::sha256::file_hex(&dir.join(file)).ok()
    })
}

/// The log line for a verdict.
pub fn describe(verdict: &Verdict) -> String {
    match verdict {
        Verdict::Off => format!("data guard: off (no {MARKER})"),
        Verdict::EmptyMarker => format!("data guard: off ({MARKER} lists no file)"),
        Verdict::Eml => "data guard: EML patches the data files on every start, not checked".into(),
        Verdict::NoWorld => "data guard: no world in the save folder, not checked".into(),
        Verdict::Patched => "data guard: the data files are the patched ones".into(),
        Verdict::Mismatch { file, expected, actual } => format!(
            "data guard: STOPPED THE SERVER. {file} is not the patched file (expected {expected}, found {actual}). \
             A game update probably brought back the vanilla data: loading the world now would delete every \
             item above the vanilla stack size. Upload the patched files for this build with UPDATE-SERVER.bat. \
             To run without the data patches on purpose, delete {MARKER} (items above the vanilla stack size \
             are then lost)."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HASH: &str = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";

    fn marker() -> String {
        format!("# shroudlink data guard\nbuild 1234\nenshrouded_server.kfc {HASH}\n")
    }

    #[test]
    fn reads_only_hash_lines() {
        let text = format!("{}# x.kfc {HASH}\nbad.kfc 1234\ntoo many {HASH} words\n", marker());
        assert_eq!(expected_hashes(&text), [("enshrouded_server.kfc", HASH)]);
    }

    #[test]
    fn stops_only_with_a_world_on_other_data() {
        let same = |_: &str| Some(HASH.to_ascii_uppercase());
        let other = |_: &str| Some("00".repeat(32));
        let m = marker();
        assert_eq!(decide(None, false, true, other), Verdict::Off);
        assert_eq!(decide(Some("build 1\n"), false, true, other), Verdict::EmptyMarker);
        assert_eq!(decide(Some(&m), true, true, other), Verdict::Eml);
        assert_eq!(decide(Some(&m), false, false, other), Verdict::NoWorld);
        assert_eq!(decide(Some(&m), false, true, same), Verdict::Patched);
        let verdict = decide(Some(&m), false, true, other);
        assert!(verdict.stops());
        assert!(decide(Some(&m), false, true, |_| None).stops(), "a missing data file stops too");
    }

    #[test]
    fn finds_the_world_in_the_configured_folder() {
        let dir = std::env::temp_dir().join(format!("shroudlink_guard_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("savegame")).unwrap();
        assert!(!world_exists(&save_dir(&dir)), "an empty save folder is no world");
        std::fs::write(dir.join("savegame/3ad85aea"), b"world").unwrap();
        assert!(world_exists(&save_dir(&dir)));
        std::fs::write(dir.join(CONFIG), r#"{ "saveDirectory": "./saves" }"#).unwrap();
        assert!(!world_exists(&save_dir(&dir)), "the configured folder counts, not ./savegame");
        std::fs::create_dir_all(dir.join("saves/world")).unwrap();
        std::fs::write(dir.join("saves/world/3ad85aea"), b"world").unwrap();
        assert!(world_exists(&save_dir(&dir)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
