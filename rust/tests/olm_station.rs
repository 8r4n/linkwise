use chrono::{TimeDelta, Utc};
use linkwise::{
    olm::{Attempt, Error, InventorySummary, Olm, Outcome},
    radio::{ControlValue, RadioControlPort, SimulatedRadio},
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};
use tempfile::TempDir;
use uuid::Uuid;

const ALPHA: &str = "ALPHA/VARA-HF/PUBLIC/7100000/7101500/500";
const BETA: &str = "BETA/VARA-HF/PUBLIC/7200000/7201500/500";

struct Fixture {
    directory: TempDir,
    inventory: Value,
}

impl Fixture {
    fn new(scenario: Value) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let as_of = Utc::now() - TimeDelta::seconds(5);
        let channel = |gateway, dial_hz: u64| {
            json!({
                "gateway": gateway,
                "protocol": "VARA-HF",
                "service_code": "PUBLIC",
                "dial_hz": dial_hz,
                "center_hz": dial_hz + 1500,
                "bandwidth_hz": 500,
                "reported_at": as_of,
            })
        };
        let inventory = json!({
            "version": 1,
            "source": "station test inventory",
            "as_of": as_of,
            "complete": true,
            "channels": [channel("BETA", 7_200_000), channel("ALPHA", 7_100_000)],
        });
        let forecast = |channel_id, reliability_bp| {
            json!({
                "channel_id": channel_id,
                "model": "VOACAP",
                "model_revision": "test-1",
                "inputs_id": "station-test-path",
                "issued_at": as_of,
                "valid_from": as_of,
                "valid_until": as_of + TimeDelta::hours(1),
                "reliability_bp": reliability_bp,
            })
        };
        let fixture = Self {
            directory,
            inventory,
        };
        fixture.write("inventory.json", &fixture.inventory);
        fixture.write(
            "forecast.json",
            &json!({ "version": 1, "forecasts": [forecast(BETA, 6200), forecast(ALPHA, 7800)] }),
        );
        fixture.write(
            "scenario.json",
            &json!({ "version": 1, "channels": scenario }),
        );
        fixture.write(
            "station.json",
            &json!({
                "version": 1,
                "inventory_path": "inventory.json",
                "forecast_path": "forecast.json",
                "journal_dir": "journal",
                "refresh_seconds": 15,
                "inventory_max_age_seconds": 3600,
                "report_max_age_seconds": 3600,
                "service_codes": ["PUBLIC"],
                "dial_ranges": [{ "min_hz": 7_000_000, "max_hz": 7_300_000 }],
                "shadow": {
                    "scenario_path": "scenario.json",
                    "prepare_seconds": 6,
                    "grant_seconds": 2,
                    "session_seconds": 3,
                    "cooldown_seconds": 30,
                    "switch_cost_bp": 200,
                },
            }),
        );
        fixture
    }

    fn path(&self, name: &str) -> PathBuf {
        self.directory.path().join(name)
    }

    fn write(&self, name: &str, value: &Value) {
        fs::write(self.path(name), serde_json::to_vec(value).unwrap()).unwrap();
    }

    fn open(&self) -> Arc<Olm> {
        Olm::open(
            &self.path("station.json"),
            Some(Arc::new(SimulatedRadio::from_env())),
        )
        .unwrap()
    }
}

fn simulated_result(duration_ms: u32, release_confirmed: bool) -> Value {
    json!({
        "duration_ms": duration_ms,
        "modem_connected": true,
        "client_transfer": true,
        "release_confirmed": release_confirmed,
    })
}

async fn rank(olm: &Olm) -> (Uuid, Value) {
    let result = serde_json::to_value(olm.rank(None, 0, 100).await.unwrap()).unwrap();
    let id = serde_json::from_value(result["ranking_id"].clone()).unwrap();
    (id, result)
}

async fn start(olm: &Arc<Olm>, channel: &str) -> Attempt {
    let (ranking, _) = rank(olm).await;
    let prepared = olm.prepare(ranking, channel).await.unwrap();
    let grant = olm.authorize(prepared.prepared_id).await.unwrap();
    olm.start(prepared.prepared_id, grant.grant_id)
        .await
        .unwrap()
}

async fn terminal(olm: &Olm, attempt_id: Uuid, elapsed: Duration) -> Attempt {
    tokio::time::advance(elapsed).await;
    tokio::time::timeout(Duration::from_millis(50), async {
        loop {
            let attempt = olm.attempt(attempt_id).await.unwrap();
            if attempt.outcome != Outcome::Active {
                return attempt;
            }
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await
    .expect("the simulated attempt should reach a terminal state")
}

#[tokio::test]
async fn ranking_deduplicates_and_keeps_last_complete_snapshot_when_refresh_fails() {
    let fixture = Fixture::new(json!({}));
    let mut inventory = fixture.inventory.clone();
    let original = inventory["channels"][1].clone();
    let mut alternate = original.clone();
    alternate["dial_hz"] = json!(7_151_000);
    alternate["center_hz"] = json!(7_152_500);
    let mut unavailable = alternate.clone();
    unavailable["gateway"] = json!("CHARLIE");
    unavailable["dial_hz"] = Value::Null;
    unavailable["reported_at"] = Value::Null;
    inventory["channels"]
        .as_array_mut()
        .unwrap()
        .extend([original, alternate, unavailable]);
    fixture.write("inventory.json", &inventory);
    let olm = fixture.open();

    let summary = olm.refresh().await.unwrap();
    assert_eq!(summary.known_gateways, Some(3));
    assert_eq!(summary.known_channels, Some(4));
    assert_eq!(summary.freshly_reported_gateways, Some(2));
    assert_eq!(summary.eligible_channels, Some(3));
    assert!(!summary.stale);
    let (ranking, page) = rank(&olm).await;
    assert_eq!(page["snapshot_id"], json!(summary.snapshot_id));
    assert_eq!(
        page["decision"],
        json!({"action": "select", "channel_id": ALPHA})
    );
    assert_eq!(
        page["entries"][0]["forecast"]["inputs_id"],
        "station-test-path"
    );
    assert_eq!(page["entries"][0]["utility_bp"], 7800);
    assert_eq!(page["entries"][1]["channel_id"], BETA);
    assert!(page["entries"][2]["utility_bp"].is_null());
    assert_eq!(
        page["entries"][3]["exclusions"],
        json!(["report_unknown_or_stale", "dial_frequency_unknown"]),
    );
    assert!(page["entries"][3]["rank"].is_null());

    inventory["channels"].as_array_mut().unwrap().reverse();
    fixture.write("inventory.json", &inventory);
    let (_, reordered) = rank(&olm).await;
    assert_eq!(reordered["snapshot_id"], page["snapshot_id"]);
    assert_eq!(reordered["entries"], page["entries"]);

    let mut partial = inventory.clone();
    partial["complete"] = json!(false);
    partial["channels"] = json!([]);
    fixture.write("inventory.json", &partial);
    let summary = olm.refresh().await.unwrap();
    assert!(summary.stale);
    assert!(summary.refresh_error.is_some());
    assert_eq!(summary.known_channels, Some(4));
    assert_eq!(summary.eligible_channels, Some(0));
    let (_, waiting) = rank(&olm).await;
    assert_eq!(waiting["snapshot_id"], page["snapshot_id"]);
    assert_eq!(waiting["decision"]["action"], "wait");
    let old_page = serde_json::to_value(olm.rank(Some(ranking), 1, 1).await.unwrap()).unwrap();
    assert_eq!(old_page["entries"], json!([page["entries"][1]]));
    assert!(matches!(
        olm.prepare(ranking, ALPHA).await,
        Err(Error::Denied(_))
    ));

    drop(olm);
    let olm = fixture.open();
    let failed = olm.inventory().await;
    assert!(failed.stale);
    assert!(failed.refresh_error.is_some());
    assert_eq!(failed.known_channels, Some(4));
    let replayed = serde_json::to_value(olm.rank(Some(ranking), 1, 1).await.unwrap()).unwrap();
    assert_eq!(replayed, old_page);

    fixture.write("inventory.json", &inventory);
    let recovered = olm.refresh().await.unwrap();
    assert!(!recovered.stale);
    assert!(recovered.refresh_error.is_none());
    assert_eq!(recovered.eligible_channels, Some(3));

    drop(olm);
    fs::remove_file(fixture.path("inventory.json")).unwrap();
    let reopened = fixture.open();
    let disk_snapshot = reopened.refresh().await.unwrap();
    assert_eq!(disk_snapshot.snapshot_id, recovered.snapshot_id);
    assert!(disk_snapshot.stale);
    assert!(disk_snapshot.refresh_error.is_some());
    assert_eq!(disk_snapshot.known_channels, Some(4));
    assert_eq!(disk_snapshot.eligible_channels, Some(0));
}

#[tokio::test(start_paused = true)]
async fn scheduled_refresh_preserves_snapshot_and_recovers_from_invalid_import() {
    async fn inventory_when(
        olm: &Olm,
        ready: impl Fn(&InventorySummary) -> bool,
    ) -> InventorySummary {
        tokio::time::timeout(Duration::from_millis(50), async {
            loop {
                let summary = olm.inventory().await;
                if ready(&summary) {
                    return summary;
                }
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        })
        .await
        .expect("scheduler must publish the expected inventory state")
    }

    let fixture = Fixture::new(json!({}));
    let olm = fixture.open();
    let refresh = olm.start_refresh();
    let first = inventory_when(&olm, |summary| !summary.stale).await;
    assert_eq!(first.known_channels, Some(2));
    assert_eq!(first.eligible_channels, Some(2));

    fs::write(fixture.path("inventory.json"), b"{").unwrap();
    tokio::time::advance(Duration::from_secs(15)).await;
    let failed = inventory_when(&olm, |summary| summary.refresh_error.is_some()).await;
    assert_eq!(failed.snapshot_id, first.snapshot_id);
    assert!(failed.stale);
    assert_eq!(failed.known_channels, Some(2));
    assert_eq!(failed.eligible_channels, Some(0));

    fixture.write("inventory.json", &fixture.inventory);
    tokio::time::advance(Duration::from_secs(15)).await;
    let recovered = inventory_when(&olm, |summary| !summary.stale).await;
    assert_eq!(recovered.snapshot_id, first.snapshot_id);
    assert_eq!(recovered.eligible_channels, Some(2));
    assert!(recovered.refresh_error.is_none());
    refresh.abort();
    assert!(refresh.await.unwrap_err().is_cancelled());
}

#[tokio::test(start_paused = true)]
async fn one_use_grant_serializes_attempt_and_preserves_distinct_outcomes() {
    let fixture = Fixture::new(json!({ (ALPHA): simulated_result(250, true) }));
    let olm = fixture.open();
    assert!(matches!(
        Olm::open(
            &fixture.path("station.json"),
            Some(Arc::new(SimulatedRadio::from_env()))
        ),
        Err(Error::Busy(_)),
    ));
    let (ranking, _) = rank(&olm).await;
    let prepared = olm.prepare(ranking, ALPHA).await.unwrap();
    assert!(matches!(
        olm.prepare(ranking, BETA).await,
        Err(Error::Busy(_))
    ));
    let grant = olm.authorize(prepared.prepared_id).await.unwrap();
    assert_eq!(
        olm.authorize(prepared.prepared_id).await.unwrap().grant_id,
        grant.grant_id
    );
    assert!(matches!(
        olm.start(prepared.prepared_id, Uuid::new_v4()).await,
        Err(Error::Denied(_))
    ));

    let active = olm
        .start(prepared.prepared_id, grant.grant_id)
        .await
        .unwrap();
    assert_eq!(active.outcome, Outcome::Active);
    assert!(active.simulated);
    assert!(active.ended_at.is_none());
    assert!(active.release_confirmed.is_none());
    let repeated = olm
        .start(prepared.prepared_id, grant.grant_id)
        .await
        .unwrap();
    assert_eq!(repeated.attempt_id, active.attempt_id);
    assert_eq!(repeated.outcome, Outcome::Active);
    assert!(matches!(
        olm.prepare(ranking, BETA).await,
        Err(Error::Busy(_))
    ));

    let completed = terminal(&olm, active.attempt_id, Duration::from_millis(250)).await;
    assert_eq!(completed.outcome, Outcome::Completed);
    assert_eq!(completed.modem_connected, Some(true));
    assert_eq!(completed.client_transfer, Some(true));
    assert_eq!(completed.release_confirmed, Some(true));
    assert!(completed.cms_accepted.is_none());
    assert!(completed.recipient_delivered.is_none());
    assert!(matches!(
        olm.authorize(prepared.prepared_id).await,
        Err(Error::Denied(_))
    ));
    let repeated = olm
        .start(prepared.prepared_id, grant.grant_id)
        .await
        .unwrap();
    assert_eq!(repeated.attempt_id, completed.attempt_id);
    assert_eq!(repeated.outcome, Outcome::Completed);

    let other = start(&olm, BETA).await;
    assert_ne!(other.attempt_id, completed.attempt_id);
    assert_eq!(
        olm.abort(other.attempt_id).await.unwrap().outcome,
        Outcome::Aborted
    );
    assert_eq!(
        olm.abort(other.attempt_id).await.unwrap().outcome,
        Outcome::Aborted
    );
    let since = Utc::now() - TimeDelta::hours(1);
    let history = olm.history(since, 10).await;
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].attempt_id, other.attempt_id);
    assert_eq!(history[1].attempt_id, completed.attempt_id);
    let metrics = olm.metrics(since).await;
    assert!(metrics.simulated);
    assert_eq!(metrics.attempted, 2);
    assert_eq!(metrics.modem_connected, 1);
    assert_eq!(metrics.modem_outcome_known, 1);
    assert_eq!(metrics.client_transfer_confirmed, 1);
    assert_eq!(metrics.client_outcome_known, 1);
    olm.shutdown().await.unwrap();
}

#[tokio::test(start_paused = true)]
async fn changed_readback_and_expired_grant_cannot_start_and_timeout_cools_down() {
    let fixture = Fixture::new(json!({ (BETA): simulated_result(8000, true) }));
    let radio = Arc::new(SimulatedRadio::from_env());
    let olm = Olm::open(&fixture.path("station.json"), Some(radio.clone())).unwrap();
    let (ranking, _) = rank(&olm).await;
    let prepared = olm.prepare(ranking, ALPHA).await.unwrap();
    let grant = olm.authorize(prepared.prepared_id).await.unwrap();
    radio
        .set_control(
            "frequency_hz",
            ControlValue::FrequencyHz(7_150_000),
            Some(prepared.state_revision),
        )
        .await
        .unwrap();
    assert!(matches!(
        olm.start(prepared.prepared_id, grant.grant_id).await,
        Err(Error::Denied(_))
    ));

    tokio::time::advance(Duration::from_secs(6)).await;
    let prepared = olm.prepare(ranking, BETA).await.unwrap();
    let grant = olm.authorize(prepared.prepared_id).await.unwrap();
    tokio::time::advance(Duration::from_secs(2)).await;
    assert!(matches!(
        olm.start(prepared.prepared_id, grant.grant_id).await,
        Err(Error::Denied(_))
    ));
    assert!(matches!(
        olm.authorize(prepared.prepared_id).await,
        Err(Error::Denied(_))
    ));

    tokio::time::advance(Duration::from_secs(4)).await;
    let active = start(&olm, BETA).await;
    let finished = terminal(&olm, active.attempt_id, Duration::from_secs(3)).await;
    assert_eq!(finished.outcome, Outcome::TimedOut);
    assert_eq!(finished.release_confirmed, Some(true));
    assert!(finished.modem_connected.is_none());
    assert!(finished.client_transfer.is_none());
    let (ranking, page) = rank(&olm).await;
    assert_eq!(
        page["entries"][1]["exclusions"],
        json!(["previous_attempt_in_cooldown"])
    );
    assert!(matches!(
        olm.prepare(ranking, BETA).await,
        Err(Error::Denied(_))
    ));
}

#[tokio::test(start_paused = true)]
async fn release_unconfirmed_inhibits_station_until_simulated_restart() {
    let fixture = Fixture::new(json!({ (ALPHA): simulated_result(10, false) }));
    let olm = fixture.open();
    let active = start(&olm, ALPHA).await;
    let finished = terminal(&olm, active.attempt_id, Duration::from_millis(10)).await;
    assert_eq!(finished.outcome, Outcome::ReleaseUnconfirmed);
    assert_eq!(finished.release_confirmed, Some(false));
    let (ranking, _) = rank(&olm).await;
    assert!(matches!(
        olm.prepare(ranking, BETA).await,
        Err(Error::Denied("station is inhibited"))
    ));
    assert_eq!(
        olm.abort(active.attempt_id).await.unwrap().outcome,
        Outcome::ReleaseUnconfirmed
    );
    olm.shutdown().await.unwrap();
    drop(olm);

    let restarted = fixture.open();
    assert_eq!(
        restarted.attempt(active.attempt_id).await.unwrap().outcome,
        Outcome::ReleaseUnconfirmed
    );
    let (ranking, _) = rank(&restarted).await;
    assert!(restarted.prepare(ranking, BETA).await.is_ok());
    restarted.shutdown().await.unwrap();
}

#[test]
fn restart_marks_open_attempt_interrupted_and_does_not_restore_grants() {
    let fixture = Fixture::new(json!({}));
    let runtime = || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .start_paused(true)
            .build()
            .unwrap()
    };
    let before = runtime();
    let (prepared_id, grant_id, attempt_id, scenario_id) = before.block_on(async {
        let olm = fixture.open();
        let (ranking, _) = rank(&olm).await;
        let prepare = olm.prepare(ranking, ALPHA).await.unwrap();
        let grant = olm.authorize(prepare.prepared_id).await.unwrap();
        let attempt = olm
            .start(prepare.prepared_id, grant.grant_id)
            .await
            .unwrap();
        assert_eq!(attempt.outcome, Outcome::Active);
        (
            prepare.prepared_id,
            grant.grant_id,
            attempt.attempt_id,
            attempt.scenario_id,
        )
    });
    drop(before);
    fixture.write(
        "scenario.json",
        &json!({ "version": 1, "channels": { (BETA): simulated_result(150, true) } }),
    );

    let (unused_preparation, unused_grant) = runtime().block_on(async {
        let olm = fixture.open();
        let attempt = olm.attempt(attempt_id).await.unwrap();
        assert_eq!(attempt.outcome, Outcome::Interrupted);
        assert_eq!(attempt.scenario_id, scenario_id);
        assert!(attempt.ended_at.is_some());
        assert!(attempt.release_confirmed.is_none());
        let repeated = olm.start(prepared_id, grant_id).await.unwrap();
        assert_eq!(repeated.attempt_id, attempt_id);
        assert_eq!(repeated.outcome, Outcome::Interrupted);
        assert!(matches!(
            olm.start(prepared_id, Uuid::new_v4()).await,
            Err(Error::Denied(_))
        ));
        assert!(matches!(
            olm.authorize(prepared_id).await,
            Err(Error::Denied(_))
        ));
        let (ranking, _) = rank(&olm).await;
        let prepared = olm.prepare(ranking, BETA).await.unwrap();
        let grant = olm.authorize(prepared.prepared_id).await.unwrap();
        (prepared.prepared_id, grant.grant_id)
    });

    runtime().block_on(async {
        let olm = fixture.open();
        assert!(matches!(
            olm.start(unused_preparation, unused_grant).await,
            Err(Error::Denied(_))
        ));
        assert!(matches!(
            olm.authorize(unused_preparation).await,
            Err(Error::Denied(_))
        ));
        let active = start(&olm, BETA).await;
        assert_ne!(active.scenario_id, scenario_id);
        olm.shutdown().await.unwrap();
        assert_eq!(
            olm.attempt(active.attempt_id).await.unwrap().outcome,
            Outcome::Aborted
        );
    });
}

struct Mcp {
    child: Child,
    stdout: mpsc::Receiver<String>,
    id: u64,
}

impl Mcp {
    fn start(fixture: &Fixture) -> Self {
        Self::start_with_binary(fixture, Path::new(env!("CARGO_BIN_EXE_linkwise")))
    }

    fn start_with_binary(fixture: &Fixture, binary: &Path) -> Self {
        let process_temp = fixture.path("process-temp");
        fs::create_dir_all(&process_temp).unwrap();
        let mut child = Command::new(binary)
            .env("LINKWISE_RADIO_BACKEND", "simulated")
            .env("LINKWISE_OLM_CONFIG", fixture.path("station.json"))
            .env("LINKWISE_AUDIT_PATH", fixture.path("radio.jsonl"))
            .env("TMPDIR", &process_temp)
            .env("TEMP", &process_temp)
            .env("TMP", &process_temp)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let (sender, stdout) = mpsc::channel();
        let stream = BufReader::new(child.stdout.take().unwrap());
        thread::spawn(move || {
            for line in stream.lines().map_while(Result::ok) {
                if sender.send(line).is_err() {
                    break;
                }
            }
        });
        Self {
            child,
            stdout,
            id: 0,
        }
    }

    fn send(&mut self, message: Value) {
        let stdin = self.child.stdin.as_mut().unwrap();
        writeln!(stdin, "{message}").unwrap();
        stdin.flush().unwrap();
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        self.send(json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params}));
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let line = self
                .stdout
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect(method);
            let reply: Value = serde_json::from_str(&line).unwrap();
            if reply["id"] == self.id {
                assert!(reply["error"].is_null(), "{method}: {reply}");
                return reply["result"].clone();
            }
        }
    }

    fn tool(&mut self, name: &str, arguments: Value, status: &str) -> Value {
        let reply = self.request("tools/call", json!({"name": name, "arguments": arguments}));
        let result: Value = if let Some(value) = reply.get("structuredContent") {
            value.clone()
        } else {
            serde_json::from_str(reply["content"][0]["text"].as_str().unwrap()).unwrap()
        };
        assert_eq!(result["status"], status, "{name}: {result}");
        result["data"].clone()
    }

    fn initialize(&mut self) {
        let initialized = self.request(
            "initialize",
            json!({
                "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "station-integration-test", "version": "1" },
            }),
        );
        assert_eq!(initialized["protocolVersion"], "2025-06-18");
        self.send(json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    }

    fn start_attempt(&mut self, channel_id: &str) -> (Value, Value) {
        let ranking = self.tool("link_rank_gateways", json!({}), "ok");
        assert_eq!(ranking["decision"]["channel_id"], channel_id);
        let prepared = self.tool(
            "link_prepare_candidate",
            json!({"ranking_id": ranking["ranking_id"], "channel_id": channel_id}),
            "ok",
        );
        let grant = self.tool(
            "link_request_session_authorization",
            json!({"prepared_id": prepared["prepared_id"]}),
            "ok",
        );
        let ids = json!({"prepared_id": prepared["prepared_id"], "grant_id": grant["grant_id"]});
        let active = self.tool("link_start_session", ids.clone(), "ok");
        assert_eq!(active["outcome"], "active");
        (active, ids)
    }

    fn terminal_attempt(&mut self, id: &Value, limit: Duration) -> Value {
        let deadline = Instant::now() + limit;
        loop {
            let attempt = self.tool("link_get_session", json!({"attempt_id": id}), "ok");
            if attempt["outcome"] != "active" {
                return attempt;
            }
            assert!(Instant::now() < deadline, "shadow attempt did not finish");
            thread::sleep(Duration::from_millis(25));
        }
    }

    fn close(&mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                assert!(status.success(), "MCP process returned {status}");
                return;
            }
            assert!(
                Instant::now() < deadline,
                "MCP did not close after stdin EOF"
            );
            thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let deadline = Instant::now() + Duration::from_secs(2);
        while matches!(self.child.try_wait(), Ok(None)) && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn stdio_mcp_runs_a_shadow_session_with_radio_controls_gated() {
    let fixture = Fixture::new(json!({ (ALPHA): simulated_result(150, true) }));
    let mut mcp = Mcp::start(&fixture);
    mcp.initialize();
    let listing = mcp.request("tools/list", json!({}));
    assert!(
        listing["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|tool| tool["name"] == "link_start_session")
    );

    let inventory = mcp.tool("winlink_refresh_inventory", json!({}), "ok");
    assert_eq!(inventory["eligible_channels"], 2);
    let rank = mcp.tool("link_rank_gateways", json!({}), "ok");
    assert_eq!(rank["decision"]["channel_id"], ALPHA);
    let prepared = mcp.tool(
        "link_prepare_candidate",
        json!({"ranking_id": rank["ranking_id"], "channel_id": ALPHA}),
        "ok",
    );
    mcp.tool(
        "radio_set_control",
        json!({
            "control_id": "frequency_hz", "value": { "type": "frequency_hz", "value": 7_000_000 },
        }),
        "denied",
    );
    let grant = mcp.tool(
        "link_request_session_authorization",
        json!({"prepared_id": prepared["prepared_id"]}),
        "ok",
    );
    let ids = json!({"prepared_id": prepared["prepared_id"], "grant_id": grant["grant_id"]});
    let active = mcp.tool("link_start_session", ids.clone(), "ok");
    assert_eq!(active["outcome"], "active");
    assert_eq!(
        mcp.tool("link_start_session", ids, "ok")["attempt_id"],
        active["attempt_id"]
    );
    mcp.tool("radio_ptt_request", json!({"action": "key_up"}), "denied");
    assert_eq!(
        mcp.tool("radio_ptt_request", json!({"action": "key_down"}), "ok")["release_confirmed"],
        true
    );

    let final_attempt = mcp.terminal_attempt(&active["attempt_id"], Duration::from_secs(5));
    assert_eq!(final_attempt["outcome"], "completed");
    assert_eq!(final_attempt["client_transfer"], true);
    assert!(final_attempt["cms_accepted"].is_null());
    let metrics = mcp.tool("link_get_metrics", json!({"window_seconds": 60}), "ok");
    assert_eq!(metrics["attempted"], 1);
    assert_eq!(metrics["client_transfer_confirmed"], 1);
    mcp.close();
}

fn managed_test_binaries() -> (PathBuf, PathBuf) {
    match (
        std::env::var_os("LINKWISE_STATION_TEST_CLI"),
        std::env::var_os("LINKWISE_STATION_TEST_MODEMD"),
    ) {
        (None, None) => (
            env!("CARGO_BIN_EXE_linkwise").into(),
            env!("CARGO_BIN_EXE_linkwise-modemd").into(),
        ),
        (Some(cli), Some(modem)) => {
            let (cli, modem) = (PathBuf::from(cli), PathBuf::from(modem));
            assert!(
                cfg!(all(target_os = "linux", target_arch = "aarch64"))
                    && cli == Path::new("/usr/bin/linkwise")
                    && modem == Path::new("/usr/bin/linkwise-modemd"),
                "installed overrides require /usr/bin/linkwise and /usr/bin/linkwise-modemd on Linux ARM64"
            );
            assert!(
                cli.is_file() && modem.is_file(),
                "installed executables are missing"
            );
            (cli, modem)
        }
        _ => panic!("set both LINKWISE_STATION_TEST_CLI and LINKWISE_STATION_TEST_MODEMD"),
    }
}

#[test]
#[ignore = "requires LINKWISE_BW500_PROFILE pointing to the reference waveform profile"]
fn managed_virtual_bw500_mcp_reaps_aborted_pair_and_verifies_both_transfers() {
    let (cli, modem) = managed_test_binaries();
    let waveform = std::env::var_os("LINKWISE_BW500_PROFILE")
        .expect("provide LINKWISE_BW500_PROFILE for the managed waveform test");
    let waveform = fs::canonicalize(waveform).unwrap();
    let profile_digest = format!("{:x}", Sha256::digest(fs::read(&waveform).unwrap()));
    let fixture = Fixture::new(json!({}));
    let mut config: Value =
        serde_json::from_slice(&fs::read(fixture.path("station.json")).unwrap()).unwrap();
    config["shadow"] = json!({
        "virtual_bw500": {
            "modem_path": modem,
            "waveform_profile": waveform,
            "local_call": "W1AW",
        },
        "prepare_seconds": 15,
        "grant_seconds": 8,
        "session_seconds": 120,
        "cooldown_seconds": 120,
        "switch_cost_bp": 200,
    });
    fixture.write("station.json", &config);

    let mut mcp = Mcp::start_with_binary(&fixture, &cli);
    let process_temp = fixture.path("process-temp");
    let private_directory_exists = || fs::read_dir(&process_temp).unwrap().next().is_some();
    mcp.initialize();
    let (aborting, ids) = mcp.start_attempt(ALPHA);
    assert_eq!(aborting["execution"], "virtual_bw500");
    assert!(aborting["virtual_proof"].is_null());
    assert_eq!(
        mcp.tool("link_start_session", ids, "ok")["attempt_id"],
        aborting["attempt_id"]
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let status = mcp.tool("link_get_status", json!({}), "ok");
        assert_eq!(status["active_attempt_id"], aborting["attempt_id"]);
        if status["virtual_phase"] == "connected" {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "managed modem pair did not connect: {status}"
        );
        thread::sleep(Duration::from_millis(10));
    }
    assert!(private_directory_exists());
    let abort_args = json!({"attempt_id": aborting["attempt_id"]});
    let connected = mcp.tool("link_get_session", abort_args.clone(), "ok");
    assert_eq!(connected["outcome"], "active");
    assert_eq!(connected["modem_connected"], true);
    let aborted = mcp.tool("link_abort_session", abort_args.clone(), "ok");
    assert_eq!(aborted["outcome"], "aborted", "{aborted}");
    assert_eq!(aborted["modem_connected"], true);
    assert_eq!(aborted["virtual_cleanup_confirmed"], true);
    assert_eq!(aborted["release_confirmed"], true);
    assert!(aborted["virtual_proof"].is_null());
    assert!(!private_directory_exists());
    assert_eq!(mcp.tool("link_abort_session", abort_args, "ok"), aborted);

    let (active, ids) = mcp.start_attempt(BETA);
    assert_ne!(active["attempt_id"], aborted["attempt_id"]);
    let finished = mcp.terminal_attempt(&active["attempt_id"], Duration::from_secs(125));
    assert_eq!(finished["outcome"], "completed", "{finished}");
    for field in [
        "simulated",
        "modem_connected",
        "client_transfer",
        "virtual_cleanup_confirmed",
        "release_confirmed",
    ] {
        assert_eq!(finished[field], true, "{field}: {finished}");
    }
    assert!(finished["cms_accepted"].is_null());
    assert!(finished["recipient_delivered"].is_null());
    assert_eq!(
        finished["virtual_proof"],
        json!({
            "caller_to_listener": {
                "bytes": 23,
                "sha256": "4f812c6d72171e6c110a653c17b6b89fd315cdadcfd1453d8c34fdf51d29705b",
            },
            "listener_to_caller": {
                "bytes": 26,
                "sha256": "4a78d83d7ca293654f047b461f02e6f475dbfb0066b146358f94cf5ee2fee7e6",
            },
            "waveform_profile_sha256": profile_digest,
        })
    );
    assert!(!private_directory_exists());
    assert_eq!(mcp.tool("link_start_session", ids, "ok"), finished);
    let metrics = mcp.tool("link_get_metrics", json!({"window_seconds": 300}), "ok");
    assert_eq!(metrics["attempted"], 2);
    assert_eq!(metrics["client_transfer_confirmed"], 1);
    mcp.close();
}

fn edit_journal_event(fixture: &Fixture, kind: &str, edit: impl FnOnce(&mut Value)) {
    for file in fs::read_dir(fixture.path("journal")).unwrap() {
        let path = file.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("json") {
            continue;
        }
        let mut record: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        if record["event"]["type"] == kind {
            edit(&mut record["event"]["data"]);
            fs::write(path, serde_json::to_vec(&record).unwrap()).unwrap();
            return;
        }
    }
    panic!("missing {kind} journal event");
}

#[tokio::test]
async fn shutdown_denies_new_preparations() {
    let fixture = Fixture::new(json!({}));
    let olm = fixture.open();
    let (ranking, _) = rank(&olm).await;
    olm.shutdown().await.unwrap();
    assert!(olm.status().await.closing);
    assert!(matches!(
        olm.prepare(ranking, ALPHA).await,
        Err(Error::Denied("station service is closing"))
    ));
}

#[tokio::test]
async fn preparation_and_replay_recheck_persisted_inventory() {
    let fixture = Fixture::new(json!({}));
    let radio = Arc::new(SimulatedRadio::from_env());
    let olm = Olm::open(&fixture.path("station.json"), Some(radio.clone())).unwrap();
    let (ranking, _) = rank(&olm).await;
    let before = radio.state().await.unwrap();
    edit_journal_event(&fixture, "ranking", |ranking| {
        ranking["entries"][0]["channel"]["dial_hz"] = json!(6_500_000);
    });
    assert!(matches!(
        olm.prepare(ranking, ALPHA).await,
        Err(Error::Denied(_))
    ));
    assert_eq!(
        radio.state().await.unwrap().state_revision,
        before.state_revision
    );
    drop(olm);

    edit_journal_event(&fixture, "inventory", |snapshot| {
        snapshot["data"]["channels"][0]["dial_hz"] = json!(6_500_000);
    });
    assert!(matches!(
        Olm::open(&fixture.path("station.json"), Some(radio)),
        Err(Error::Invalid("inventory snapshot fingerprint mismatch"))
    ));
}

#[tokio::test]
async fn refresh_journal_failure_blocks_preparation_even_with_cached_inventory() {
    let fixture = Fixture::new(json!({}));
    let olm = fixture.open();
    let (ranking, _) = rank(&olm).await;
    assert!(!olm.inventory().await.stale);

    fixture.write("inventory.json", &json!("invalid inventory"));
    let records = fs::read_dir(fixture.path("journal"))
        .unwrap()
        .filter(|file| {
            file.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|ext| ext == "json")
        })
        .count();
    fs::create_dir(fixture.path(&format!("journal/{:020}.json", records + 1))).unwrap();
    assert!(olm.refresh().await.is_err());
    let inventory = olm.inventory().await;
    assert!(inventory.stale);
    assert_eq!(inventory.eligible_channels, Some(0));
    assert!(olm.status().await.inhibited);
    assert!(matches!(
        olm.prepare(ranking, ALPHA).await,
        Err(Error::Denied(_))
    ));
}
