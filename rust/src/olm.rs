//! Local Winlink inventory and non-transmitting station coordination.

mod inventory;
mod journal;

use crate::radio::{
    ControlValue, PttAction, PttRequest, RadioControlPort, RadioError, SimulatedRadio,
};
use chrono::{DateTime, TimeDelta, Utc};
use inventory::{Config, Snapshot, exclusions, fingerprint, forecasts, read_json};
use journal::{Journal, Record};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    io,
    path::Path,
    sync::Arc,
    time::Duration,
};
use tokio::{
    sync::{Mutex, oneshot},
    task::JoinHandle,
    time::Instant,
};
use uuid::Uuid;

pub use inventory::{Channel, Forecast, InventorySummary};

const RANKER: &str = "voacap-prior-v1";
const PAGE_LIMIT: usize = 100;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Denied(&'static str),
    #[error("{0}")]
    Busy(&'static str),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Radio(#[from] RadioError),
}

type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Deserialize, Serialize)]
pub struct Candidate {
    pub channel_id: String,
    pub channel: Channel,
    pub rank: Option<usize>,
    pub exclusions: Vec<String>,
    pub forecast: Option<Forecast>,
    pub switching_penalty_bp: u16,
    pub utility_bp: Option<i32>,
}

#[derive(Clone, Deserialize, Serialize)]
struct Ranking {
    id: Uuid,
    evaluated_at: DateTime<Utc>,
    snapshot_id: Option<String>,
    method: String,
    decision: Decision,
    forecast_warning: Option<String>,
    entries: Vec<Candidate>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Decision {
    Select { channel_id: String },
    Wait { reason: String },
    NoRoute,
}

#[derive(Serialize)]
pub struct RankingPage {
    ranking_id: Uuid,
    profile_id: String,
    evaluated_at: DateTime<Utc>,
    snapshot_id: Option<String>,
    method: String,
    decision: Decision,
    forecast_warning: Option<String>,
    total: usize,
    offset: usize,
    entries: Vec<Candidate>,
}

#[derive(Clone, Serialize)]
pub struct Preparation {
    pub prepared_id: Uuid,
    pub channel_id: String,
    pub state_revision: u64,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Serialize)]
pub struct Authorization {
    pub grant_id: Uuid,
    pub prepared_id: Uuid,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Active,
    Completed,
    Failed,
    TimedOut,
    Aborted,
    Interrupted,
    ReleaseUnconfirmed,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct Attempt {
    pub attempt_id: Uuid,
    pub ranking_id: Uuid,
    pub channel_id: String,
    pub gateway: String,
    pub inventory_snapshot_id: String,
    pub state_revision: u64,
    pub started_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub outcome: Outcome,
    pub simulated: bool,
    pub scenario_id: String,
    pub modem_connected: Option<bool>,
    pub client_transfer: Option<bool>,
    pub cms_accepted: Option<bool>,
    pub recipient_delivered: Option<bool>,
    pub release_confirmed: Option<bool>,
}

#[derive(Serialize)]
pub struct Metrics {
    pub from: DateTime<Utc>,
    pub to: DateTime<Utc>,
    pub attempted: usize,
    pub modem_connected: usize,
    pub modem_outcome_known: usize,
    pub client_transfer_confirmed: usize,
    pub client_outcome_known: usize,
    pub simulated: bool,
}

#[derive(Serialize)]
pub struct GatewayChannels {
    pub inventory: InventorySummary,
    pub gateway: String,
    pub total: usize,
    pub offset: usize,
    pub channels: Vec<Channel>,
}

#[derive(Serialize)]
pub struct StationStatus {
    pub profile_id: String,
    pub shadow: bool,
    pub inhibited: bool,
    pub prepared: Option<Preparation>,
    pub active_attempt_id: Option<Uuid>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Scenario {
    version: u8,
    channels: BTreeMap<String, ScenarioResult>,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ScenarioResult {
    duration_ms: u32,
    modem_connected: bool,
    client_transfer: bool,
    release_confirmed: bool,
}

impl Scenario {
    fn load(path: &Path) -> Result<Self> {
        let scenario: Self = read_json(path)?;
        if scenario.version != 1
            || scenario.channels.values().any(|result| {
                !(1..=600_000).contains(&result.duration_ms)
                    || (result.client_transfer && !result.modem_connected)
            })
        {
            return Err(Error::Invalid("invalid shadow scenario version or outcome"));
        }
        Ok(scenario)
    }
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
enum Event {
    Inventory(Snapshot),
    Collection {
        snapshot_id: Option<String>,
        error: Option<String>,
    },
    Ranking(Ranking),
    Preparing {
        id: Uuid,
        ranking_id: Uuid,
        channel_id: String,
    },
    Prepared {
        id: Uuid,
        ranking_id: Uuid,
        channel_id: String,
        state_revision: u64,
    },
    Authorized {
        id: Uuid,
        prepared_id: Uuid,
    },
    Started {
        prepared_id: Uuid,
        grant_id: Uuid,
        attempt: Attempt,
    },
    Attempt(Attempt),
    ReleasedOnRestart,
}

struct Prepared {
    view: Preparation,
    ranking_id: Uuid,
    snapshot_id: String,
    dial_hz: u64,
    deadline: Instant,
    grant: Option<(Authorization, Instant)>,
}

struct Active {
    id: Uuid,
    _cancel: oneshot::Sender<()>,
}

#[derive(Default)]
struct State {
    inventory: Option<Snapshot>,
    refresh_error: Option<String>,
    rankings: BTreeMap<Uuid, u64>,
    attempts: BTreeMap<Uuid, Attempt>,
    starts: BTreeMap<Uuid, (Uuid, Uuid)>,
    prepared: Option<Prepared>,
    active: Option<Active>,
    inhibited: bool,
}

struct Local {
    state: State,
    journal: Journal,
}

pub struct Olm {
    config: Config,
    profile_id: String,
    shadow: Option<Shadow>,
    local: Mutex<Local>,
}

struct Shadow {
    radio: Arc<SimulatedRadio>,
    scenario: Scenario,
    scenario_id: String,
}

impl Olm {
    pub fn open(path: &Path, simulated: Option<Arc<SimulatedRadio>>) -> Result<Arc<Self>> {
        let config = Config::load(path)?;
        let profile_id = fingerprint(&config)?;
        let shadow = config
            .shadow
            .as_ref()
            .map(|cfg| {
                let radio = simulated.ok_or(Error::Denied(
                    "shadow execution requires the simulated radio backend",
                ))?;
                let scenario = Scenario::load(&cfg.scenario_path)?;
                let scenario_id = fingerprint(&scenario)?;
                Ok::<_, Error>(Shadow {
                    radio,
                    scenario,
                    scenario_id,
                })
            })
            .transpose()?;
        let mut state = State::default();
        let mut journal = Journal::open(&config.journal_dir, |sequence, record: Record<Event>| {
            if record.profile_id != profile_id {
                return Ok(());
            }
            match record.event {
                Event::Inventory(snapshot) => {
                    snapshot.verify_fingerprint()?;
                    state.inventory = Some(snapshot);
                    state.refresh_error = None;
                }
                Event::Collection { error, .. } => state.refresh_error = error,
                Event::Ranking(ranking) => {
                    state.rankings.insert(ranking.id, sequence);
                }
                Event::Started {
                    prepared_id,
                    grant_id,
                    attempt,
                } => {
                    state
                        .starts
                        .insert(grant_id, (prepared_id, attempt.attempt_id));
                    state.attempts.insert(attempt.attempt_id, attempt);
                }
                Event::Attempt(attempt) => {
                    state.inhibited |= attempt.outcome == Outcome::ReleaseUnconfirmed;
                    state.attempts.insert(attempt.attempt_id, attempt);
                }
                Event::ReleasedOnRestart => state.inhibited = false,
                _ => {}
            }
            Ok(())
        })?;
        let now = Utc::now();
        for attempt in state
            .attempts
            .values_mut()
            .filter(|attempt| attempt.outcome == Outcome::Active)
        {
            attempt.outcome = Outcome::Interrupted;
            attempt.ended_at = Some(now);
            journal.append(&profile_id, now, Event::Attempt(attempt.clone()))?;
        }
        // Restart builds a fresh, no-device radio. Live execution must reconcile real hardware here.
        if state.inhibited && shadow.is_some() {
            journal.append(&profile_id, now, Event::ReleasedOnRestart)?;
            state.inhibited = false;
        }
        Ok(Arc::new(Self {
            config,
            profile_id,
            shadow,
            local: Mutex::new(Local { state, journal }),
        }))
    }

    pub fn is_shadow(&self) -> bool {
        self.shadow.is_some()
    }

    pub fn start_refresh(self: &Arc<Self>) -> JoinHandle<()> {
        let service = self.clone();
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(Duration::from_secs(service.config.refresh_seconds.into()));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let _ = service.refresh().await;
            }
        })
    }

    pub async fn refresh(&self) -> Result<InventorySummary> {
        let mut local = self.local.lock().await;
        self.refresh_locked(&mut local, Utc::now())?;
        Ok(self.summary(&local.state, Utc::now()))
    }

    fn refresh_locked(&self, local: &mut Local, now: DateTime<Utc>) -> Result<()> {
        let snapshot = Snapshot::load(&self.config, now).and_then(|snapshot| {
            if !snapshot.complete()
                && local
                    .state
                    .inventory
                    .as_ref()
                    .is_some_and(Snapshot::complete)
            {
                return Err(Error::Invalid("source returned a partial inventory"));
            }
            if local
                .state
                .inventory
                .as_ref()
                .is_some_and(|old| snapshot.as_of() < old.as_of())
            {
                return Err(Error::Invalid("source inventory moved backwards in time"));
            }
            Ok(snapshot)
        });
        let result = self.store_refresh(local, snapshot, now);
        if let Err(error) = &result {
            local.state.inhibited = true;
            local.state.refresh_error = Some(format!("inventory journal failed: {error}"));
        }
        result
    }

    fn store_refresh(
        &self,
        local: &mut Local,
        snapshot: Result<Snapshot>,
        now: DateTime<Utc>,
    ) -> Result<()> {
        match snapshot {
            Ok(snapshot) => {
                if local
                    .state
                    .inventory
                    .as_ref()
                    .is_none_or(|old| old.id != snapshot.id)
                {
                    local.journal.append(
                        &self.profile_id,
                        now,
                        Event::Inventory(snapshot.clone()),
                    )?;
                    local.state.inventory = Some(snapshot);
                } else if local.state.refresh_error.is_some() {
                    local.journal.append(
                        &self.profile_id,
                        now,
                        Event::Collection {
                            snapshot_id: Some(snapshot.id),
                            error: None,
                        },
                    )?;
                }
                local.state.refresh_error = None;
            }
            Err(error) => {
                let status = error.to_string();
                let previous = local.state.refresh_error.replace(status.clone());
                if previous.as_ref() != Some(&status) {
                    local.journal.append(
                        &self.profile_id,
                        now,
                        Event::Collection {
                            snapshot_id: local.state.inventory.as_ref().map(|s| s.id.clone()),
                            error: Some(status),
                        },
                    )?;
                }
            }
        }
        Ok(())
    }

    pub async fn inventory(&self) -> InventorySummary {
        self.summary(&self.local.lock().await.state, Utc::now())
    }

    pub async fn gateway_channels(
        &self,
        gateway: &str,
        offset: usize,
        limit: usize,
    ) -> Result<GatewayChannels> {
        if gateway.is_empty()
            || gateway.len() > 16
            || !gateway
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return Err(Error::Invalid("invalid gateway callsign"));
        }
        let gateway = gateway.to_ascii_uppercase();
        let local = self.local.lock().await;
        let matching = local
            .state
            .inventory
            .as_ref()
            .into_iter()
            .flat_map(Snapshot::channels)
            .filter(|channel| channel.gateway == gateway);
        Ok(GatewayChannels {
            inventory: self.summary(&local.state, Utc::now()),
            total: matching.clone().count(),
            offset,
            channels: matching
                .skip(offset)
                .take(limit.clamp(1, PAGE_LIMIT))
                .cloned()
                .collect(),
            gateway,
        })
    }

    pub async fn status(&self) -> StationStatus {
        let local = self.local.lock().await;
        let state = &local.state;
        StationStatus {
            profile_id: self.profile_id.clone(),
            shadow: self.is_shadow(),
            inhibited: state.inhibited,
            prepared: state
                .prepared
                .as_ref()
                .filter(|p| Instant::now() < p.deadline)
                .map(|p| p.view.clone()),
            active_attempt_id: state.active.as_ref().map(|a| a.id),
        }
    }

    fn summary(&self, state: &State, now: DateTime<Utc>) -> InventorySummary {
        state.inventory.as_ref().map_or_else(
            || InventorySummary {
                snapshot_id: None,
                source: None,
                source_as_of: None,
                retrieved_at: None,
                known_gateways: None,
                known_channels: None,
                freshly_reported_gateways: None,
                eligible_channels: None,
                stale: true,
                complete: false,
                refresh_error: state.refresh_error.clone(),
                evaluated_at: now,
            },
            |snapshot| snapshot.summary(&self.config, now, state.refresh_error.clone()),
        )
    }

    pub async fn rank(
        &self,
        ranking_id: Option<Uuid>,
        offset: usize,
        limit: usize,
    ) -> Result<RankingPage> {
        let mut local = self.local.lock().await;
        if let Some(id) = ranking_id {
            return Ok(self.page(&self.load_ranking(&local, id)?, offset, limit));
        }
        let now = Utc::now();
        self.refresh_locked(&mut local, now)?;
        let ranking = self.build_ranking(&local.state, now);
        let result = self.page(&ranking, offset, limit);
        let sequence = local
            .journal
            .append(&self.profile_id, now, Event::Ranking(ranking))?;
        local.state.rankings.insert(result.ranking_id, sequence);
        Ok(result)
    }

    fn load_ranking(&self, local: &Local, id: Uuid) -> Result<Ranking> {
        let sequence = local
            .state
            .rankings
            .get(&id)
            .ok_or(Error::Invalid("unknown ranking ID"))?;
        let record: Record<Event> = local.journal.load(*sequence)?;
        match record.event {
            Event::Ranking(ranking) if ranking.id == id && record.profile_id == self.profile_id => {
                Ok(ranking)
            }
            _ => Err(Error::Invalid("invalid station ranking record")),
        }
    }

    fn page(&self, r: &Ranking, offset: usize, limit: usize) -> RankingPage {
        RankingPage {
            ranking_id: r.id,
            profile_id: self.profile_id.clone(),
            evaluated_at: r.evaluated_at,
            snapshot_id: r.snapshot_id.clone(),
            method: r.method.clone(),
            decision: r.decision.clone(),
            forecast_warning: r.forecast_warning.clone(),
            total: r.entries.len(),
            offset,
            entries: r
                .entries
                .iter()
                .skip(offset)
                .take(limit.clamp(1, PAGE_LIMIT))
                .cloned()
                .collect(),
        }
    }

    fn build_ranking(&self, state: &State, now: DateTime<Utc>) -> Ranking {
        let mut ranking = Ranking {
            id: Uuid::new_v4(),
            evaluated_at: now,
            snapshot_id: state.inventory.as_ref().map(|s| s.id.clone()),
            method: RANKER.into(),
            decision: Decision::Wait {
                reason: "inventory_unavailable".into(),
            },
            forecast_warning: None,
            entries: Vec::new(),
        };
        let Some(snapshot) = &state.inventory else {
            return ranking;
        };
        let (mut forecasts, forecast_warning) = match &self.config.forecast_path {
            Some(path) => match forecasts(path, now) {
                Ok(forecasts) => (forecasts, None),
                Err(error) => (BTreeMap::new(), Some(error.to_string())),
            },
            None => (BTreeMap::new(), Some("forecast_source_unconfigured".into())),
        };
        ranking.forecast_warning = forecast_warning;
        let (mut cooled, mut last_success) = (BTreeSet::new(), None);
        if let Some(shadow) = &self.config.shadow {
            for attempt in state.attempts.values() {
                let Some(ended) = attempt.ended_at else {
                    continue;
                };
                if attempt.outcome == Outcome::Completed {
                    if last_success.is_none_or(|last: &Attempt| last.ended_at < attempt.ended_at) {
                        last_success = Some(attempt);
                    }
                } else if cooling(ended, now, shadow.cooldown_seconds) {
                    cooled.insert(&attempt.channel_id);
                }
            }
        }
        for channel in snapshot.channels() {
            let id = channel.id();
            let mut reasons = exclusions(
                channel,
                snapshot,
                &self.config,
                now,
                state.refresh_error.is_some(),
            );
            if cooled.contains(&id) {
                reasons.push("previous_attempt_in_cooldown");
            }
            let forecast = forecasts.remove(&id);
            let penalty = self
                .config
                .shadow
                .as_ref()
                .filter(|_| {
                    forecast.is_some() && last_success.is_some_and(|last| last.channel_id != id)
                })
                .map_or(0, |shadow| shadow.switch_cost_bp);
            ranking.entries.push(Candidate {
                channel_id: id,
                channel: channel.clone(),
                rank: None,
                exclusions: reasons.into_iter().map(str::to_owned).collect(),
                utility_bp: forecast
                    .as_ref()
                    .map(|f| i32::from(f.reliability_bp) - i32::from(penalty)),
                forecast,
                switching_penalty_bp: penalty,
            });
        }
        ranking.entries.sort_by(|a, b| {
            b.exclusions
                .is_empty()
                .cmp(&a.exclusions.is_empty())
                .then(b.utility_bp.cmp(&a.utility_bp))
                .then(b.channel.reported_at.cmp(&a.channel.reported_at))
                .then(a.channel_id.cmp(&b.channel_id))
        });
        for (index, entry) in ranking
            .entries
            .iter_mut()
            .take_while(|entry| entry.exclusions.is_empty())
            .enumerate()
        {
            entry.rank = Some(index + 1);
        }
        ranking.decision = if let Some(first) = ranking.entries.first().filter(|e| e.rank.is_some())
        {
            Decision::Select {
                channel_id: first.channel_id.clone(),
            }
        } else if snapshot.stale(&self.config, now)
            || !snapshot.complete()
            || state.refresh_error.is_some()
        {
            Decision::Wait {
                reason: "inventory_stale_or_partial".into(),
            }
        } else if ranking
            .entries
            .iter()
            .any(|entry| entry.exclusions == ["previous_attempt_in_cooldown"])
        {
            Decision::Wait {
                reason: "channel_cooldown".into(),
            }
        } else {
            Decision::NoRoute
        };
        ranking
    }

    pub async fn prepare(&self, ranking_id: Uuid, channel_id: &str) -> Result<Preparation> {
        let radio = &self
            .shadow
            .as_ref()
            .ok_or(Error::Denied(
                "station execution is available only in shadow mode",
            ))?
            .radio;
        let cfg = self
            .config
            .shadow
            .as_ref()
            .expect("configured shadow profile");
        let mut local = self.local.lock().await;
        let now = Utc::now();
        if local.state.active.is_some()
            || local
                .state
                .prepared
                .as_ref()
                .is_some_and(|p| Instant::now() < p.deadline)
        {
            return Err(Error::Busy("station lease is held"));
        }
        if local.state.inhibited {
            return Err(Error::Denied("station is inhibited"));
        }
        self.refresh_locked(&mut local, now)?;
        let ranking = self.load_ranking(&local, ranking_id)?;
        let candidate = ranking
            .entries
            .iter()
            .find(|entry| entry.channel_id == channel_id && entry.rank.is_some())
            .ok_or(Error::Denied("channel was ineligible in this ranking"))?;
        let snapshot_id = ranking
            .snapshot_id
            .as_deref()
            .ok_or(Error::Invalid("ranking has no inventory"))?;
        let channel = self.verify_candidate(&local.state, now, snapshot_id, channel_id)?;
        if candidate.channel != *channel {
            return Err(Error::Denied(
                "ranking channel differs from current inventory",
            ));
        }
        let dial_hz = channel
            .dial_hz
            .ok_or(Error::Invalid("missing dial frequency"))?;
        let snapshot_id = snapshot_id.to_owned();
        let before = radio.state().await?;
        if before.ptt_asserted != Some(false) {
            return Err(Error::Denied("receive state unconfirmed"));
        }
        // Shadow uses one USB profile with no RF power. Direct MCP writes are disabled in this mode.
        if before.mode.as_deref() != Some("USB") || before.rf_power_fraction != Some(0.0) {
            return Err(Error::Denied(
                "simulated radio is outside the shadow profile",
            ));
        }
        let id = Uuid::new_v4();
        local.journal.append(
            &self.profile_id,
            now,
            Event::Preparing {
                id,
                ranking_id,
                channel_id: channel_id.into(),
            },
        )?;
        let readback = radio
            .set_control(
                "frequency_hz",
                ControlValue::FrequencyHz(dial_hz),
                Some(before.state_revision),
            )
            .await?;
        if readback.frequency_hz != Some(dial_hz)
            || readback.mode.as_deref() != Some("USB")
            || readback.ptt_asserted != Some(false)
            || readback.rf_power_fraction != Some(0.0)
        {
            local.state.inhibited = true;
            return Err(Error::Denied("simulated configuration readback mismatch"));
        }
        let view = Preparation {
            prepared_id: id,
            channel_id: channel_id.into(),
            state_revision: readback.state_revision,
            expires_at: Utc::now() + TimeDelta::seconds(cfg.prepare_seconds.into()),
        };
        local.journal.append(
            &self.profile_id,
            now,
            Event::Prepared {
                id,
                ranking_id,
                channel_id: channel_id.into(),
                state_revision: readback.state_revision,
            },
        )?;
        local.state.prepared = Some(Prepared {
            view: view.clone(),
            ranking_id,
            snapshot_id,
            dial_hz,
            deadline: Instant::now() + Duration::from_secs(cfg.prepare_seconds.into()),
            grant: None,
        });
        Ok(view)
    }

    fn verify_candidate<'a>(
        &self,
        state: &'a State,
        now: DateTime<Utc>,
        snapshot_id: &str,
        channel_id: &str,
    ) -> Result<&'a Channel> {
        let snapshot = state
            .inventory
            .as_ref()
            .filter(|s| s.id == snapshot_id)
            .ok_or(Error::Denied(
                "current inventory differs from the prepared snapshot",
            ))?;
        let channel = snapshot
            .channels()
            .iter()
            .find(|c| c.id() == channel_id)
            .ok_or(Error::Denied("channel has disappeared from the inventory"))?;
        if !exclusions(
            channel,
            snapshot,
            &self.config,
            now,
            state.refresh_error.is_some(),
        )
        .is_empty()
        {
            return Err(Error::Denied(
                "candidate failed current freshness or eligibility checks",
            ));
        }
        if let Some(cfg) = &self.config.shadow
            && state.attempts.values().any(|a| {
                a.channel_id == channel_id
                    && a.outcome != Outcome::Completed
                    && a.ended_at
                        .is_some_and(|ended| cooling(ended, now, cfg.cooldown_seconds))
            })
        {
            return Err(Error::Denied("channel is in cooldown"));
        }
        Ok(channel)
    }

    async fn verify_prepared<'a>(
        &self,
        state: &'a State,
        now: DateTime<Utc>,
        prepared_id: Uuid,
    ) -> Result<&'a Prepared> {
        let radio = &self
            .shadow
            .as_ref()
            .ok_or(Error::Denied(
                "station execution is available only in shadow mode",
            ))?
            .radio;
        let prepared = state
            .prepared
            .as_ref()
            .filter(|p| p.view.prepared_id == prepared_id && Instant::now() < p.deadline)
            .ok_or(Error::Denied(
                "prepared handle is missing, consumed, or expired",
            ))?;
        if state.inhibited || state.active.is_some() {
            return Err(Error::Denied("station is busy or inhibited"));
        }
        self.verify_candidate(state, now, &prepared.snapshot_id, &prepared.view.channel_id)?;
        let readback = radio.state().await?;
        if readback.state_revision != prepared.view.state_revision
            || readback.frequency_hz != Some(prepared.dial_hz)
            || readback.mode.as_deref() != Some("USB")
            || readback.rf_power_fraction != Some(0.0)
            || readback.ptt_asserted != Some(false)
        {
            return Err(Error::Denied("prepared radio configuration changed"));
        }
        Ok(prepared)
    }

    pub async fn authorize(&self, prepared_id: Uuid) -> Result<Authorization> {
        let mut local = self.local.lock().await;
        let now = Utc::now();
        self.refresh_locked(&mut local, now)?;
        let prepared = self.verify_prepared(&local.state, now, prepared_id).await?;
        if let Some((grant, deadline)) = &prepared.grant {
            if Instant::now() < *deadline {
                return Ok(grant.clone());
            }
            return Err(Error::Denied(
                "authorization expired; prepare a fresh attempt",
            ));
        }
        let cfg = self
            .config
            .shadow
            .as_ref()
            .expect("configured shadow profile");
        let issued = Instant::now();
        let duration = Duration::from_secs(cfg.grant_seconds.into())
            .min(prepared.deadline.saturating_duration_since(issued));
        let grant = Authorization {
            grant_id: Uuid::new_v4(),
            prepared_id,
            expires_at: now
                + TimeDelta::from_std(duration).expect("bounded authorization lifetime"),
        };
        local.journal.append(
            &self.profile_id,
            now,
            Event::Authorized {
                id: grant.grant_id,
                prepared_id,
            },
        )?;
        local
            .state
            .prepared
            .as_mut()
            .expect("verified preparation")
            .grant = Some((grant.clone(), issued + duration));
        Ok(grant)
    }

    pub async fn start(self: &Arc<Self>, prepared_id: Uuid, grant_id: Uuid) -> Result<Attempt> {
        let mut local = self.local.lock().await;
        if let Some(&(previous_prepared, attempt_id)) = local.state.starts.get(&grant_id) {
            if prepared_id != previous_prepared {
                return Err(Error::Denied(
                    "authorization belongs to a different prepared handle",
                ));
            }
            return local
                .state
                .attempts
                .get(&attempt_id)
                .cloned()
                .ok_or(Error::Invalid("missing attempt for accepted authorization"));
        }
        let now = Utc::now();
        self.refresh_locked(&mut local, now)?;
        let prepared = self.verify_prepared(&local.state, now, prepared_id).await?;
        if !prepared.grant.as_ref().is_some_and(|(grant, deadline)| {
            grant.grant_id == grant_id && Instant::now() < *deadline
        }) {
            return Err(Error::Denied(
                "authorization is missing, mismatched, or expired",
            ));
        }
        let shadow = self.shadow.as_ref().expect("verified shadow configuration");
        let simulated = shadow
            .scenario
            .channels
            .get(&prepared.view.channel_id)
            .copied();
        let cfg = self
            .config
            .shadow
            .as_ref()
            .expect("configured shadow profile");
        let snapshot = local.state.inventory.as_ref().expect("verified inventory");
        let channel = snapshot
            .channels()
            .iter()
            .find(|c| c.id() == prepared.view.channel_id)
            .expect("verified channel");
        let attempt = Attempt {
            attempt_id: Uuid::new_v4(),
            ranking_id: prepared.ranking_id,
            channel_id: channel.id(),
            gateway: channel.gateway.clone(),
            inventory_snapshot_id: snapshot.id.clone(),
            state_revision: prepared.view.state_revision,
            started_at: now,
            ended_at: None,
            outcome: Outcome::Active,
            simulated: true,
            scenario_id: shadow.scenario_id.clone(),
            modem_connected: None,
            client_transfer: None,
            cms_accepted: None,
            recipient_delivered: None,
            release_confirmed: None,
        };
        local.journal.append(
            &self.profile_id,
            now,
            Event::Started {
                prepared_id,
                grant_id,
                attempt: attempt.clone(),
            },
        )?;
        let (cancel, canceled) = oneshot::channel();
        local.state.active = Some(Active {
            id: attempt.attempt_id,
            _cancel: cancel,
        });
        local.state.prepared = None;
        local
            .state
            .starts
            .insert(grant_id, (prepared_id, attempt.attempt_id));
        local
            .state
            .attempts
            .insert(attempt.attempt_id, attempt.clone());
        let limit = Duration::from_secs(cfg.session_seconds.into());
        let service = self.clone();
        let id = attempt.attempt_id;
        let started = Instant::now();
        tokio::spawn(async move {
            let duration = simulated.map_or(limit, |result| {
                Duration::from_millis(result.duration_ms.into())
            });
            let outcome = if simulated.is_none() || duration >= limit {
                None
            } else {
                simulated
            };
            tokio::select! {
                biased;
                _ = canceled => {},
                _ = tokio::time::sleep_until(started + duration.min(limit)) => {
                    let _ = service.finish(id, outcome, Outcome::TimedOut).await;
                }
            }
        });
        Ok(attempt)
    }

    async fn finish(
        &self,
        id: Uuid,
        simulated: Option<ScenarioResult>,
        fallback: Outcome,
    ) -> Result<Attempt> {
        // Release is independent of journal completion. The shadow adapter never keys a device.
        let radio = &self
            .shadow
            .as_ref()
            .ok_or(Error::Denied(
                "station execution is available only in shadow mode",
            ))?
            .radio;
        let released = radio
            .request_ptt(PttRequest {
                action: PttAction::KeyDown,
                session_id: None,
                transmit_permit_id: None,
            })
            .await
            .is_ok_and(|release| release.release_confirmed)
            && simulated.is_none_or(|s| s.release_confirmed);
        let mut local = self.local.lock().await;
        let mut attempt = local
            .state
            .attempts
            .get(&id)
            .cloned()
            .ok_or(Error::Invalid("unknown attempt ID"))?;
        if local
            .state
            .active
            .as_ref()
            .is_none_or(|active| active.id != id)
        {
            return Ok(attempt);
        }
        attempt.ended_at = Some(Utc::now());
        attempt.release_confirmed = Some(released);
        attempt.modem_connected = simulated.map(|s| s.modem_connected);
        attempt.client_transfer = simulated.map(|s| s.client_transfer);
        attempt.outcome = if !released {
            Outcome::ReleaseUnconfirmed
        } else if let Some(s) = simulated {
            if s.client_transfer {
                Outcome::Completed
            } else {
                Outcome::Failed
            }
        } else {
            fallback
        };
        let result = local.journal.append(
            &self.profile_id,
            attempt.ended_at.expect("terminal attempt time"),
            Event::Attempt(attempt.clone()),
        );
        local.state.inhibited |= !released || result.is_err();
        local.state.active = None;
        local.state.attempts.insert(id, attempt.clone());
        result?;
        Ok(attempt)
    }

    pub async fn abort(&self, attempt_id: Uuid) -> Result<Attempt> {
        self.finish(attempt_id, None, Outcome::Aborted).await
    }

    pub async fn shutdown(&self) -> Result<()> {
        let active = {
            let mut local = self.local.lock().await;
            local.state.prepared = None;
            local.state.active.as_ref().map(|active| active.id)
        };
        if let Some(id) = active {
            self.abort(id).await?;
        }
        Ok(())
    }

    pub async fn attempt(&self, id: Uuid) -> Result<Attempt> {
        self.local
            .lock()
            .await
            .state
            .attempts
            .get(&id)
            .cloned()
            .ok_or(Error::Invalid("unknown attempt ID"))
    }

    pub async fn history(&self, from: DateTime<Utc>, limit: usize) -> Vec<Attempt> {
        let local = self.local.lock().await;
        let mut attempts = local
            .state
            .attempts
            .values()
            .filter(|attempt| attempt.started_at >= from)
            .collect::<Vec<_>>();
        attempts.sort_unstable_by_key(|attempt| {
            std::cmp::Reverse((attempt.started_at, attempt.attempt_id))
        });
        attempts
            .into_iter()
            .take(limit.clamp(1, PAGE_LIMIT))
            .cloned()
            .collect()
    }

    pub async fn metrics(&self, from: DateTime<Utc>) -> Metrics {
        let now = Utc::now();
        let mut m = Metrics {
            from,
            to: now,
            attempted: 0,
            modem_connected: 0,
            modem_outcome_known: 0,
            client_transfer_confirmed: 0,
            client_outcome_known: 0,
            simulated: self.is_shadow(),
        };
        let local = self.local.lock().await;
        for a in local
            .state
            .attempts
            .values()
            .filter(|a| from <= a.started_at && a.started_at <= now)
        {
            m.attempted += 1;
            m.modem_connected += usize::from(a.modem_connected == Some(true));
            m.modem_outcome_known += usize::from(a.modem_connected.is_some());
            m.client_transfer_confirmed += usize::from(a.client_transfer == Some(true));
            m.client_outcome_known += usize::from(a.client_transfer.is_some());
        }
        m
    }
}

fn cooling(ended: DateTime<Utc>, now: DateTime<Utc>, seconds: u32) -> bool {
    now - ended < TimeDelta::seconds(seconds.into())
}
