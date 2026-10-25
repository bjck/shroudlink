//! Chains: an order short of an ingredient that a station at this base can make orders
//! it first ("a Strawberry Milkshake needs Sugar: the Cooking Station makes Sugar from
//! Sugar Cane"). The parent waits and takes the ingredient from the chests in its own
//! steps once the sub-order has put it there. Pure, so it is tested without the game.

use crate::orders::Order;
use crate::planner;
use crate::recipes::{Input, Table};

/// How many levels of sub-orders an order may have.
pub const MAX_DEPTH: usize = 3;

#[derive(Clone, Debug, PartialEq)]
pub struct SubOrder {
    /// the survivor's workshop that runs the station
    pub workshop: u32,
    pub product: u32,
    pub recipe: u32,
    pub amount: u32,
}

/// The base a chain works with.
pub struct Here<'a> {
    /// station workshops standing near the order's survivor
    pub stations: &'a [u32],
    /// survivor workshops whose people are at the base
    pub people: &'a [u32],
}

/// Sub-orders for what `order` is missing for its next run (`missing`: item, count).
/// `ancestors`: products further up its chain (never ordered again: no loops); `depth`:
/// how many parents it has; `open`: (parent, product) of sub-orders already running.
pub fn sub_orders(table: &Table, order: &Order, missing: &[(u32, u32)], depth: usize, ancestors: &[u32], here: &Here, open: &[(u32, u32)]) -> Vec<SubOrder> {
    if depth >= MAX_DEPTH {
        return Vec::new();
    }
    let Some(recipe) = table.recipe(order.recipe) else { return Vec::new() };
    let runs = planner::runs_for(recipe, order.product, order.amount.saturating_sub(order.produced)).max(1);
    let mut out = Vec::new();
    for &(item, short) in missing {
        if item == order.product || ancestors.contains(&item) || open.contains(&(order.id, item)) {
            continue;
        }
        // a station here that makes it, run by someone here; the order's own survivor first
        let mut makers: Vec<(&crate::recipes::Recipe, u32)> = table
            .recipes
            .iter()
            .filter(|r| r.makes(item) > 0 && here.stations.contains(&r.workshop))
            // a maker that uses what the chain is making would go round in a circle
            .filter(|r| !r.inputs.iter().any(|i| matches!(*i, Input::Item { item: x, .. } if x == order.product || ancestors.contains(&x))))
            .filter_map(|r| Some((r, table.station(r.workshop)?.unlocked_by)))
            .filter(|(_, who)| here.people.contains(who))
            .collect();
        makers.sort_by_key(|(r, who)| (*who != order.npc, r.inputs.len()));
        let Some(&(maker, workshop)) = makers.first() else { continue };
        // enough for every run still to come, not just the next one
        let per_run: u32 = recipe.inputs.iter().filter_map(|i| match *i {
            Input::Item { item: x, count } if x == item => Some(count),
            _ => None,
        }).sum();
        let amount = (per_run.max(short) * runs).max(short);
        out.push(SubOrder { workshop, product: item, recipe: maker.id, amount });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orders::Status;

    const KITCHEN: u32 = 100;
    const COOKING: u32 = 200;
    const FARMER: u32 = 9;
    const SHAKE: u32 = 20;
    const SUGAR: u32 = 30;
    const CANE: u32 = 31;
    const MILK: u32 = 40;

    fn table() -> Table {
        Table::parse(
            "station\t100\t1\t9\tKitchen\n\
             station\t200\t2\t9\tCooking Station\n\
             recipe\t1\t100\t360\t0\t0\t0\titem:30:5,item:40:5\t20:5\tShake\n\
             recipe\t2\t200\t90\t0\t0\t0\titem:31:10\t30:5\tSugar\n\
             recipe\t3\t200\t90\t0\t0\t0\titem:20:1\t31:1\tLoop\n",
        )
    }

    fn shake(amount: u32) -> Order {
        Order { id: 1, npc: FARMER, product: SHAKE, recipe: 1, amount, produced: 0, by: "p".into(), status: Status::Away, parent: None, borrowed: Vec::new() }
    }

    #[test]
    fn a_missing_ingredient_is_made_first_on_a_station_here() {
        let t = table();
        let here = Here { stations: &[KITCHEN, COOKING], people: &[FARMER] };
        // 10 shakes are 2 runs of 5 sugar
        let subs = sub_orders(&t, &shake(10), &[(SUGAR, 5)], 0, &[], &here, &[]);
        assert_eq!(subs, vec![SubOrder { workshop: FARMER, product: SUGAR, recipe: 2, amount: 10 }]);
    }

    #[test]
    fn nothing_is_ordered_without_a_station_or_twice_or_in_a_loop() {
        let t = table();
        let no_station = Here { stations: &[KITCHEN], people: &[FARMER] };
        assert!(sub_orders(&t, &shake(5), &[(SUGAR, 5)], 0, &[], &no_station, &[]).is_empty());
        let here = Here { stations: &[KITCHEN, COOKING], people: &[FARMER] };
        assert!(sub_orders(&t, &shake(5), &[(SUGAR, 5)], 0, &[], &here, &[(1, SUGAR)]).is_empty(), "already on its way");
        assert!(sub_orders(&t, &shake(5), &[(MILK, 5)], 0, &[], &here, &[]).is_empty(), "nobody makes milk");
        // cane would be made from shakes: the shake is up the chain
        let sugar = Order { id: 2, product: SUGAR, recipe: 2, amount: 5, parent: Some(1), ..shake(5) };
        assert!(sub_orders(&t, &sugar, &[(CANE, 10)], 1, &[SHAKE], &here, &[]).is_empty());
        assert!(sub_orders(&t, &shake(5), &[(SUGAR, 5)], MAX_DEPTH, &[], &here, &[]).is_empty(), "too deep");
    }
}
