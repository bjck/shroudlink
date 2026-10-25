//! Production orders and their file, `shroudlink_assistants.json` next to the server.
//! Orders name the assistant by its `NpcId`, which stays the same across sessions (entity
//! ids do not), and follow the assistant to whatever base it lives at.

use std::path::{Path, PathBuf};

use shroudlink_sdk::{json, log, Value};

use crate::base::Home;
use crate::jobs::{Job, Kind};
use crate::walk::Moved;

pub const FILE: &str = "shroudlink_assistants.json";

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// nobody is near the assistant, so its base is not loaded
    Away,
    /// no station for the recipe near the assistant
    NoStation,
    /// every matching station runs something else
    /// all its stations work on something else: what one of them makes
    Busy(String),
    /// (item, count) missing in storage for the next run
    Missing(Vec<(u32, u32)>),
    /// missing items that sub-orders are making
    Waiting(Vec<u32>),
    /// the station lacks power (`missingRequirements`)
    NeedsEnergy,
    /// the station's tank is short of water and no Well nearby has any
    NeedsWater,
    /// no chest near the assistant has room for the results
    StorageFull,
    Running,
    Done,
}

impl Status {
    pub fn describe(&self, name: impl Fn(u32) -> String) -> String {
        match self {
            Status::Away => "waiting until someone is at the base".into(),
            Status::NoStation => "no station for it nearby".into(),
            Status::Busy(what) => format!("{what}, it waits until that is done"),
            Status::Missing(items) => format!("needs {}", items.iter().map(|&(i, n)| format!("{n} {}", name(i))).collect::<Vec<_>>().join(", ")),
            Status::Waiting(items) => format!("waiting for {} (being made)", items.iter().map(|&i| name(i)).collect::<Vec<_>>().join(", ")),
            Status::NeedsEnergy => "the station needs power".into(),
            Status::NeedsWater => "needs water: place a Well near it".into(),
            Status::StorageFull => "no room in the chests".into(),
            Status::Running => "working".into(),
            Status::Done => "done".into(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Order {
    pub id: u32,
    /// `NpcId` of the assistant (or survivor) doing the work
    pub npc: u32,
    pub product: u32,
    pub recipe: u32,
    pub amount: u32,
    /// results already put into storage
    pub produced: u32,
    /// the player who placed it
    pub by: String,
    pub status: Status,
    /// the order this one makes an ingredient for
    pub parent: Option<u32>,
    /// idle stations it took over from someone else's recipe: (station, recipe, options),
    /// given back when it no longer needs them
    pub borrowed: Vec<(u32, u32, Vec<(u32, u8)>)>,
}

impl Order {
    pub fn done(&self) -> bool {
        self.status == Status::Done
    }
    fn to_json(&self) -> Value {
        json!({ "id": self.id, "npc": self.npc, "product": self.product, "recipe": self.recipe, "amount": self.amount, "produced": self.produced, "by": self.by, "done": self.done(), "parent": self.parent, "borrowed": self.borrowed })
    }
    fn from_json(v: &Value) -> Option<Order> {
        let n = |k: &str| v[k].as_u64().map(|x| x as u32);
        Some(Order {
            id: n("id")?,
            npc: n("npc")?,
            product: n("product")?,
            recipe: n("recipe")?,
            amount: n("amount")?,
            produced: n("produced").unwrap_or(0),
            by: v["by"].as_str().unwrap_or("").into(),
            status: if v["done"].as_bool() == Some(true) { Status::Done } else { Status::Away },
            parent: v["parent"].as_u64().map(|p| p as u32),
            borrowed: serde_json::from_value(v["borrowed"].clone()).unwrap_or_default(),
        })
    }
}

#[derive(Default, Debug)]
pub struct Orders {
    pub list: Vec<Order>,
    /// standing jobs (harvesting, fishing) that are switched on
    pub jobs: Vec<Job>,
    /// survivors whose home point was moved to their work, with the one to give back
    pub homes: Vec<Moved>,
    next_id: u32,
    path: Option<PathBuf>,
}

/// Finished orders kept for the window (the newest ones).
const KEEP_DONE: usize = 20;

impl Orders {
    pub fn load(path: &Path) -> Orders {
        let mut orders = Orders { path: Some(path.to_path_buf()), ..Orders::default() };
        if let Ok(text) = std::fs::read_to_string(path) {
            match serde_json::from_str::<Value>(&text) {
                Ok(v) => {
                    orders.list = v["orders"].as_array().map(|a| a.iter().filter_map(Order::from_json).collect()).unwrap_or_default();
                    orders.next_id = v["next_id"].as_u64().unwrap_or(0) as u32;
                    orders.jobs = v["jobs"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|j| {
                                    let mut job = Job::new(j["npc"].as_u64()? as u32, Kind::from_name(j["kind"].as_str()?)?);
                                    job.done = j["done"].as_u64().unwrap_or(0) as u32;
                                    job.replanted = j["replanted"].as_u64().unwrap_or(0) as u32;
                                    job.spot = point(&j["spot"]);
                                    Some(job)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    orders.homes = v["homes"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|h| {
                                    let original = Home { pos: point(&h["pos"])?, override_range: h["override"].as_bool()?, range: h["range"].as_f64()? as f32 };
                                    // entity ids change between sessions: found again by workshop and site
                                    Some(Moved { workshop: h["workshop"].as_u64()? as u32, npc: 0, original, site: point(&h["site"])? })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                }
                Err(e) => log!("assistants: {} is not valid JSON ({e}), starting without orders", path.display()),
            }
        }
        orders.next_id = orders.next_id.max(orders.list.iter().map(|o| o.id + 1).max().unwrap_or(1));
        orders
    }

    pub fn add(&mut self, npc: u32, product: u32, recipe: u32, amount: u32, by: &str) -> u32 {
        self.add_for(npc, product, recipe, amount, by, None)
    }

    /// An order, possibly making an ingredient for another one (`parent`).
    pub fn add_for(&mut self, npc: u32, product: u32, recipe: u32, amount: u32, by: &str, parent: Option<u32>) -> u32 {
        let id = self.next_id.max(1);
        self.next_id = id + 1;
        self.list.push(Order { id, npc, product, recipe, amount, produced: 0, by: by.into(), status: Status::Away, parent, borrowed: Vec::new() });
        self.save();
        id
    }

    /// The products further up an order's chain, and how many parents it has.
    pub fn ancestors(&self, order: &Order) -> (Vec<u32>, usize) {
        let mut products = Vec::new();
        let mut parent = order.parent;
        while let Some(p) = parent.and_then(|id| self.list.iter().find(|o| o.id == id)) {
            if products.len() > crate::chain::MAX_DEPTH {
                break;
            }
            products.push(p.product);
            parent = p.parent;
        }
        let depth = products.len();
        (products, depth)
    }

    /// The open sub-orders under an order, all levels down.
    pub fn children(&self, id: u32) -> Vec<u32> {
        let mut out = Vec::new();
        let mut todo = vec![id];
        while let Some(p) = todo.pop() {
            for o in self.list.iter().filter(|o| o.parent == Some(p) && !o.done()) {
                if !out.contains(&o.id) {
                    out.push(o.id);
                    todo.push(o.id);
                }
            }
        }
        out
    }


    pub fn of(&self, npc: u32) -> impl Iterator<Item = &Order> {
        self.list.iter().filter(move |o| o.npc == npc)
    }

    /// Drops old finished orders and writes the file (`.new`, then renamed over the old one).
    pub fn save(&mut self) {
        let done: Vec<u32> = self.list.iter().filter(|o| o.done()).map(|o| o.id).collect();
        if done.len() > KEEP_DONE {
            let drop = &done[..done.len() - KEEP_DONE];
            self.list.retain(|o| !drop.contains(&o.id));
        }
        let Some(path) = &self.path else { return };
        let jobs: Vec<Value> = self.jobs.iter().map(|j| json!({ "npc": j.npc, "kind": j.kind.name(), "done": j.done, "replanted": j.replanted, "spot": j.spot })).collect();
        let homes: Vec<Value> = self
            .homes
            .iter()
            .map(|m| json!({ "workshop": m.workshop, "pos": m.original.pos, "override": m.original.override_range, "range": m.original.range, "site": m.site }))
            .collect();
        let text = json!({ "next_id": self.next_id, "orders": self.list.iter().map(Order::to_json).collect::<Vec<_>>(), "jobs": jobs, "homes": homes }).to_string();
        let new = path.with_extension("json.new");
        if let Err(e) = std::fs::write(&new, text).and_then(|_| std::fs::rename(&new, path)) {
            log!("assistants: could not save {}: {e}", path.display());
        }
    }
}

/// `[x, y, z]` from the file.
fn point(v: &Value) -> Option<[f32; 3]> {
    let a = v.as_array()?;
    Some([a.first()?.as_f64()? as f32, a.get(1)?.as_f64()? as f32, a.get(2)?.as_f64()? as f32])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orders_survive_a_restart() {
        let dir = std::env::temp_dir().join(format!("assistants_test_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(FILE);
        let _ = std::fs::remove_file(&path);
        let mut orders = Orders::load(&path);
        let a = orders.add(7, 20, 1, 40, "Bjarne");
        let b = orders.add(7, 21, 2, 5, "Bjarne");
        orders.list[0].produced = 15;
        orders.list[1].status = Status::Done;
        orders.jobs.push(Job { done: 12, spot: Some([1.0, 2.0, 3.0]), ..Job::new(9, Kind::Fish) });
        orders.homes.push(Moved { workshop: 9, npc: 0, original: Home { pos: [4.0, 5.0, 6.0], override_range: false, range: 30.0 }, site: [1.0, 2.0, 3.0] });
        orders.save();
        let again = Orders::load(&path);
        assert_eq!(again.list.len(), 2);
        assert_eq!((again.list[0].id, again.list[0].produced, again.list[0].status.clone()), (a, 15, Status::Away));
        assert!(again.list[1].done());
        assert_eq!((again.jobs[0].npc, again.jobs[0].kind, again.jobs[0].done, again.jobs[0].spot), (9, Kind::Fish, 12, Some([1.0, 2.0, 3.0])));
        assert_eq!(again.homes, orders.homes);
        let mut again = again;
        assert!(again.add(7, 22, 3, 1, "x") > b);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn only_the_newest_finished_orders_are_kept() {
        let mut orders = Orders::default();
        for _ in 0..30 {
            orders.add(1, 2, 3, 4, "p");
            orders.list.last_mut().unwrap().status = Status::Done;
        }
        orders.add(1, 2, 3, 4, "open");
        orders.save();
        assert_eq!(orders.list.len(), KEEP_DONE + 1);
        assert!(orders.list.iter().any(|o| o.by == "open"));
    }
}
