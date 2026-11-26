use super::{Error, Result};
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

const MAX_INPUT: u64 = 2 * 1024 * 1024;
const MAX_CHANNELS: usize = 4096;
const FORECAST_MODEL: &str = "VOACAP";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    pub version: u8,
    pub inventory_path: PathBuf,
    pub forecast_path: Option<PathBuf>,
    pub journal_dir: PathBuf,
    pub refresh_seconds: u32,
    pub inventory_max_age_seconds: u32,
    pub report_max_age_seconds: u32,
    pub service_codes: BTreeSet<String>,
    pub dial_ranges: Vec<DialRange>,
    pub shadow: Option<ShadowConfig>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DialRange {
    min_hz: u64,
    max_hz: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ShadowConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scenario_path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub virtual_bw500: Option<VirtualBenchConfig>,
    pub prepare_seconds: u32,
    pub grant_seconds: u32,
    pub session_seconds: u32,
    pub cooldown_seconds: u32,
    pub switch_cost_bp: u16,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct VirtualBenchConfig {
    pub modem_path: PathBuf,
    pub waveform_profile: PathBuf,
    pub local_call: String,
}

impl Config {
    pub(super) fn load(path: &Path) -> Result<Self> {
        let path = path.canonicalize()?;
        let mut config: Self = read_json(&path)?;
        if config.version != 1
            || !(1..=3600).contains(&config.refresh_seconds)
            || !(config.refresh_seconds..=604_800).contains(&config.inventory_max_age_seconds)
            || !(1..=604_800).contains(&config.report_max_age_seconds)
            || config.service_codes.is_empty()
            || config.service_codes.iter().any(|s| !identifier(s, 16))
            || config.dial_ranges.is_empty()
            || config.dial_ranges.iter().any(|range| {
                !hf_frequency(range.min_hz)
                    || !hf_frequency(range.max_hz)
                    || range.min_hz > range.max_hz
            })
        {
            return Err(Error::Invalid("invalid station inventory policy"));
        }
        let directory = path.parent().unwrap_or_else(|| Path::new("."));
        config.inventory_path = directory.join(&config.inventory_path);
        config.journal_dir = directory.join(&config.journal_dir);
        if let Some(forecast) = &mut config.forecast_path {
            *forecast = directory.join(&*forecast);
        }
        if let Some(shadow) = &mut config.shadow {
            if !(1..=120).contains(&shadow.prepare_seconds)
                || !(1..=60).contains(&shadow.grant_seconds)
                || !(1..=300).contains(&shadow.session_seconds)
                || !(1..=3600).contains(&shadow.cooldown_seconds)
                || shadow.switch_cost_bp > 10_000
            {
                return Err(Error::Invalid("invalid shadow session policy"));
            }
            match (&mut shadow.scenario_path, &mut shadow.virtual_bw500) {
                (Some(path), None) => *path = directory.join(&*path),
                (None, Some(bench)) => {
                    bench.modem_path = directory.join(&bench.modem_path);
                    bench.waveform_profile = directory.join(&bench.waveform_profile);
                }
                _ => return Err(Error::Invalid("select one shadow execution backend")),
            }
        }
        Ok(config)
    }

    pub(super) fn allowed_frequency(&self, hz: u64) -> bool {
        self.dial_ranges
            .iter()
            .any(|range| (range.min_hz..=range.max_hz).contains(&hz))
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ImportedInventory {
    version: u8,
    source: String,
    as_of: DateTime<Utc>,
    complete: bool,
    channels: Vec<Channel>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Channel {
    pub gateway: String,
    pub protocol: String,
    pub service_code: String,
    pub dial_hz: Option<u64>,
    pub center_hz: Option<u64>,
    pub bandwidth_hz: Option<u16>,
    pub reported_at: Option<DateTime<Utc>>,
}

impl Channel {
    pub(super) fn id(&self) -> String {
        format!(
            "{}/{}/{}/{}/{}/{}",
            self.gateway,
            self.protocol,
            self.service_code,
            self.dial_hz.unwrap_or(0),
            self.center_hz.unwrap_or(0),
            self.bandwidth_hz.unwrap_or(0)
        )
    }

    fn validate(&self, as_of: DateTime<Utc>) -> Result<()> {
        if !identifier(&self.gateway, 16)
            || !identifier(&self.protocol, 24)
            || !identifier(&self.service_code, 16)
            || (self.dial_hz.is_none() && self.center_hz.is_none())
            || self.dial_hz.is_some_and(|hz| !hf_frequency(hz))
            || self.center_hz.is_some_and(|hz| !hf_frequency(hz))
            || self
                .bandwidth_hz
                .is_some_and(|bw| !(1..=10_000).contains(&bw))
            || self.reported_at.is_some_and(|time| time > as_of)
        {
            return Err(Error::Invalid("invalid channel or source report time"));
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Snapshot {
    pub id: String,
    pub retrieved_at: DateTime<Utc>,
    data: ImportedInventory,
}

impl Snapshot {
    pub fn load(config: &Config, now: DateTime<Utc>) -> Result<Self> {
        let mut data: ImportedInventory = read_json(&config.inventory_path)?;
        if data.version != 1
            || data.source.trim().is_empty()
            || data.source.len() > 240
            || data.as_of > now
            || data.channels.len() > MAX_CHANNELS
        {
            return Err(Error::Invalid(
                "invalid inventory source, time, version or size",
            ));
        }
        let mut channels = BTreeMap::new();
        for channel in data.channels {
            channel.validate(data.as_of)?;
            if !config.service_codes.contains(&channel.service_code) {
                continue;
            }
            let id = channel.id();
            if let Some(old) = channels.insert(id, channel.clone())
                && old != channel
            {
                return Err(Error::Invalid(
                    "conflicting records for one gateway channel",
                ));
            }
        }
        data.channels = channels.into_values().collect();
        // Visible snapshots are scoped to the profile before storage and hashing.
        let id = fingerprint(&data)?;
        Ok(Self {
            id,
            retrieved_at: now,
            data,
        })
    }

    pub fn verify_fingerprint(&self) -> Result<()> {
        if fingerprint(&self.data)? != self.id {
            return Err(Error::Invalid("inventory snapshot fingerprint mismatch"));
        }
        Ok(())
    }

    pub fn as_of(&self) -> DateTime<Utc> {
        self.data.as_of
    }
    pub fn complete(&self) -> bool {
        self.data.complete
    }
    pub fn channels(&self) -> &[Channel] {
        &self.data.channels
    }
    pub fn stale(&self, config: &Config, now: DateTime<Utc>) -> bool {
        let age = now - self.data.as_of;
        age < TimeDelta::zero() || age > TimeDelta::seconds(config.inventory_max_age_seconds.into())
    }

    pub fn summary(
        &self,
        config: &Config,
        now: DateTime<Utc>,
        refresh_error: Option<String>,
        supports: impl Fn(&Channel) -> bool,
    ) -> InventorySummary {
        let mut stations = BTreeSet::new();
        let mut fresh_stations = BTreeSet::new();
        let mut eligible_channels = 0;
        for channel in &self.data.channels {
            stations.insert(channel.gateway.as_str());
            if fresh_report(channel, config, now) {
                fresh_stations.insert(channel.gateway.as_str());
            }
            if supports(channel)
                && exclusions(channel, self, config, now, refresh_error.is_some()).is_empty()
            {
                eligible_channels += 1;
            }
        }
        InventorySummary {
            snapshot_id: Some(self.id.clone()),
            source: Some(self.data.source.clone()),
            source_as_of: Some(self.as_of()),
            retrieved_at: Some(self.retrieved_at),
            known_gateways: Some(stations.len()),
            known_channels: Some(self.data.channels.len()),
            freshly_reported_gateways: Some(fresh_stations.len()),
            eligible_channels: Some(eligible_channels),
            stale: self.stale(config, now) || refresh_error.is_some(),
            complete: self.complete(),
            refresh_error,
            evaluated_at: now,
        }
    }
}

#[derive(Serialize)]
pub struct InventorySummary {
    pub snapshot_id: Option<String>,
    pub source: Option<String>,
    pub source_as_of: Option<DateTime<Utc>>,
    pub retrieved_at: Option<DateTime<Utc>>,
    pub known_gateways: Option<usize>,
    pub known_channels: Option<usize>,
    pub freshly_reported_gateways: Option<usize>,
    pub eligible_channels: Option<usize>,
    pub stale: bool,
    pub complete: bool,
    pub refresh_error: Option<String>,
    pub evaluated_at: DateTime<Utc>,
}

pub(super) fn fresh_report(channel: &Channel, config: &Config, now: DateTime<Utc>) -> bool {
    channel.reported_at.is_some_and(|time| {
        time <= now && now - time <= TimeDelta::seconds(config.report_max_age_seconds.into())
    })
}

pub(super) fn exclusions(
    channel: &Channel,
    snapshot: &Snapshot,
    config: &Config,
    now: DateTime<Utc>,
    refresh_failed: bool,
) -> Vec<&'static str> {
    let mut reasons = Vec::new();
    if snapshot.stale(config, now) || refresh_failed {
        reasons.push("inventory_stale");
    }
    if !snapshot.complete() {
        reasons.push("inventory_partial");
    }
    if !fresh_report(channel, config, now) {
        reasons.push("report_unknown_or_stale");
    }
    if channel.protocol != "VARA-HF" || channel.bandwidth_hz != Some(500) {
        reasons.push("profile_requires_vara_hf_bw500");
    }
    match channel.dial_hz {
        None => reasons.push("dial_frequency_unknown"),
        Some(hz) if !config.allowed_frequency(hz) => reasons.push("dial_frequency_outside_profile"),
        Some(_) => {}
    }
    reasons
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Forecast {
    pub channel_id: String,
    pub model: String,
    pub model_revision: String,
    pub inputs_id: String,
    pub issued_at: DateTime<Utc>,
    pub valid_from: DateTime<Utc>,
    pub valid_until: DateTime<Utc>,
    pub reliability_bp: u16,
}

impl Forecast {
    fn validate(&self, now: DateTime<Utc>) -> bool {
        self.model == FORECAST_MODEL
            && !self.model_revision.trim().is_empty()
            && self.model_revision.len() <= 100
            && !self.inputs_id.trim().is_empty()
            && self.inputs_id.len() <= 200
            && self.reliability_bp <= 10_000
            && self.channel_id.len() <= 128
            && self.issued_at <= now
            && self.valid_from < self.valid_until
            && self.issued_at < self.valid_until
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ForecastFile {
    version: u8,
    forecasts: Vec<Forecast>,
}

pub(super) fn forecasts(path: &Path, now: DateTime<Utc>) -> Result<BTreeMap<String, Forecast>> {
    let file: ForecastFile = read_json(path)?;
    if file.version != 1 || file.forecasts.len() > MAX_CHANNELS {
        return Err(Error::Invalid("invalid forecast file version or size"));
    }
    let mut active = BTreeMap::<String, Forecast>::new();
    for forecast in file.forecasts {
        if !forecast.validate(now) {
            return Err(Error::Invalid("invalid forecast"));
        }
        if !(forecast.valid_from <= now && now < forecast.valid_until) {
            continue;
        }
        match active.entry(forecast.channel_id.clone()) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(forecast);
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                if entry.get().issued_at == forecast.issued_at {
                    return Err(Error::Invalid("ambiguous forecast issue time"));
                }
                if entry.get().issued_at < forecast.issued_at {
                    entry.insert(forecast);
                }
            }
        }
    }
    Ok(active)
}

pub(super) fn read_json<T: DeserializeOwned>(path: &Path) -> Result<T> {
    if !std::fs::metadata(path)?.is_file() {
        return Err(Error::Invalid("expected a regular input file"));
    }
    let mut bytes = Vec::new();
    File::open(path)?
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(Error::Invalid("input exceeds 2 MiB"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

pub(super) fn fingerprint(value: &impl Serialize) -> Result<String> {
    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.update(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(Sha256::new());
    serde_json::to_writer(&mut writer, value)?;
    Ok(format!("{:x}", writer.0.finalize()))
}

fn hf_frequency(hz: u64) -> bool {
    (1_800_000..=30_000_000).contains(&hz)
}

fn identifier(text: &str, maximum: usize) -> bool {
    !text.is_empty()
        && text.len() <= maximum
        && text
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'-')
}
