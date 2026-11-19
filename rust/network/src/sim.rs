//! One slot fits one bounded transfer per endpoint. Policies see the commissioned
//! schedule and local call outcomes. Exogenous outages are sampled after selection.
use anyhow::{Result, ensure};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Contact {
    pub slot: u32,
    pub from: usize,
    pub to: usize,
    pub group: usize,
    pub channel: u8,
    pub primary: bool,
    pub bytes: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub source: usize,
    pub destination: usize,
    pub start: u32,
    pub deadline: u32,
    pub size: u32,
    pub weight: u32,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub nodes: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub regions: Vec<Region>,
    pub slots: u32,
    pub queue_limit: usize,
    pub outage_percent: u32,
    pub contacts: Vec<Contact>,
    pub offers: Vec<Offer>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Region {
    pub name: String,
    pub latitude: f64,
    pub longitude: f64,
}
#[derive(Clone, Copy, Serialize, Debug)]
pub struct Policy {
    pub adapt_contacts: bool,
    pub adapt_routes: bool,
}
#[derive(Clone, Serialize, Default)]
pub struct ResultRow {
    pub offered: usize,
    pub delivered: usize,
    pub queued: usize,
    pub expired: usize,
    pub rejected: usize,
    pub weighted_offered: u64,
    pub weighted_delivered: u64,
    pub delivered_payload_bytes: u64,
    pub delivered_by_source: BTreeMap<usize, usize>,
    pub delivery_delay_slots: u64,
    pub control_bytes: u64,
    pub data_bytes: u64,
    pub failed_calls: u64,
    pub collisions: u64,
    pub link_changes: u64,
    pub trace: Vec<(u32, usize, usize, usize)>,
}
const CONTROL: u32 = 48; // illustrative byte-equivalent reservation for call/grant/ACK
const OVERHEAD: u32 = 64; // illustrative serialized envelope cost
fn noise(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
    x ^ (x >> 31)
}
impl Scenario {
    pub fn check(&self) -> Result<()> {
        ensure!(
            (2..=2000).contains(&self.nodes)
                && (1..=2048).contains(&self.slots)
                && self.queue_limit > 0
                && self.outage_percent <= 100,
            "invalid scenario limits"
        );
        ensure!(
            self.contacts.len() <= 200000 && self.offers.len() <= 20000,
            "scenario too large"
        );
        for c in &self.contacts {
            ensure!(
                c.from < self.nodes
                    && c.to < self.nodes
                    && c.from != c.to
                    && c.slot < self.slots
                    && c.bytes > CONTROL + OVERHEAD,
                "invalid contact"
            );
        }
        for o in &self.offers {
            ensure!(
                o.source < self.nodes
                    && o.destination < self.nodes
                    && o.source != o.destination
                    && o.start < o.deadline
                    && o.deadline <= self.slots
                    && o.size > 0
                    && o.size < 1000000
                    && o.weight > 0,
                "invalid offer"
            );
        }
        Ok(())
    }
}
pub fn six() -> Scenario {
    let edges = [
        (1, 0, 1),
        (8, 1, 2),
        (25, 2, 5),
        (4, 0, 3),
        (6, 3, 4),
        (9, 4, 5),
    ];
    Scenario {
        nodes: 6,
        regions: vec![],
        slots: 27,
        queue_limit: 4,
        outage_percent: 0,
        contacts: edges
            .into_iter()
            .map(|(slot, from, to)| Contact {
                slot,
                from,
                to,
                group: 0,
                channel: 0,
                primary: true,
                bytes: 512,
            })
            .collect(),
        offers: vec![Offer {
            source: 0,
            destination: 5,
            start: 0,
            deadline: 20,
            size: 256,
            weight: 1,
        }],
    }
}
pub fn generated(n: usize) -> Result<Scenario> {
    ensure!((6..=1000).contains(&n), "node count must be 6..1000");
    let mut contacts = Vec::new();
    for slot in 0..96 {
        for from in 1..n {
            // A commissioned binary tree with alternate parents and local channel reuse.
            if slot % 6 != (from % 6) as u32 {
                continue;
            }
            for channel in 0..2 {
                contacts.push(Contact {
                    slot,
                    from,
                    to: (from - 1) / 2,
                    group: from / 12,
                    channel,
                    primary: channel == 0,
                    bytes: 768,
                });
            }
            if from >= 4 {
                contacts.push(Contact {
                    slot,
                    from,
                    to: (from - 2) / 3,
                    group: from / 12,
                    channel: 1,
                    primary: true,
                    bytes: 768,
                });
            }
        }
    }
    Ok(Scenario {
        nodes: n,
        regions: vec![],
        slots: 96,
        contacts,
        queue_limit: 16,
        outage_percent: 25,
        offers: (1..n)
            .map(|source| Offer {
                source,
                destination: 0,
                start: (source % 13) as u32,
                deadline: 96,
                size: 128,
                weight: 1 + (source % 4) as u32,
            })
            .collect(),
    })
}
// Backward earliest-arrival labels on published contacts, with explicit wait edges.
// Capacity class and destination are fixed for each table. Slot end is exclusive.
fn arrival(
    s: &Scenario,
    slots: &[Vec<usize>],
    dest: usize,
    size: u32,
    channels: bool,
) -> Vec<Vec<u32>> {
    let mut table = vec![vec![u32::MAX; s.nodes]; s.slots as usize + 1];
    table[s.slots as usize][dest] = s.slots;
    for t in (0..s.slots as usize).rev() {
        let (rows, tail) = table.split_at_mut(t + 1);
        let row = &mut rows[t];
        let next = &tail[0];
        row.copy_from_slice(next);
        row[dest] = t as u32;
        for &i in &slots[t] {
            let c = &s.contacts[i];
            if (channels || c.primary) && size + OVERHEAD + CONTROL <= c.bytes {
                row[c.from] = row[c.from].min(next[c.to]);
            }
        }
    }
    table
}
pub fn run(s: &Scenario, policy: Policy, seed: u64, trace: bool) -> Result<ResultRow> {
    s.check()?;
    let mut slots = vec![Vec::new(); s.slots as usize];
    for (i, c) in s.contacts.iter().enumerate() {
        slots[c.slot as usize].push(i);
    }
    let static_routes = fixed_routes(s);
    let mut labels = BTreeMap::new();
    if policy.adapt_routes {
        for o in &s.offers {
            let key = (o.destination, o.size);
            if !labels.contains_key(&key) {
                ensure!(
                    (labels.len() + 1) * s.nodes * (s.slots as usize + 1) <= 16_000_000,
                    "routing table budget exceeded"
                );
                labels.insert(
                    key,
                    arrival(s, &slots, o.destination, o.size, policy.adapt_contacts),
                );
            }
        }
    }
    let mut queues = vec![Vec::<usize>::new(); s.nodes];
    let mut visited = vec![BTreeSet::new(); s.offers.len()];
    let mut result = ResultRow {
        offered: s.offers.len(),
        weighted_offered: s.offers.iter().map(|o| o.weight as u64).sum(),
        ..Default::default()
    };
    let mut failures = BTreeMap::<(usize, usize, u8), u32>::new();
    let mut previous = BTreeMap::new();
    for (t, available) in slots.iter().enumerate() {
        let time = t as u32;
        for q in &mut queues {
            q.retain(|id| {
                let expired = s.offers[*id].deadline <= time;
                result.expired += expired as usize;
                !expired
            });
        }
        for (id, o) in s.offers.iter().enumerate().filter(|(_, o)| o.start == time) {
            if queues[o.source].len() == s.queue_limit {
                result.rejected += 1;
            } else {
                queues[o.source].push(id);
                visited[id].insert(o.source);
            }
        }
        let mut choices = BTreeMap::new();
        for &i in available {
            let c = &s.contacts[i];
            if !policy.adapt_contacts && !c.primary {
                continue;
            }
            for &id in &queues[c.from] {
                let o = &s.offers[id];
                if !policy.adapt_routes && static_routes[&o.destination][c.from] != Some(c.to) {
                    continue;
                }
                if visited[id].contains(&c.to) || o.size + CONTROL + OVERHEAD > c.bytes {
                    continue;
                }
                let eta = if policy.adapt_routes {
                    let tab = &labels[&(o.destination, o.size)];
                    let next = tab[t + 1][c.to];
                    if next > o.deadline || next > tab[t + 1][c.from] {
                        continue;
                    }
                    next
                } else {
                    0
                };
                let cost = if policy.adapt_contacts {
                    *failures.get(&(c.from, c.to, c.channel)).unwrap_or(&0)
                } else {
                    0
                };
                let key = (std::cmp::Reverse(o.weight), o.deadline, eta, cost, i, id);
                let best = choices.entry(c.from).or_insert(key);
                *best = (*best).min(key);
            }
        }
        let mut occupied = BTreeMap::new();
        let mut medium = BTreeMap::new();
        for &(_, _, _, _, i, _) in choices.values() {
            let c = &s.contacts[i];
            *occupied.entry(c.from).or_insert(0) += 1;
            *occupied.entry(c.to).or_insert(0) += 1;
            *medium.entry((c.group, c.channel)).or_insert(0) += 1;
        }
        // Receiver ingress admission uses its queue at slot start (simultaneous calls collide).
        for (_, _, _, _, i, id) in choices.into_values() {
            let c = &s.contacts[i];
            let o = &s.offers[id];
            result.control_bytes += CONTROL as u64;
            let collision =
                occupied[&c.from] > 1 || occupied[&c.to] > 1 || medium[&(c.group, c.channel)] > 1;
            let outage = noise(
                seed ^ noise(c.group as u64)
                    ^ noise((time / 4) as u64 + 4000)
                    ^ noise(c.channel as u64 + 10000),
            ) % 100
                < u64::from(s.outage_percent);
            if collision || outage || (c.to != o.destination && queues[c.to].len() >= s.queue_limit)
            {
                result.failed_calls += 1;
                result.collisions += collision as u64;
                *failures.entry((c.from, c.to, c.channel)).or_default() += 1;
                continue;
            }
            failures.remove(&(c.from, c.to, c.channel));
            if previous
                .insert(c.from, (c.to, c.channel))
                .is_some_and(|old| old != (c.to, c.channel))
            {
                result.link_changes += 1;
            }
            queues[c.from].retain(|x| *x != id);
            visited[id].insert(c.to);
            result.data_bytes += u64::from(o.size + OVERHEAD);
            if trace {
                result.trace.push((time + 1, id, c.from, c.to));
            }
            if c.to == o.destination {
                result.delivered += 1;
                result.delivered_payload_bytes += u64::from(o.size);
                result.delivery_delay_slots += u64::from(time + 1 - o.start);
                *result.delivered_by_source.entry(o.source).or_default() += 1;
                result.weighted_delivered += o.weight as u64;
            } else {
                queues[c.to].push(id);
            }
        }
    }
    for id in queues.into_iter().flatten() {
        if s.offers[id].deadline <= s.slots {
            result.expired += 1
        } else {
            result.queued += 1
        }
    }
    ensure!(
        result.offered == result.delivered + result.expired + result.rejected + result.queued,
        "object accounting mismatch"
    );
    Ok(result)
}

// Shortest-hop forwarding over commissioned primary edges, independent of time
// and queue state. Numeric next-hop order breaks ties consistently.
fn fixed_routes(s: &Scenario) -> BTreeMap<usize, Vec<Option<usize>>> {
    let mut incoming = vec![BTreeSet::new(); s.nodes];
    for c in &s.contacts {
        if c.primary {
            incoming[c.to].insert(c.from);
        }
    }
    let mut out = BTreeMap::new();
    for dest in s
        .offers
        .iter()
        .map(|o| o.destination)
        .collect::<BTreeSet<_>>()
    {
        let mut distance = vec![usize::MAX; s.nodes];
        distance[dest] = 0;
        let mut next = vec![None; s.nodes];
        let mut work = std::collections::VecDeque::from([dest]);
        while let Some(n) = work.pop_front() {
            for &p in &incoming[n] {
                if distance[p] == usize::MAX {
                    distance[p] = distance[n] + 1;
                    next[p] = Some(n);
                    work.push_back(p);
                } else if distance[p] == distance[n] + 1 && next[p].is_some_and(|old| n < old) {
                    next[p] = Some(n);
                }
            }
        }
        out.insert(dest, next);
    }
    out
}
// Synthetic UTC schedule: six commissioned regional gateways in a ring.
// Longitude selects local solar time. No empirical propagation probabilities.
pub fn global(n: usize) -> Result<Scenario> {
    ensure!((6..=1000).contains(&n), "global size must be 6..1000");
    let regions: Vec<_> = [
        ("California", 33., -117.),
        ("Hawaii", 21., -158.),
        ("Japan", 35., 140.),
        ("Australia", -33., 151.),
        ("Kenya", -1., 37.),
        ("Portugal", 39., -9.),
    ]
    .into_iter()
    .map(|(name, latitude, longitude)| Region {
        name: name.into(),
        latitude,
        longitude,
    })
    .collect();
    let mut contacts = vec![];
    for slot in 0..192 {
        for from in 0..n {
            let region = from % 6;
            let mut add = |to, group, channel, primary| {
                contacts.push(Contact {
                    slot,
                    from,
                    to,
                    group,
                    channel,
                    primary,
                    bytes: 768,
                })
            };
            if from >= 6 {
                if slot % 16 == (from / 6 % 16) as u32 {
                    add(region, region, 0, true);
                    add(region, region, 1, false);
                }
                continue;
            }
            if slot % 3 != from as u32 % 3 {
                continue;
            }
            for to in [(from + 1) % 6, (from + 5) % 6] {
                let hour =
                    |r: usize| (slot as f64 / 4. + regions[r].longitude / 15.).rem_euclid(24.);
                let daylight = |r| (6.0..18.).contains(&hour(r));
                let band = if daylight(from) && daylight(to) { 2 } else { 1 };
                add(to, from.min(to) + 6, band, true);
                add(to, from.min(to) + 6, 3, false);
            }
        }
    }
    Ok(Scenario {
        nodes: n,
        regions,
        slots: 192,
        queue_limit: 32,
        outage_percent: 20,
        contacts,
        offers: (0..n)
            .map(|source| Offer {
                source,
                destination: (source + 3) % 6,
                start: (source % 31) as u32,
                deadline: 192,
                size: 128,
                weight: 1 + (source % 3) as u32,
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn waiting_for_a_contact_beats_shortest_hop_forwarding() -> Result<()> {
        for adaptive in [false, true] {
            let row = run(
                &six(),
                Policy {
                    adapt_routes: adaptive,
                    adapt_contacts: true,
                },
                1,
                true,
            )?;
            assert_eq!(row.delivered, usize::from(adaptive));
            assert_eq!(row.expired, usize::from(!adaptive));
            if adaptive {
                assert_eq!(row.trace, vec![(5, 0, 0, 3), (7, 0, 3, 4), (10, 0, 4, 5)]);
            }
        }
        Ok(())
    }
}
