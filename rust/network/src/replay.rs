//! Compact deterministic event export. Contacts index the shared scenario schedule.
use crate::sim::{self, Policy, Scenario};
use anyhow::{Result, ensure};
use serde_json::{Value, json};

pub fn export(s: &Scenario, name: &str, seed: u64, forecast: Option<&Value>) -> Result<Value> {
    ensure!(
        name == "six" || (name.starts_with("global-") && s.regions.len() == 6),
        "geographic replays support six or global-N"
    );
    let six = [
        ("Lisbon", 38.72, -9.14),
        ("Dublin", 53.35, -6.26),
        ("Reykjavík", 64.15, -21.94),
        ("Madeira", 32.65, -16.91),
        ("Azores", 37.74, -25.67),
        ("Newfoundland", 47.56, -52.71),
    ];
    let stations: Vec<_> = (0..s.nodes)
        .map(|i| {
            let (label, lat, lon) = if name == "six" {
                six[i]
            } else {
                let r = &s.regions[i % 6];
                let index = (i / 6) as f64;
                let radius = index.sqrt() * 0.35;
                (
                    r.name.as_str(),
                    r.latitude + radius * (index * 2.4).cos(),
                    r.longitude + radius * (index * 2.4).sin(),
                )
            };
            json!({"id":i,"name":label,"lat":lat,"lon":lon})
        })
        .collect();
    let mut runs = vec![];
    let mut blocked = Vec::new();
    if let Some(table) = forecast {
        let family = if name == "six" { "six" } else { "global" };
        for (i, c) in s.contacts.iter().enumerate() {
            // Regional access links have no commissioned hourly forecast.
            if c.from >= 6 || c.to >= 6 {
                continue;
            }
            let hour = if name == "six" {
                (5 + c.slot) / 60
            } else {
                (c.slot / 4) % 24
            };
            let key = format!("{family}/{}/{}/{hour}/{}", c.from, c.to, c.channel);
            let value = table[&key]["REL"]
                .as_f64()
                .ok_or_else(|| anyhow::anyhow!("missing forecast {key}"))?;
            ensure!((0.0..=1.0).contains(&value), "invalid reliability");
            if value < 0.90 {
                blocked.push(i);
            }
        }
    }
    for gated in [false, true] {
        if gated && forecast.is_none() {
            continue;
        }
        let ids: Vec<_> = (0..s.contacts.len())
            .filter(|i| !gated || blocked.binary_search(i).is_err())
            .collect();
        let mut schedule = s.clone();
        schedule.contacts = ids.iter().map(|i| s.contacts[*i].clone()).collect();
        runs.extend(policy_runs(&schedule, seed, &ids, gated)?);
    }
    Ok(
        json!({"version":1,"scenario":name,"seed":seed,"stations":stations,"utc":if name=="six" {"2026-10-20T00:05:00Z"} else {"2026-10-20T00:00:00Z"},
        "seconds_per_slot":if name=="six"{60}else{900},"frequency_hz":[7100000,5400000,14100000,10120000],
        "slots":s.slots,"contacts":s.contacts.iter().map(|c|[c.slot,c.from as u32,c.to as u32,c.channel as u32]).collect::<Vec<_>>(),
        "blocked_contacts":blocked,"offers":s.offers,"runs":runs}),
    )
}

pub fn policy_runs(
    schedule: &Scenario,
    seed: u64,
    ids: &[usize],
    gated: bool,
) -> Result<Vec<Value>> {
    let mut runs = Vec::new();
    for adaptive in [false, true] {
        let policy = Policy {
            adapt_routes: adaptive,
            adapt_contacts: adaptive,
        };
        let mut events = vec![];
        let result = sim::observe(
            schedule,
            policy,
            seed,
            false,
            |slot, id, contact, station, outcome| {
                events.push(json!([slot, id, contact.map(|i| ids[i]), station, outcome]));
            },
        )?;
        runs.push(json!({"policy":policy,"forecast_gate":gated,"events":events,"summary":result}));
    }
    Ok(runs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_keeps_simulator_results_and_original_contact_indices() -> Result<()> {
        let scenario = sim::six();
        let mut forecasts = json!({});
        for contact in &scenario.contacts {
            let key = format!("six/{}/{}/0/0", contact.from, contact.to);
            forecasts[key] = json!({"REL": if contact.to == 1 {0.1} else {1.0}});
        }
        let data = export(&scenario, "six", 17, Some(&forecasts))?;
        assert_eq!(data["blocked_contacts"], json!([0]));
        for (i, adaptive) in [false, true].into_iter().enumerate() {
            let policy = Policy {
                adapt_contacts: adaptive,
                adapt_routes: adaptive,
            };
            let baseline = sim::run(&scenario, policy, 17, false)?;
            assert_eq!(data["runs"][i]["summary"], serde_json::to_value(baseline)?);
        }
        assert_eq!(
            data["runs"][3]["events"],
            json!([
                [4, 0, 3, 0, "transfer"],
                [6, 0, 4, 3, "transfer"],
                [9, 0, 5, 4, "transfer"]
            ])
        );
        Ok(())
    }
}
