//! Pure planning: what a station needs for the next runs of a recipe, given what it holds
//! and what the base's storage holds. No game access, so it is unit-tested on its own.

use std::collections::HashMap;

use crate::recipes::{Input, Recipe, Table};

/// Item counts by item id.
pub type Counts = HashMap<u32, u32>;

/// How many of an option of a category input one run takes.
fn scaled(count: u32, scaling: f32) -> u32 {
    ((count as f32 * scaling).ceil() as u32).max(1)
}

/// Runs still needed to make `left` more of `product`.
pub fn runs_for(recipe: &Recipe, product: u32, left: u32) -> u32 {
    let per_run = recipe.makes(product).max(1);
    left.div_ceil(per_run)
}

/// The item and count one run takes for an input; for a category the option the base has
/// most runs' worth of (station and storage together), the first option if it has none,
/// with the category and the option's index (`RecipeInputCategorySelection`).
fn choose(table: &Table, input: &Input, have: impl Fn(u32) -> u32) -> Option<(u32, u32, Option<(u32, u8)>)> {
    match *input {
        Input::Item { item, count } => Some((item, count, None)),
        Input::Category { id, count } => {
            let options = table.categories.get(&id)?;
            let (index, (item, per_run)) = options
                .iter()
                .map(|&(item, scaling)| (item, scaled(count, scaling)))
                .enumerate()
                .max_by_key(|&(index, (item, per_run))| (have(item) / per_run, std::cmp::Reverse(per_run), std::cmp::Reverse(index)))?;
            Some((item, per_run, Some((id, index as u8))))
        }
    }
}

/// What to move from storage into a station so it can do `runs` more runs.
#[derive(Debug, Default, PartialEq)]
pub struct Feed {
    /// runs the station will have material for after the moves (at most the runs asked)
    pub runs: u32,
    /// (item, count) to move from storage into the station
    pub moves: Vec<(u32, u32)>,
    /// (item, count) missing for even one more run
    pub missing: Vec<(u32, u32)>,
    /// the option chosen per category input: (category id, option index)
    pub selections: Vec<(u32, u8)>,
}

/// Plans the material for up to `runs` runs. The station may already hold some of it; the
/// plan tops it up to as many runs as storage allows, so a partial batch still starts.
pub fn feed(table: &Table, recipe: &Recipe, runs: u32, station: &Counts, storage: &Counts) -> Feed {
    let get = |c: &Counts, item: u32| c.get(&item).copied().unwrap_or(0);
    let mut per_run: Vec<(u32, u32)> = Vec::new();
    let mut selections = Vec::new();
    for input in &recipe.inputs {
        let Some((item, count, selection)) = choose(table, input, |i| get(station, i) + get(storage, i)) else { continue };
        selections.extend(selection);
        match per_run.iter_mut().find(|(i, _)| *i == item) {
            Some((_, c)) => *c += count,
            None => per_run.push((item, count)),
        }
    }
    let possible = per_run.iter().map(|&(item, count)| (get(station, item) + get(storage, item)) / count).min().unwrap_or(runs).min(runs);
    if possible == 0 {
        let missing = per_run
            .iter()
            .filter(|&&(item, count)| get(station, item) + get(storage, item) < count)
            .map(|&(item, count)| (item, count - get(station, item) - get(storage, item)))
            .collect();
        return Feed { runs: 0, moves: Vec::new(), missing, selections };
    }
    let moves = per_run
        .iter()
        .filter_map(|&(item, count)| {
            let need = (count * possible).saturating_sub(get(station, item));
            (need > 0).then_some((item, need))
        })
        .collect();
    Feed { runs: possible, moves, missing: Vec::new(), selections }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table() -> Table {
        Table::parse(
            "recipe\t1\t100\t60\t0\t0\t0\titem:10:2,cat:50:1\t20:5\tStew\n\
             category\t50\t30:1.0,31:2.0\n",
        )
    }

    fn counts(pairs: &[(u32, u32)]) -> Counts {
        pairs.iter().copied().collect()
    }

    #[test]
    fn runs_round_up_to_whole_batches() {
        let t = table();
        let r = t.recipe(1).unwrap();
        assert_eq!(runs_for(r, 20, 12), 3);
        assert_eq!(runs_for(r, 20, 10), 2);
        assert_eq!(runs_for(r, 20, 0), 0);
    }

    #[test]
    fn feeds_as_many_runs_as_storage_allows() {
        let t = table();
        let r = t.recipe(1).unwrap();
        // 5 runs asked, storage has meat (10) for 3, and option 30 for 4
        let plan = feed(&t, r, 5, &Counts::new(), &counts(&[(10, 6), (30, 4)]));
        assert_eq!(plan, Feed { runs: 3, moves: vec![(10, 6), (30, 3)], missing: vec![], selections: vec![(50, 0)] });
    }

    #[test]
    fn counts_what_the_station_already_holds() {
        let t = table();
        let r = t.recipe(1).unwrap();
        let plan = feed(&t, r, 2, &counts(&[(10, 3), (30, 2)]), &counts(&[(10, 10)]));
        assert_eq!(plan, Feed { runs: 2, moves: vec![(10, 1)], missing: vec![], selections: vec![(50, 0)] });
    }

    #[test]
    fn picks_the_category_option_with_most_runs_and_scales_it() {
        let t = table();
        let r = t.recipe(1).unwrap();
        // option 31 needs 2 per run: 9 of them are 4 runs, 1 of option 30 is 1 run
        let plan = feed(&t, r, 3, &Counts::new(), &counts(&[(10, 20), (30, 1), (31, 9)]));
        assert_eq!(plan, Feed { runs: 3, moves: vec![(10, 6), (31, 6)], missing: vec![], selections: vec![(50, 1)] });
    }

    #[test]
    fn says_what_is_missing() {
        let t = table();
        let r = t.recipe(1).unwrap();
        let plan = feed(&t, r, 1, &Counts::new(), &counts(&[(10, 1)]));
        assert_eq!(plan.runs, 0);
        assert_eq!(plan.missing, vec![(10, 1), (30, 1)]);
    }
}
