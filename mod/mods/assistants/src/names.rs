//! Readable names from the game's debug names (the dedicated server has no localisation;
//! item grids show the real names on the client, text lines use these).

use std::collections::HashMap;
use std::sync::OnceLock;

use shroudlink_categories::ITEM_NAMES;

fn item_names() -> &'static HashMap<u32, String> {
    static NAMES: OnceLock<HashMap<u32, String>> = OnceLock::new();
    NAMES.get_or_init(|| {
        ITEM_NAMES
            .lines()
            .filter_map(|line| {
                let (id, name) = line.split_once('\t')?;
                Some((id.parse().ok()?, name.to_string()))
            })
            .collect()
    })
}

/// Words of a debug name that say what kind of thing it is, not what it is.
const NOISE: &[&str] = &["food", "material", "consumable", "prop", "production", "factory", "processed", "dish", "raw", "craftingstation", "npc"];

/// "Food_T5_Yakstew" -> "Yakstew", "Material_T3_Factory_BronzeBar_SM" -> "Bronze Bar SM".
pub fn pretty(debug: &str) -> String {
    let words: Vec<String> = debug
        .split('_')
        .filter(|w| !w.is_empty() && !NOISE.contains(&w.to_lowercase().as_str()))
        .filter(|w| !(w.len() <= 3 && w.starts_with(['T', 't']) && w[1..].chars().all(|c| c.is_ascii_digit()) && w.len() > 1))
        .filter(|w| !w.chars().all(|c| c.is_ascii_digit()))
        .map(split_camel)
        .collect();
    let text = words.join(" ");
    if text.is_empty() {
        debug.to_string()
    } else {
        text
    }
}

fn split_camel(word: &str) -> String {
    let mut out = String::new();
    let mut prev_lower = false;
    for c in word.chars() {
        if c.is_uppercase() && prev_lower {
            out.push(' ');
        }
        prev_lower = c.is_lowercase();
        out.push(c);
    }
    let mut chars = out.chars();
    chars.next().map(|f| f.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

pub fn item(id: u32) -> String {
    item_names().get(&id).map(|n| pretty(n)).unwrap_or_else(|| format!("item {id}"))
}

/// "NPC_Workshop_Farmer_Assistant_01" -> "Farmer's assistant", "NPC_Workshop_Hunter01" ->
/// "Hunter", "NPC_Workshop_cryptKeeper_Assistant_01" -> "Crypt keeper's assistant".
pub fn npc(debug: &str) -> String {
    let rest = debug.strip_prefix("NPC_Workshop_").unwrap_or(debug);
    let first = rest.split('_').next().unwrap_or(rest).trim_end_matches(|c: char| c.is_ascii_digit());
    let mut name = split_camel(first);
    if let Some((head, tail)) = name.split_once(' ') {
        name = format!("{head} {}", tail.to_lowercase());
    }
    if rest.contains("_Assistant") {
        format!("{name}'s assistant")
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_names_read_well() {
        assert_eq!(pretty("Food_T5_Yakstew"), "Yakstew");
        assert_eq!(pretty("Material_T3_Factory_BronzeBar_SM"), "Bronze Bar SM");
        assert_eq!(pretty("Food_T1_processed_grain_CorncobSkewer"), "Grain Corncob Skewer");
        assert_eq!(pretty("Craftingstation_Farmer_Kitchen"), "Farmer Kitchen");
        assert_eq!(pretty("Prop_Production_T2_Smelter"), "Smelter");
        assert_eq!(npc("NPC_Workshop_Farmer_Assistant_01"), "Farmer's assistant");
        assert_eq!(npc("NPC_Workshop_Hunter01"), "Hunter");
        assert_eq!(npc("NPC_Workshop_cryptKeeper_Assistant_01"), "Crypt keeper's assistant");
        assert_eq!(npc("NPC_Workshop_ancientResearcher_LoraineCrim"), "Ancient researcher");
    }
}
