//! EML (github.com/Brabb3l/kfc-parser), when the server owner installed it: shroudlink
//! does not ship it. Its proxy DLL (`dbghelp.dll`, or `dinput8.dll`) patches the data files
//! with the EML mods in `mods/` on every start of the server, so data patches follow game
//! updates by themselves. The core only notices it: it lists the EML mods in the log, and
//! the data guard (`data_guard.rs`) leaves the patching to it.

use std::path::Path;

/// File names EML's proxy is installed under (its `dbghelp-proxy` and `dinput8-proxy`).
const PROXY_NAMES: &[&str] = &["dbghelp.dll", "dinput8.dll"];
/// Both proxies carry their environment variable names; Microsoft's DLLs of the same
/// names do not.
const PROXY_MARK: &[u8] = b"EML_LOG_FILE";

pub struct Eml {
    /// file name of EML's proxy DLL
    pub proxy: String,
    /// one line per entry of `mods/`
    pub mods: Vec<String>,
}

/// EML in `dir`, if its proxy is installed there. `own` is the core's own file name, which
/// may be `dinput8.dll` too.
pub fn detect(dir: &Path, own: Option<&str>) -> Option<Eml> {
    let proxy = PROXY_NAMES
        .iter()
        .filter(|name| !own.is_some_and(|own| own.eq_ignore_ascii_case(name)))
        .find(|name| std::fs::read(dir.join(name)).is_ok_and(|bytes| contains(&bytes, PROXY_MARK)))?;
    Some(Eml { proxy: proxy.to_string(), mods: list_mods(&dir.join("mods")) })
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// EML loads every folder or zip in `mods/` that has a `mod.json`.
fn list_mods(dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut mods: Vec<String> = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                let manifest = std::fs::read_to_string(path.join("mod.json")).ok()?;
                Some(describe(&name, &manifest))
            } else if name.to_ascii_lowercase().ends_with(".zip") {
                Some(name)
            } else {
                None
            }
        })
        .collect();
    mods.sort();
    mods
}

/// `folder: id version [capabilities]`, from EML's `ModManifest`.
fn describe(folder: &str, manifest: &str) -> String {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(manifest) else {
        return format!("{folder}: mod.json unreadable");
    };
    let text = |key: &str| json.get(key).and_then(|v| v.as_str()).unwrap_or("?").to_owned();
    let capabilities: Vec<&str> = json
        .get("capabilities")
        .and_then(|v| v.as_array())
        .map(|a| a.iter().filter_map(|c| c.as_str()).collect())
        .unwrap_or_default();
    format!("{folder}: {} {} [{}]", text("id"), text("version"), capabilities.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_a_manifest() {
        let manifest = r#"{ "id": "chestsort_bigger_stacks", "name": "Bigger Stacks", "version": "0.1.0", "capabilities": ["patch"] }"#;
        assert_eq!(describe("stacks", manifest), "stacks: chestsort_bigger_stacks 0.1.0 [patch]");
        assert_eq!(describe("broken", "{"), "broken: mod.json unreadable");
    }

    #[test]
    fn finds_the_proxy_by_its_marker() {
        let dir = std::env::temp_dir().join(format!("shroudlink_eml_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("mods/stacks")).unwrap();
        std::fs::write(dir.join("dbghelp.dll"), b"MZ...Microsoft").unwrap();
        assert!(detect(&dir, None).is_none());
        std::fs::write(dir.join("dinput8.dll"), b"MZ...EML_LOG_FILE_PATH...").unwrap();
        // the core itself as dinput8.dll is not EML
        assert!(detect(&dir, Some("DINPUT8.dll")).is_none());
        std::fs::write(dir.join("mods/stacks/mod.json"), r#"{"id":"s","version":"1.0.0","capabilities":["patch"]}"#).unwrap();
        std::fs::write(dir.join("mods/other.zip"), b"").unwrap();
        std::fs::write(dir.join("mods/readme.txt"), b"").unwrap();
        let eml = detect(&dir, Some("steam_api64.dll")).unwrap();
        assert_eq!(eml.proxy, "dinput8.dll");
        assert_eq!(eml.mods, ["other.zip", "stacks: s 1.0.0 [patch]"]);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
