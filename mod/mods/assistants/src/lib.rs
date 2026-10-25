//! Assistants: survivors and their assistants run the base's production stations on order.
//! A player orders an amount of something one of the survivor's stations makes (Yakstew
//! at the Farmer's Kitchen, iron bars at the Blacksmith's Smelter …); the survivor fills
//! the station from the chests around it, the game's own station makes the item, and the
//! results go into the chests. Nothing is created from nothing: the station consumes and
//! produces as it would for a player, with its crafting time, fuel and power.
//!
//! Built from the SDK: `entities_near` (NPCs, stations), components (`ActiveNpcState`,
//! `FactoryStation`, `Crafting`), `inventories_near`, `inventory`, `move_items`. The base
//! is only loaded while a player is near it, so the work pauses while everybody is away.
//! Standing jobs (`jobs.rs`): the Farmer and their assistant harvest what grows around
//! them, the Fisher fishes with bait from the chests.
//!
//! See `mod/docs/assistants.md`.

mod base;
mod chain;
mod feeding;
mod fishing;
mod jobs;
mod names;
mod orders;
mod planner;
mod plants;
mod recipes;
mod walk;
mod work;

use std::collections::HashMap;

use shroudlink_sdk::ui::{Event, Item, MenuEntry, Node, ToClient, UiEvent, Window};
use shroudlink_sdk::{export_mod, log, server_dir, Host, Mod, Value, World};

use base::{Base, WorldBase};
use jobs::{Job, Kind};
use orders::{Order, Orders, Status, FILE};
use recipes::{table, Recipe};

const ID: &str = "assistants";
/// Ticks (about a second each) between work steps.
const WORK_EVERY: u32 = 5;
/// Ticks between the gardener's rounds: plants that refill (coffee, coconuts) grow one
/// item at a time, so harvesting at every step would bring in single items.
const HARVEST_EVERY: u32 = 10;
/// How near their work site survivors start working at a station, in metres.
const AT_WORK: f32 = 8.0;
/// Ticks between notifications about harvests and catches (all of them in one message).
const NEWS_EVERY: u32 = 60;
/// How far around a player assistants are looked for, in metres (the loaded world).
const SCAN: f64 = 150.0;
/// How near the player an assistant must be to take orders in the window.
const NEAR: f64 = 40.0;
const MAX_AMOUNT: u32 = 9999;

/// What a client's window shows.
#[derive(Default)]
struct View {
    /// the survivor whose window is open: (entity, npc id)
    npc: Option<(u32, u32)>,
    product: Option<u32>,
    amount: String,
    message: Option<String>,
    /// order ids in the order the window lists them
    listed: Vec<u32>,
    /// survivors the main window lists: (entity, npc id)
    survivors: Vec<(u32, u32)>,
    /// products the survivor window offers
    products: Vec<u32>,
    /// which page of products is shown
    page: usize,
    /// the baits the fisher's window lists, in its order
    baits: Vec<u32>,
}

/// Amounts the quick buttons order.
const QUICK: [u32; 4] = [1, 5, 10, 20];

/// Products shown at a time (mod pages do not scroll).
const PAGE: usize = 24;

#[derive(Default)]
pub struct Assistants {
    orders: Orders,
    dice: Option<fishing::Dice>,
    ticks: u32,
    views: HashMap<u64, View>,
    /// harvests and catches not announced yet: (workshop, who, what they did, items)
    news: Vec<(u32, String, Kind, Vec<(u32, u32)>)>,
    /// the tick of the last such notification
    news_sent: u32,
}

/// The survivor of a workshop ("Farmer": the Farmer and the Farmer's assistant).
fn npc_name(workshop: u32) -> String {
    let t = table();
    let people: Vec<_> = t.npcs.iter().filter(|n| n.workshop == workshop).collect();
    people.iter().find(|n| !n.assistant).or(people.first()).map_or_else(|| format!("workshop {workshop}"), |n| names::npc(&n.name))
}

/// Survivors near a player who run stations or have a job: (entity, npc id).
fn survivors_near(base: &dyn Base, player: u32) -> Vec<(u32, u32)> {
    let t = table();
    let works = |w: u32| t.stations_of(w).next().is_some() || !jobs::kinds_for(w).is_empty();
    let mut found: Vec<(u32, u32)> = Vec::new();
    for (entity, w) in base.npcs_near(player, NEAR) {
        // a survivor and their assistant are one entry
        if works(w) && !found.iter().any(|f| f.1 == w) {
            found.push((entity, w));
        }
    }
    found
}

/// Who does a workshop's work, for notifications: "Farmer's assistant" when the
/// assistant is there, else the survivor.
fn worker_name(base: &dyn Base, workshop: u32, npcs: &[u32]) -> String {
    let t = table();
    let assistant = npcs.iter().any(|&n| base.is_assistant(n)) && t.npcs.iter().any(|n| n.workshop == workshop && n.assistant);
    match t.npcs.iter().find(|n| n.workshop == workshop && n.assistant == assistant) {
        Some(n) if assistant => names::npc(&n.name),
        _ => npc_name(workshop),
    }
}

/// "39 Coconut Water, 2 Red Berries and 1 Egg"
fn item_list(items: &[(u32, u32)]) -> String {
    let mut parts: Vec<String> = items.iter().map(|&(item, n)| format!("{n} {}", names::item(item))).collect();
    match parts.len() {
        0 => String::new(),
        1 => parts.remove(0),
        _ => {
            let last = parts.pop().unwrap_or_default();
            format!("{} and {last}", parts.join(", "))
        }
    }
}

fn job_label(kind: Kind) -> &'static str {
    match kind {
        Kind::Harvest => "Harvest the crops, planted trees and tame animals around them, and replant the crops",
        Kind::Fish => "Fish with bait from the chests (a catch a minute)",
        Kind::Feed => "Keep the animal feeders filled, with food from the chests or made from what they hold",
    }
}

fn job_line(job: &Job) -> String {
    match job.kind {
        Kind::Harvest => format!("{} harvested, {} replanted so far - {}", job.done, job.replanted, job.status),
        Kind::Fish => format!("{} fish caught so far - {}", job.done, job.status),
        Kind::Feed => format!("{} food put out so far - {}", job.done, job.status),
    }
}

/// Stations of a survivor's workshops standing near it, by workshop.
fn stations_here(base: &dyn Base, npc_entity: u32, workshop: u32) -> Vec<u32> {
    let t = table();
    let mine: Vec<u32> = t.stations_of(workshop).map(|s| s.workshop).collect();
    let mut found: Vec<u32> = base.stations_near(npc_entity, work::RADIUS).into_iter().map(|s| s.workshop).filter(|w| mine.contains(w)).collect();
    found.sort();
    found.dedup();
    found
}

/// The recipe a survivor uses for a product: one on a station standing near it.
fn recipe_for(base: &dyn Base, npc_entity: u32, workshop: u32, product: u32) -> Option<&'static Recipe> {
    let here = stations_here(base, npc_entity, workshop);
    table().recipes_for(workshop, product).into_iter().find(|r| here.contains(&r.workshop))
}

fn order_line(o: &Order) -> String {
    let status = o.status.describe(names::item);
    format!("{} {}/{} - {}", names::item(o.product), o.produced.min(o.amount), o.amount, status)
}

impl Assistants {
    fn window(&mut self, host: &Host, client: u64) -> ToClient {
        let world = host.world();
        let base = WorldBase { world: host.world() };
        let view = self.views.entry(client).or_default();
        let mut content = Vec::new();
        // what the last click did, where the player looks: under the order row, or on top
        let message = view.message.clone().map(|text| Node::Text { text });
        let (title, id) = match (view.npc, world.player_of(client)) {
            (_, None) => {
                content.push(Node::Text { text: "Open any chest once, so the server knows which player you are.".into() });
                ("Assistants".to_string(), "main")
            }
            (None, Some(player)) => {
                content.extend(message.clone());
                view.survivors = survivors_near(&base, player);
                if view.survivors.is_empty() {
                    content.push(Node::Text { text: "No survivor or assistant is near you. Stand at your base, close to the one who should work.".into() });
                } else {
                    content.push(Node::Text { text: "Who should work? They run their stations nearby with what is in the chests around them.".into() });
                    // name and orders in two columns (a tab: the addon lines them up)
                    let entries = view
                        .survivors
                        .iter()
                        .map(|&(_, id)| match self.orders.of(id).filter(|o| !o.done()).count() {
                            1 => format!("{}\t1 open order", npc_name(id)),
                            n => format!("{}\t{n} open orders", npc_name(id)),
                        })
                        .collect();
                    content.push(Node::List { id: "survivors".into(), entries });
                }
                ("Assistants".to_string(), "main")
            }
            (Some((entity, npc_id)), Some(_)) => {
                let t = table();
                content.push(Node::Toggle { id: "notify".into(), label: "Notifications about their work".into(), value: !self.orders.muted.contains(&npc_id) });
                for kind in jobs::kinds_for(npc_id) {
                    let job = self.orders.jobs.iter().find(|j| j.npc == npc_id && j.kind == kind);
                    content.push(Node::Toggle { id: kind.name().into(), label: job_label(kind).into(), value: job.is_some() });
                    if let Some(job) = job {
                        content.push(Node::Text { text: job_line(job) });
                        if kind == Kind::Fish {
                            content.push(Node::Button { id: "fish_here".into(), label: "Fish here (where you stand)".into() });
                            // the baits in the order they are used, with what the chests hold
                            let chests = base.chests_near(entity, work::RADIUS);
                            let mut stock: HashMap<u32, u32> = HashMap::new();
                            for slot in chests.iter().flat_map(|&c| base.slots(c)) {
                                *stock.entry(slot.item).or_default() += slot.count;
                            }
                            let order = jobs::bait_order(&job.baits);
                            // icons and the game's names in the tooltip; the number is the stock
                            let items = order.iter().map(|&b| Item { id: b, count: stock.get(&b).copied().unwrap_or(0) }).collect();
                            content.push(Node::Heading { text: "Bait order".into() });
                            content.push(Node::Text { text: "They use the first of these baits that is in the chests, from left to right. Click a bait to use it first.".into() });
                            content.push(Node::ItemGrid { id: "baits".into(), columns: 8, items });
                            view.baits = order;
                        }
                    }
                }
                let here = stations_here(&base, entity, npc_id);
                let names: Vec<String> = here.iter().filter_map(|w| t.station(*w)).map(|s| names::pretty(&s.name)).collect();
                if t.stations_of(npc_id).next().is_some() {
                    content.push(Node::Text {
                        text: match names.len() {
                            0 => "None of their stations stands within 40 m.".into(),
                            1..=3 => format!("Stations: {}", names.join(", ")),
                            n => format!("{n} stations: {} ...", names[..3].join(", ")),
                        },
                    });
                }
                let workshop = npc_id;
                view.products = t.products_of(workshop).into_iter().filter(|(_, rs)| rs.iter().any(|r| here.contains(&r.workshop))).map(|(item, _)| item).collect();
                if !view.products.is_empty() {
                    let pages = view.products.len().div_ceil(PAGE);
                    view.page = view.page.min(pages - 1);
                    let shown = &view.products[view.page * PAGE..((view.page + 1) * PAGE).min(view.products.len())];
                    let items = shown.iter().map(|&p| Item { id: p, count: recipe_for(&base, entity, npc_id, p).map_or(1, |r| r.makes(p)) }).collect();
                    content.push(Node::ItemGrid { id: "products".into(), columns: 8, items });
                    if pages > 1 {
                        content.push(Node::Row {
                            children: vec![
                                Node::Button { id: "prev".into(), label: "«".into() },
                                Node::Text { text: format!("{} of {pages}", view.page + 1) },
                                Node::Button { id: "next".into(), label: "»".into() },
                            ],
                        });
                    }
                    content.push(Node::Text {
                        text: match view.product.and_then(|p| Some((p, recipe_for(&base, entity, npc_id, p)?))) {
                            Some((p, r)) => format!("{}: {} per run, {:.0} s a run", names::item(p), r.makes(p), r.seconds),
                            None => "Click what they should make.".into(),
                        },
                    });
                    let mut row: Vec<Node> = QUICK.iter().map(|n| Node::Button { id: format!("make{n}"), label: format!("Make {n}") }).collect();
                    row.push(Node::TextInput { id: "amount".into(), value: view.amount.clone(), hint: "how many".into() });
                    row.push(Node::Button { id: "order".into(), label: "Order".into() });
                    content.push(Node::Row { children: row });
                }
                content.extend(message.clone());
                // open orders, each followed by the sub-orders making its ingredients
                let mut listed: Vec<&Order> = Vec::new();
                for top in self.orders.of(npc_id).filter(|o| !o.done() && o.parent.is_none()) {
                    listed.push(top);
                    listed.extend(self.orders.children(top.id).iter().filter_map(|id| self.orders.list.iter().find(|o| o.id == *id)));
                }
                let done: Vec<&Order> = self.orders.of(npc_id).filter(|o| o.done()).collect();
                listed.extend(done.into_iter().rev().take(5));
                view.listed = listed.iter().map(|o| o.id).collect();
                if !listed.is_empty() {
                    content.push(Node::Heading { text: "Orders".into() });
                    let line = |o: &Order| if o.parent.is_some() { format!("  > {}", order_line(o)) } else { order_line(o) };
                    content.push(Node::List { id: "orders".into(), entries: listed.iter().map(|o| line(o)).collect() });
                    content.push(Node::Text { text: "Click an open order to cancel it, a finished one to clear it.".into() });
                }
                (npc_name(npc_id), "npc")
            }
        };
        let mut buttons = Vec::new();
        if let Some((_, npc)) = view.npc {
            buttons.push(Node::Button { id: "back".into(), label: "« Back".into() });
            if self.orders.of(npc).any(|o| o.done()) {
                buttons.push(Node::Button { id: "clear".into(), label: "Clear finished".into() });
            }
        }
        buttons.push(Node::Button { id: "refresh".into(), label: "Refresh".into() });
        content.push(Node::Row { children: buttons });
        ToClient::Open { owner: ID.into(), window: Window { id: id.into(), title, content } }
    }

    /// Feedback on a click: in the window and as a toast, as the window may be scrolled.
    fn tell(&mut self, host: &mut Host, client: u64, text: String) {
        host.send(client, &ToClient::Notify { owner: ID.into(), text: text.clone() }.to_message());
        self.views.entry(client).or_default().message = Some(text);
    }

    /// Sends the client's window, closing the other one: one window at a time.
    fn show(&mut self, host: &mut Host, client: u64) {
        let window = self.window(host, client);
        if let ToClient::Open { window: w, .. } = &window {
            let other = if w.id == "main" { "npc" } else { "main" };
            host.send(client, &ToClient::Close { owner: ID.into(), id: other.into() }.to_message());
        }
        host.send(client, &window.to_message());
    }

    fn place(&mut self, host: &Host, client: u64) -> String {
        let world = host.world();
        let base = WorldBase { world: host.world() };
        let view = self.views.entry(client).or_default();
        let (Some((entity, npc_id)), Some(product)) = (view.npc, view.product) else { return "Click what they should make first.".into() };
        let Ok(amount) = view.amount.trim().parse::<u32>() else { return "Type how many they should make.".into() };
        if amount == 0 || amount > MAX_AMOUNT {
            return format!("Between 1 and {MAX_AMOUNT}, please.");
        }
        let Some(recipe) = recipe_for(&base, entity, npc_id, product) else { return "Their station for it is not nearby any more.".into() };
        let by = world.player_of(client).and_then(|p| world.player_name(p)).unwrap_or_else(|| "someone".into());
        self.orders.add(npc_id, product, recipe.id, amount, &by);
        log!("assistants: {by} ordered {amount} {} from {} ({})", names::item(product), npc_name(npc_id), recipe.name);
        format!("Ordered {amount} {}.", names::item(product))
    }

    fn cancel(&mut self, host: &Host, client: u64, index: usize) -> Option<String> {
        let base = WorldBase { world: host.world() };
        let view = self.views.get(&client)?;
        let (entity, _) = view.npc?;
        let id = *view.listed.get(index)?;
        let pos = self.orders.list.iter().position(|o| o.id == id)?;
        if self.orders.list[pos].done() {
            let order = self.orders.list.remove(pos);
            self.orders.save();
            return Some(format!("Cleared: {} {}.", order.amount, names::item(order.product)));
        }
        let mut order = self.orders.list.remove(pos);
        work::release(table(), &mut order, entity, &base, true);
        for child in self.orders.children(order.id) {
            if let Some(p) = self.orders.list.iter().position(|o| o.id == child) {
                let mut sub = self.orders.list.remove(p);
                work::release(table(), &mut sub, entity, &base, true);
            }
        }
        self.orders.save();
        log!("assistants: order {} ({} {}) cancelled, {} made", order.id, order.amount, names::item(order.product), order.produced);
        Some(format!("Cancelled: {} {} ({} made).", order.amount, names::item(order.product), order.produced))
    }

    /// Switches a standing job on or off.
    fn switch_job(&mut self, client: u64, kind: Kind, on: bool) {
        let Some((_, npc)) = self.views.get(&client).and_then(|v| v.npc) else { return };
        self.orders.jobs.retain(|j| !(j.npc == npc && j.kind == kind));
        if on {
            self.orders.jobs.push(Job::new(npc, kind));
        }
        self.orders.save();
        log!("assistants: {} {} {}", npc_name(npc), kind.name(), if on { "on" } else { "off" });
    }

    /// A work step for every open order and job whose survivor is near a player.
    fn work(&mut self, host: &mut Host) -> bool {
        if self.orders.list.iter().all(|o| o.done()) && self.orders.jobs.is_empty() && self.orders.homes.is_empty() {
            return false;
        }
        let world: World = host.world();
        let base = WorldBase { world: host.world() };
        let mut at: HashMap<u32, u32> = HashMap::new();
        let mut people: HashMap<u32, Vec<u32>> = HashMap::new();
        for player in world.players() {
            for (entity, id) in base.npcs_near(player, SCAN) {
                at.entry(id).or_insert(entity);
                let list = people.entry(id).or_default();
                if !list.contains(&entity) {
                    list.push(entity);
                }
            }
        }
        // where each workshop's survivors should be: their work site, if they have one
        let mut want: HashMap<u32, walk::Want> = HashMap::new();
        let mut wish = |workshop: u32, w: walk::Want| {
            let entry = want.entry(workshop).or_insert(walk::Want::Free);
            *entry = entry.or(w);
        };
        let mut claimed = Vec::new();
        // workshops whose stations run an order, and where
        let mut at_work: Vec<(u32, [f32; 3])> = Vec::new();
        let mut changed = false;
        let mut finished = Vec::new();
        let muted = self.orders.muted.clone();
        let mut subs: Vec<(u32, chain::SubOrder, String)> = Vec::new();
        let open: Vec<(u32, u32)> = self.orders.list.iter().filter(|o| !o.done()).filter_map(|o| Some((o.parent?, o.product))).collect();
        let present: Vec<u32> = at.keys().copied().collect();
        let chains: HashMap<u32, (Vec<u32>, usize)> = self.orders.list.iter().filter(|o| !o.done()).map(|o| (o.id, self.orders.ancestors(o))).collect();
        for order in self.orders.list.iter_mut().filter(|o| !o.done()) {
            let before = (order.produced, order.status.clone());
            order.status = match at.get(&order.npc) {
                Some(&entity) => work::step(table(), order, entity, &base, &mut claimed),
                None => Status::Away,
            };
            // short of something a station here makes: order it first, then wait for it
            if let (Status::Missing(missing), Some(&entity)) = (&order.status, at.get(&order.npc)) {
                let mut stations: Vec<u32> = base.stations_near(entity, work::RADIUS).into_iter().map(|s| s.workshop).collect();
                stations.dedup();
                let (ancestors, depth) = chains.get(&order.id).cloned().unwrap_or_default();
                let here = chain::Here { stations: &stations, people: &present };
                for sub in chain::sub_orders(table(), order, missing, depth, &ancestors, &here, &open) {
                    subs.push((order.id, sub, order.by.clone()));
                }
                let coming: Vec<u32> = missing.iter().map(|m| m.0).filter(|&item| open.contains(&(order.id, item)) || subs.iter().any(|s| s.0 == order.id && s.1.product == item)).collect();
                if !coming.is_empty() {
                    order.status = Status::Waiting(coming);
                }
            }
            // at the stations the order runs on
            if let (Some(&entity), Some(recipe)) = (at.get(&order.npc), table().recipe(order.recipe)) {
                let busy: Vec<[f64; 3]> = base
                    .stations_near(entity, work::RADIUS)
                    .into_iter()
                    .filter(|s| s.workshop == recipe.workshop && (s.next == recipe.id || s.running == recipe.id))
                    .filter_map(|s| base.position(s.entity))
                    .collect();
                let site = walk::site_of(&busy);
                if let Some(site) = site {
                    at_work.push((order.npc, site));
                }
                wish(order.npc, site.map_or(walk::Want::Stay, walk::Want::Go));
            }
            if (order.produced, order.status.clone()) != before {
                changed = true;
                if order.status == Status::Done {
                    if !muted.contains(&order.npc) {
                        finished.push(format!("{} finished {} {}.", npc_name(order.npc), order.produced, names::item(order.product)));
                    }
                    log!("assistants: order {} done, {} {} made", order.id, order.produced, names::item(order.product));
                }
            }
        }
        for (parent, sub, by) in subs {
            let id = self.orders.add_for(sub.workshop, sub.product, sub.recipe, sub.amount, &by, Some(parent));
            log!("assistants: order {parent} needs {} {} first: order {id} at {}", sub.amount, names::item(sub.product), npc_name(sub.workshop));
            if !muted.contains(&sub.workshop) {
                finished.push(format!("{} first makes {} {} for an order.", npc_name(sub.workshop), sub.amount, names::item(sub.product)));
            }
            changed = true;
        }
        // a finished order no longer needs what its sub-orders still make
        let done: Vec<u32> = self.orders.list.iter().filter(|o| o.done()).map(|o| o.id).collect();
        let orphans: Vec<u32> = done.iter().flat_map(|&d| self.orders.children(d)).collect();
        for id in orphans {
            if let Some(pos) = self.orders.list.iter().position(|o| o.id == id) {
                let mut order = self.orders.list.remove(pos);
                if let Some(&entity) = at.get(&order.npc) {
                    work::release(table(), &mut order, entity, &base, false);
                }
                log!("assistants: order {id} no longer needed, stopped with {} made", order.produced);
                changed = true;
            }
        }
        let dice = self.dice.get_or_insert_with(|| fishing::Dice::new(std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(1, |d| d.as_nanos() as u64)));
        for job in self.orders.jobs.iter_mut() {
            let before = (job.done, job.status.clone());
            match at.get(&job.npc) {
                None => job.status = "waiting until someone is at the base".into(),
                Some(&entity) => match job.kind {
                    Kind::Harvest if self.ticks % HARVEST_EVERY >= WORK_EVERY => {
                        job.news.clear();
                        wish(job.npc, job.site.map_or(walk::Want::Stay, walk::Want::Go));
                    }
                    Kind::Harvest => {
                        jobs::harvest(&base, entity, job);
                        wish(job.npc, job.site.map_or(walk::Want::Stay, walk::Want::Go));
                    }
                    Kind::Fish => {
                        jobs::fish(&base, entity, WORK_EVERY, job, dice);
                        wish(job.npc, job.spot.map_or(walk::Want::Free, walk::Want::Go));
                    }
                    Kind::Feed => {
                        // walks to the feeder just filled, then goes on with the rest
                        let site = (jobs::feed(&base, entity, job) > 0).then_some(job.site).flatten();
                        wish(job.npc, site.map_or(walk::Want::Stay, walk::Want::Go));
                    }
                },
            }
            changed |= (job.done, job.status.clone()) != before;
            if !job.news.is_empty() && !muted.contains(&job.npc) {
                let who = people.get(&job.npc).map_or_else(|| npc_name(job.npc), |npcs| worker_name(&base, job.npc, npcs));
                match self.news.iter_mut().find(|n| n.1 == who && n.2 == job.kind) {
                    Some(n) => {
                        for &(item, count) in &job.news {
                            match n.3.iter_mut().find(|i| i.0 == item) {
                                Some(i) => i.1 += count,
                                None => n.3.push((item, count)),
                            }
                        }
                    }
                    None => self.news.push((job.npc, who, job.kind, job.news.clone())),
                }
            }
        }
        // the gardener keeps the seedlings for the fields in stock: Seedbed orders
        let mut seed_orders: Vec<(u32, u32, u32, u32)> = Vec::new();
        for job in self.orders.jobs.iter().filter(|j| j.kind == Kind::Harvest && !j.peaks.is_empty()) {
            let Some(&entity) = at.get(&job.npc) else { continue };
            let chests = base.chests_near(entity, work::RADIUS);
            let stock = |item: u32| chests.iter().flat_map(|&c| base.slots(c)).filter(|s| s.item == item).map(|s| s.count).sum::<u32>();
            for (seedling, amount) in jobs::seedlings_short(&job.peaks, stock) {
                let ordered = self.orders.list.iter().any(|o| !o.done() && o.product == seedling);
                if let (false, Some(recipe)) = (ordered, recipe_for(&base, entity, job.npc, seedling)) {
                    seed_orders.push((job.npc, seedling, recipe.id, amount));
                }
            }
        }
        for (npc, seedling, recipe, amount) in seed_orders {
            let id = self.orders.add(npc, seedling, recipe, amount, "the gardener");
            log!("assistants: order {id}: the gardener restocks {amount} {}", names::item(seedling));
            if !muted.contains(&npc) {
                finished.push(format!("{} sows {amount} {} for the fields.", npc_name(npc), names::item(seedling)));
            }
            changed = true;
        }
        // survivors walk to their work, and home again when it is done
        let mut workshops: Vec<u32> = want.keys().copied().collect();
        workshops.extend(self.orders.homes.iter().map(|m| m.workshop));
        workshops.sort();
        workshops.dedup();
        for workshop in workshops {
            let Some(npcs) = people.get(&workshop) else { continue };
            changed |= walk::send(&base, workshop, npcs, want.get(&workshop).copied().unwrap_or(walk::Want::Free), &mut self.orders.homes);
        }
        // at the work site with nothing to do: they work at their stations, so it shows
        for &(workshop, site) in &at_work {
            let Some(npcs) = people.get(&workshop) else { continue };
            for npc in walk::workers(&base, npcs) {
                let near = base.position(npc).is_some_and(|p| (0..3).map(|i| (p[i] as f32 - site[i]).powi(2)).sum::<f32>() < AT_WORK * AT_WORK);
                // each interaction is short: sent again whenever they stand idle
                if near {
                    base.work_at_station(npc);
                }
            }
        }
        if changed {
            self.orders.save();
        }
        if !self.news.is_empty() && self.ticks.wrapping_sub(self.news_sent) >= NEWS_EVERY {
            self.news_sent = self.ticks;
            // switched off meanwhile: what was collected goes unannounced too
            for (_, who, kind, items) in self.news.drain(..).filter(|n| !muted.contains(&n.0)) {
                finished.push(match kind {
                    Kind::Harvest => format!("{who} harvested {}.", item_list(&items)),
                    Kind::Fish => format!("{who} caught {}.", item_list(&items)),
                    Kind::Feed => format!("{who} filled the feeders with {}.", item_list(&items)),
                });
            }
        }
        for text in finished {
            for &client in self.views.keys() {
                host.send(client, &ToClient::Notify { owner: ID.into(), text: text.clone() }.to_message());
            }
        }
        changed
    }
}

impl Mod for Assistants {
    fn id(&self) -> &'static str {
        ID
    }

    fn on_load(&mut self) {
        self.orders = Orders::load(&server_dir().join(FILE));
        log!("assistants: {} open order(s)", self.orders.list.iter().filter(|o| !o.done()).count());
    }

    fn on_join(&mut self, host: &mut Host, client: u64) {
        self.views.insert(client, View::default());
        let entries = ToClient::Entries { owner: ID.into(), entries: vec![MenuEntry { id: "open".into(), label: "Assistants".into() }] };
        host.send(client, &entries.to_message());
    }

    fn on_message(&mut self, host: &mut Host, client: u64, message: &Value) {
        let Some(UiEvent { node, event, .. }) = UiEvent::from_message(message) else { return };
        let view = self.views.entry(client).or_default();
        view.message = None;
        match (node.as_str(), event) {
            ("open", Event::Menu) => view.npc = None,
            ("refresh", Event::Click { .. }) => {}
            ("back", Event::Click { .. }) => {
                view.npc = None;
                view.product = None;
            }
            ("survivors", Event::Click { index: Some(i) }) => {
                view.npc = view.survivors.get(i as usize).copied();
                view.product = None;
                view.page = 0;
            }
            ("products", Event::Click { index: Some(i) }) => view.product = view.products.get(view.page * PAGE + i as usize).copied(),
            ("prev", Event::Click { .. }) => view.page = view.page.saturating_sub(1),
            ("next", Event::Click { .. }) => view.page += 1,
            ("fish_here", Event::Click { .. }) => {
                let world = host.world();
                let spot = world.player_of(client).and_then(|p| world.position(p)).map(|p| p.map(|v| v as f32));
                if let (Some(spot), Some(job)) = (spot, self.orders.jobs.iter_mut().find(|j| j.kind == Kind::Fish)) {
                    job.spot = Some(spot);
                    self.orders.save();
                    log!("assistants: the fisher fishes at {spot:?}");
                    self.views.entry(client).or_default().message = Some("The fisher will fish here.".into());
                }
            }
            ("baits", Event::Click { index: Some(i) }) => {
                let order = view.baits.clone();
                if let (Some(&bait), Some((_, npc))) = (order.get(i as usize), view.npc) {
                    if let Some(job) = self.orders.jobs.iter_mut().find(|j| j.npc == npc && j.kind == Kind::Fish) {
                        // the clicked bait first, the rest in the order they had
                        job.baits = std::iter::once(bait).chain(order.into_iter().filter(|&b| b != bait)).collect();
                        self.orders.save();
                        log!("assistants: {} uses {} first", npc_name(npc), names::item(bait));
                    }
                }
            }
            ("notify", Event::Change { value }) => {
                if let Some((_, npc)) = view.npc {
                    self.orders.muted.retain(|&m| m != npc);
                    if value.as_bool() == Some(false) {
                        self.orders.muted.push(npc);
                    }
                    self.orders.save();
                }
            }
            (name @ ("harvest" | "fish" | "feed"), Event::Change { value }) => {
                let kind = Kind::from_name(name).unwrap_or(Kind::Harvest);
                self.switch_job(client, kind, value.as_bool().unwrap_or(false));
            }
            ("amount", Event::Change { value }) => {
                view.amount = value.as_str().unwrap_or("").to_string();
                return;
            }
            ("amount", Event::Submit { value }) => {
                view.amount = value;
                let text = self.place(host, client);
                self.tell(host, client, text);
            }
            ("order", Event::Click { .. }) => {
                let text = self.place(host, client);
                self.tell(host, client, text);
            }
            (quick, Event::Click { .. }) if quick.strip_prefix("make").is_some_and(|n| n.parse::<u32>().is_ok()) => {
                view.amount = quick["make".len()..].to_string();
                let text = self.place(host, client);
                self.tell(host, client, text);
            }
            ("clear", Event::Click { .. }) => {
                if let Some((_, npc)) = view.npc {
                    let before = self.orders.list.len();
                    self.orders.list.retain(|o| !(o.npc == npc && o.done()));
                    let n = before - self.orders.list.len();
                    self.orders.save();
                    view.message = Some(format!("Cleared {n} finished order(s)."));
                }
            }
            ("orders", Event::Click { index: Some(i) }) => {
                if let Some(text) = self.cancel(host, client, i as usize) {
                    self.tell(host, client, text);
                }
            }
            ("main" | "npc", Event::Closed) => return,
            _ => return,
        }
        self.show(host, client);
    }

    fn on_leave(&mut self, _host: &mut Host, client: u64) {
        self.views.remove(&client);
    }

    fn on_tick(&mut self, host: &mut Host) {
        self.ticks += 1;
        if self.ticks % WORK_EVERY != 0 {
            return;
        }
        let started = std::time::Instant::now();
        let worked = self.work(host);
        // a step runs on the game thread: a slow one makes everybody rubberband
        let took = started.elapsed().as_secs_f64() * 1000.0;
        if took > 50.0 {
            log!("assistants: slow work step: {took:.0} ms");
        }
        if worked {
            // windows showing orders follow the progress
            let open: Vec<u64> = self.views.iter().filter(|(_, v)| v.npc.is_some()).map(|(&c, _)| c).collect();
            for client in open {
                self.show(host, client);
            }
        }
    }
}

export_mod!(Assistants);

#[cfg(test)]
mod tests {
    use super::*;
    use shroudlink_sdk::testing::MockHost;

    #[test]
    fn the_menu_entry_opens_a_window_that_explains_what_to_do() {
        let mut m = Assistants::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        assert_eq!(host.take_sent()[0].1["op"], "entries");
        let menu = UiEvent { window: String::new(), node: "open".into(), event: Event::Menu };
        m.on_message(&mut host.host(), 1, &menu.to_message(ID));
        let sent = host.take_sent();
        let open = &sent.iter().find(|m| m.1["op"] == "open").expect("a window").1;
        assert!(sent.iter().any(|m| m.1["op"] == "close" && m.1["id"] == "npc"), "the other window is closed");
        assert_eq!(open["window"]["title"], "Assistants");
        assert!(open["window"].to_string().contains("Open any chest once"));
    }

    #[test]
    fn typed_amounts_count_without_enter_and_finished_orders_clear() {
        let mut m = Assistants::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        let click = |m: &mut Assistants, host: &mut MockHost, node: &str, event: Event| {
            m.on_message(&mut host.host(), 1, &UiEvent { window: "npc".into(), node: node.into(), event }.to_message(ID));
            host.take_sent()
        };
        m.views.get_mut(&1).unwrap().npc = Some((50, 9));
        click(&mut m, &mut host, "amount", Event::Change { value: Value::String("7".into()) });
        assert_eq!(m.views[&1].amount, "7", "the field's text arrives before Order is clicked");
        // a quick button orders its amount; the answer also comes as a toast
        let sent = click(&mut m, &mut host, "make5", Event::Click { index: None });
        assert_eq!(m.views[&1].amount, "5");
        assert!(sent.iter().any(|s| s.1["op"] == "notify"));
        // a finished order in the list is cleared by a click, open ones stay
        let done = m.orders.add(9, 1, 1, 2, "p");
        m.orders.list.iter_mut().find(|o| o.id == done).unwrap().produced = 2;
        m.orders.list.iter_mut().find(|o| o.id == done).unwrap().status = Status::Done;
        let open = m.orders.add(9, 1, 1, 2, "p");
        m.views.get_mut(&1).unwrap().listed = vec![open, done];
        click(&mut m, &mut host, "orders", Event::Click { index: Some(1) });
        assert_eq!(m.orders.list.iter().map(|o| o.id).collect::<Vec<_>>(), vec![open]);
    }

    #[test]
    fn notifications_switch_off_per_survivor() {
        let mut m = Assistants::default();
        let mut host = MockHost::default();
        m.on_join(&mut host.host(), 1);
        m.views.get_mut(&1).unwrap().npc = Some((50, 9));
        let switch = |m: &mut Assistants, host: &mut MockHost, on: bool| {
            m.on_message(&mut host.host(), 1, &UiEvent { window: "npc".into(), node: "notify".into(), event: Event::Change { value: Value::Bool(on) } }.to_message(ID));
        };
        switch(&mut m, &mut host, false);
        assert_eq!(m.orders.muted, vec![9]);
        switch(&mut m, &mut host, true);
        assert!(m.orders.muted.is_empty());
    }

    #[test]
    fn harvest_news_reads_well() {
        assert_eq!(item_list(&[]), "");
        let one = item_list(&[(2160078991, 5)]);
        assert!(one.starts_with("5 ") && !one.contains(" and "), "{one}");
        assert!(item_list(&[(2160078991, 5), (1564406835, 2), (169532188, 1)]).contains(", 2 ") && item_list(&[(2160078991, 5), (1564406835, 2)]).contains(" and 2 "));
    }

    #[test]
    fn ticks_without_orders_do_nothing() {
        let mut m = Assistants::default();
        let mut host = MockHost::default();
        for _ in 0..WORK_EVERY * 2 {
            m.on_tick(&mut host.host());
        }
        assert!(host.take_sent().is_empty());
    }
}
