//! Typed radio-control contracts and the current radio backend adapters.
//!
//! [`RadioControlPort`] separates MCP request handling from a radio backend.
//! [`RigctldRadio`] translates the typed contract to Hamlib's line-oriented
//! `rigctld` protocol; [`SimulatedRadio`] provides limited process-local state
//! for development. See `rust/docs/developer-guide.md` for the implementation
//! flow, invariants, and extension checklist.

use std::{collections::BTreeMap, env, sync::Arc, time::Duration};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
    sync::Mutex,
    time::timeout,
};
use uuid::Uuid;

const DEFAULT_RIGCTLD_ADDR: &str = "hamlib:4532";
const DEFAULT_TIMEOUT_MS: u64 = 2_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case", tag = "type", content = "value")]
/// A typed radio value serialized for MCP as `{ "type": "...", "value": ... }`.
///
/// The selected variant must match the control's descriptor `value_type`.
pub enum ControlValue {
    /// Frequency in whole hertz.
    FrequencyHz(u64),
    /// Hamlib mode identifier, conventionally uppercase (for example `USB`).
    Mode(String),
    /// Observed PTT assertion state; it is not a keying request.
    PttState(bool),
    /// Hamlib's RFPOWER level is normalized from 0.0 (0%) to 1.0 (100%).
    RfPowerFraction(f64),
    /// Typed scalar values for capability-discovered Hamlib levels and parameters.
    Number(f64),
    /// Boolean values for capability-discovered Hamlib functions.
    Boolean(bool),
    /// Enumerated or textual values such as VFO and antenna identifiers.
    Text(String),
    /// Filter/passband width in hertz.
    PassbandHz(u32),
    /// Unsigned integer control value, such as a memory channel index.
    Integer(u64),
    /// Split operation state and transmit VFO selection.
    SplitVfo { enabled: bool, tx_vfo: String },
    /// Hamlib antenna index (backend indexing conventions may vary).
    AntennaIndex(u32),
    /// Signed tuning offset in hertz; sign is meaningful.
    SignedHertz(i64),
    /// Split transmit mode and passband width in hertz.
    ModePassband { mode: String, passband_hz: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
/// Requested PTT operation. Current backends permit release but deny key-up.
pub enum PttAction {
    /// Request transmit/key-up. Currently denied until an authorization arbiter exists.
    KeyUp,
    /// Request receive/key-down. Release is intended to remain available as a safety action.
    KeyDown,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// PTT request metadata. The optional IDs are not currently validated as authorization.
pub struct PttRequest {
    /// Requested transition.
    pub action: PttAction,
    /// Reserved for future session-bound transmit authorization.
    pub session_id: Option<String>,
    /// Reserved for a future one-use transmit permit; not currently enforced.
    pub transmit_permit_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Result of a PTT request, separating the request from observed hardware state.
pub struct PttObservation {
    /// The operation that was requested.
    pub requested_action: PttAction,
    /// Observed asserted state, if the backend supports readback.
    pub observed_ptt_asserted: Option<bool>,
    /// Whether release was positively observed/confirmed.
    pub release_confirmed: bool,
    /// UTC RFC 3339 observation timestamp.
    pub observed_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Backend capability and current policy for one typed control.
pub struct ControlDescriptor {
    /// Exact ID accepted by `control` and `set_control`.
    pub control_id: String,
    /// Whether the backend reports the control as available.
    pub supported: bool,
    /// Whether a current value can be read.
    pub readable: bool,
    /// Whether the installed configuration currently allows a write.
    pub writable: bool,
    /// Human-readable policy class; descriptive metadata, not enforcement itself.
    pub access_class: String,
    /// Short backend/control description.
    pub description: String,
    /// Expected `ControlValue` variant name.
    pub value_type: String,
    /// Inclusive minimum when the backend/profile provides one.
    pub minimum: Option<f64>,
    /// Inclusive maximum when the backend/profile provides one.
    pub maximum: Option<f64>,
    /// Valid symbolic values when known.
    pub enum_values: Vec<String>,
    /// Descriptive side-effect classification.
    pub side_effect_class: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Best-effort radio snapshot. Missing fields are represented by `None` and explained in `field_errors`.
pub struct RadioState {
    /// Optional backend-provided model or rig identification.
    pub radio_info: Option<String>,
    /// VFO frequency in hertz.
    pub frequency_hz: Option<u64>,
    /// Current mode identifier, generally Hamlib's symbolic name.
    pub mode: Option<String>,
    /// Current mode filter width in hertz.
    pub passband_hz: Option<u32>,
    /// Whether PTT is asserted, if the backend can observe it.
    pub ptt_asserted: Option<bool>,
    /// RF power as a normalized fraction, if readable.
    pub rf_power_fraction: Option<f64>,
    /// UTC RFC 3339 timestamp for this snapshot.
    pub observed_at_utc: String,
    /// Linkwise-process-local write counter; external radio changes may not increment it.
    pub state_revision: u64,
    /// Per-field read failures; a partial snapshot may still be useful.
    pub field_errors: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// Raw backend capability response plus normalized Linkwise descriptors.
pub struct RadioCapabilities {
    /// Unmodified or joined backend capability text, intended for diagnostics.
    pub backend_dump: String,
    /// Normalized control catalog used by MCP clients.
    pub controls: Vec<ControlDescriptor>,
    /// UTC RFC 3339 discovery timestamp.
    pub observed_at_utc: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
/// MCP-facing result envelope. `status` values include `ok`, `denied`,
/// `unsupported`, `unavailable`, `readback_mismatch`, and `failed`;
/// the MCP adapter may additionally report `audit_failure`.
pub struct ApiResult<T> {
    /// Unique identifier for this returned result.
    pub request_id: String,
    /// Identifier shared by related request/completion audit records when applicable.
    pub correlation_id: String,
    /// UTC RFC 3339 result timestamp.
    pub observed_at_utc: String,
    /// Stable machine-readable outcome category.
    pub status: String,
    /// Present for successful operations; absent for ordinary failures.
    pub data: Option<T>,
    /// Non-fatal conditions or degraded guarantees.
    pub warnings: Vec<String>,
    /// Failure details; may also be populated when a side effect's outcome is uncertain.
    pub errors: Vec<String>,
}

impl<T> ApiResult<T> {
    pub fn ok(data: T) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            request_id: Uuid::new_v4().to_string(),
            correlation_id: Uuid::new_v4().to_string(),
            observed_at_utc: now,
            status: "ok".into(),
            data: Some(data),
            warnings: Vec::new(),
            errors: Vec::new(),
        }
    }

    pub fn failed(message: impl Into<String>) -> Self {
        let now = chrono::Utc::now().to_rfc3339();
        Self {
            request_id: Uuid::new_v4().to_string(),
            correlation_id: Uuid::new_v4().to_string(),
            observed_at_utc: now,
            status: "failed".into(),
            data: None,
            warnings: Vec::new(),
            errors: vec![message.into()],
        }
    }
}

fn parse_discovered_tokens(query: &str, line: &str) -> Result<Vec<String>, RadioError> {
    let tokens: Vec<String> = line
        .split(|character: char| character.is_whitespace() || character == ',')
        .map(|token| {
            token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != '_'
            })
        })
        .filter(|token| !token.is_empty())
        .map(str::to_ascii_uppercase)
        .collect();
    if tokens.is_empty() {
        return Err(RadioError::Protocol(format!(
            "no tokens returned for {query}"
        )));
    }
    Ok(tokens)
}

fn parse_capability_range(dump: &str, line_prefix: &str, name: &str) -> Option<(f64, f64)> {
    let line = dump.lines().find(|line| line.starts_with(line_prefix))?;
    let marker = format!("{name}(");
    let token = line
        .split_whitespace()
        .find(|token| token.starts_with(&marker))?;
    let range = token.strip_prefix(&marker)?.split_once(')')?.0;
    let (bounds, _) = range.split_once('/')?;
    let (minimum, maximum) = bounds.split_once("..")?;
    Some((minimum.parse().ok()?, maximum.parse().ok()?))
}

fn catalog_tokens(dump: &str, prefix: &str) -> Vec<String> {
    dump.lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(|line| {
            line.split_whitespace()
                .map(|token| token.split('(').next().unwrap_or(token))
                .map(|token| token.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '_'))
                .filter(|token| !token.is_empty())
                .map(str::to_ascii_uppercase)
                .collect()
        })
        .unwrap_or_default()
}

fn capability_flag(dump: &str, capability: &str) -> bool {
    dump.lines().any(|line| {
        line.strip_prefix(capability)
            .and_then(|tail| tail.split_once(':'))
            .is_some_and(|(_, value)| value.trim() == "Y")
    })
}

#[derive(Debug, thiserror::Error)]
/// Radio operation errors. MCP maps each variant to a caller-visible status.
pub enum RadioError {
    /// The backend could not be reached or timed out.
    #[error("RADIO_UNAVAILABLE: {0}")]
    Unavailable(String),
    /// The backend does not advertise or implement the requested operation.
    #[error("CAPABILITY_UNSUPPORTED: {0}")]
    Unsupported(String),
    /// Local station policy or request validation denied the operation.
    #[error("POLICY_DENIED: {0}")]
    PolicyDenied(String),
    /// A write completed but required readback did not match or was unavailable.
    #[error("READBACK_MISMATCH: {0}")]
    ReadbackMismatch(String),
    /// Backend response was malformed or reported a protocol error.
    #[error("invalid radio response: {0}")]
    Protocol(String),
}

#[async_trait]
/// Backend-independent async radio contract used by the MCP adapter.
///
/// Implementations must tolerate calls from concurrent MCP requests and
/// serialize backend operations where the underlying protocol requires it.
pub trait RadioControlPort: Send + Sync {
    /// Discover backend support and the effective local read/write policy.
    async fn capabilities(&self) -> Result<RadioCapabilities, RadioError>;
    /// Read a best-effort common state snapshot with per-field errors.
    async fn state(&self) -> Result<RadioState, RadioError>;
    /// Read one control identified by its exact descriptor ID.
    async fn control(&self, control_id: &str) -> Result<ControlValue, RadioError>;
    /// Request a typed write; implementations validate policy and value, then
    /// return resulting state only after their supported readback checks.
    async fn set_control(
        &self,
        control_id: &str,
        value: ControlValue,
        expected_revision: Option<u64>,
    ) -> Result<RadioState, RadioError>;
    /// Request key-up or release. Current adapters deny key-up; callers must
    /// not treat this API as transmit authorization.
    async fn request_ptt(&self, request: PttRequest) -> Result<PttObservation, RadioError>;
}

#[derive(Clone)]
/// Hamlib `rigctld` TCP backend with bounded requests and serialized CAT access.
///
/// Write limits and enablement are read once from the process environment by
/// [`RigctldRadio::from_env`]. The revision counter is local to this instance.
pub struct RigctldRadio {
    address: String,
    timeout: Duration,
    command_lock: Arc<Mutex<()>>,
    revision: Arc<Mutex<u64>>,
    writes_enabled: bool,
    min_frequency_hz: Option<u64>,
    max_frequency_hz: Option<u64>,
    max_power_fraction: Option<f64>,
    allowed_modes: Vec<String>,
    enabled_hamlib_controls: Vec<String>,
}

impl RigctldRadio {
    /// Build a backend from `LINKWISE_RIGCTLD_ADDR`, timeout, write enablement,
    /// frequency/power limits, allowed modes, and enabled-control settings.
    /// Invalid/missing optional values fall back to disabled/empty settings.
    pub fn from_env() -> Self {
        Self {
            address: env::var("LINKWISE_RIGCTLD_ADDR")
                .unwrap_or_else(|_| DEFAULT_RIGCTLD_ADDR.into()),
            timeout: Duration::from_millis(
                env::var("LINKWISE_RIGCTLD_TIMEOUT_MS")
                    .ok()
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(DEFAULT_TIMEOUT_MS),
            ),
            command_lock: Arc::new(Mutex::new(())),
            revision: Arc::new(Mutex::new(0)),
            writes_enabled: env::var("LINKWISE_RADIO_WRITES")
                .is_ok_and(|v| v.eq_ignore_ascii_case("true")),
            min_frequency_hz: env::var("LINKWISE_MIN_FREQ_HZ")
                .ok()
                .and_then(|v| v.parse().ok()),
            max_frequency_hz: env::var("LINKWISE_MAX_FREQ_HZ")
                .ok()
                .and_then(|v| v.parse().ok()),
            max_power_fraction: env::var("LINKWISE_MAX_POWER_FRACTION")
                .ok()
                .and_then(|v| v.parse::<f64>().ok()),
            allowed_modes: env::var("LINKWISE_ALLOWED_MODES")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_ascii_uppercase)
                .collect(),
            enabled_hamlib_controls: env::var("LINKWISE_ENABLED_HAMLIB_CONTROLS")
                .unwrap_or_default()
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_ascii_uppercase)
                .collect(),
        }
    }

    async fn command(&self, command: &str, multi_line: bool) -> Result<Vec<String>, RadioError> {
        let _guard = self.command_lock.lock().await;
        self.command_unlocked(command, multi_line).await
    }

    async fn command_unlocked(
        &self,
        command: &str,
        multi_line: bool,
    ) -> Result<Vec<String>, RadioError> {
        // Caller must hold `command_lock`; keeping this helper lock-free lets a
        // multi-command write/readback sequence stay serialized as one unit.
        let operation = async {
            let mut stream = TcpStream::connect(&self.address)
                .await
                .map_err(|e| RadioError::Unavailable(e.to_string()))?;
            stream
                .write_all(format!("{command}\n").as_bytes())
                .await
                .map_err(|e| RadioError::Unavailable(e.to_string()))?;
            stream
                .flush()
                .await
                .map_err(|e| RadioError::Unavailable(e.to_string()))?;
            let mut reader = BufReader::new(stream);
            let mut lines = Vec::new();
            loop {
                let mut line = String::new();
                let read = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|e| RadioError::Unavailable(e.to_string()))?;
                if read == 0 {
                    break;
                }
                let line = line.trim().to_owned();
                if line.is_empty() {
                    continue;
                }
                if line.starts_with("RPRT ") {
                    if line != "RPRT 0" {
                        return Err(RadioError::Protocol(format!("{command}: {line}")));
                    }
                    break;
                }
                lines.push(line);
                if !multi_line {
                    break;
                }
            }
            if lines.is_empty() {
                return Err(RadioError::Protocol(format!("empty response to {command}")));
            }
            Ok(lines)
        };
        timeout(self.timeout, operation)
            .await
            .map_err(|_| RadioError::Unavailable(format!("{command} timed out")))?
    }

    async fn scalar(&self, command: &str) -> Result<String, RadioError> {
        self.command(command, false)
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| RadioError::Protocol(format!("empty response to {command}")))
    }

    async fn read_state(&self) -> RadioState {
        let mut errors = BTreeMap::new();
        let info = self.scalar("\\get_info").await.ok();
        let frequency_hz = match self.scalar("f").await {
            Ok(value) => value.parse().ok().or_else(|| {
                errors.insert("frequency_hz".into(), "invalid frequency".into());
                None
            }),
            Err(error) => {
                errors.insert("frequency_hz".into(), error.to_string());
                None
            }
        };
        let (mode, passband_hz) = match self.command("m", false).await {
            Ok(values) => {
                let mode = values.first().cloned();
                let passband = values.get(1).and_then(|v| v.parse().ok());
                if mode.is_none() {
                    errors.insert("mode".into(), "missing mode".into());
                }
                (mode, passband)
            }
            Err(error) => {
                errors.insert("mode".into(), error.to_string());
                (None, None)
            }
        };
        let ptt_asserted = match self.scalar("t").await {
            Ok(value) => value.parse::<u8>().ok().map(|v| v != 0),
            Err(error) => {
                errors.insert("ptt_asserted".into(), error.to_string());
                None
            }
        };
        let rf_power_fraction = match self.scalar("l RFPOWER").await {
            Ok(value) => value.parse().ok(),
            Err(error) => {
                errors.insert("rf_power_fraction".into(), error.to_string());
                None
            }
        };
        let revision = *self.revision.lock().await;
        RadioState {
            radio_info: info,
            frequency_hz,
            mode,
            passband_hz,
            ptt_asserted,
            rf_power_fraction,
            observed_at_utc: chrono::Utc::now().to_rfc3339(),
            state_revision: revision,
            field_errors: errors,
        }
    }

    fn parse_generic_id(control_id: &str) -> Option<(&'static str, String)> {
        let (kind, name) = control_id.split_once(':')?;
        let name = name.to_ascii_uppercase();
        let safe = !name.is_empty()
            && name
                .bytes()
                .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_');
        if !safe {
            return None;
        }
        match kind {
            "level" => Some(("level", name)),
            "function" => Some(("function", name)),
            "parameter" => Some(("parameter", name)),
            _ => None,
        }
    }

    async fn discovered_tokens(&self, query: &str) -> Result<Vec<String>, RadioError> {
        let _guard = self.command_lock.lock().await;
        self.discovered_tokens_unlocked(query).await
    }

    async fn discovered_tokens_unlocked(&self, query: &str) -> Result<Vec<String>, RadioError> {
        let line = self.command_unlocked(query, false).await?.join(" ");
        parse_discovered_tokens(query, &line)
    }

    fn require_writes_enabled(&self) -> Result<(), RadioError> {
        if self.writes_enabled {
            Ok(())
        } else {
            Err(RadioError::PolicyDenied("CAT writes disabled; set LINKWISE_RADIO_WRITES=true only with a qualified station profile".into()))
        }
    }

    fn control_enabled_by_profile(&self, control_id: &str) -> bool {
        self.enabled_hamlib_controls
            .iter()
            .any(|enabled| enabled.eq_ignore_ascii_case(control_id))
    }
}

#[async_trait]
impl RadioControlPort for RigctldRadio {
    async fn capabilities(&self) -> Result<RadioCapabilities, RadioError> {
        let lines = self.command("1", true).await?;
        let dump = lines.join("\n");
        let mut controls = Vec::new();
        for (id, description) in [
            ("frequency_hz", "frequency get/set"),
            ("mode", "mode and passband get/set"),
            ("rf_power_fraction", "normalized RF power level get/set"),
            ("ptt", "PTT state observation; key-up is managed separately"),
        ] {
            let readable = match id {
                "frequency_hz" => capability_flag(&dump, "Can get Frequency"),
                "mode" => capability_flag(&dump, "Can get Mode"),
                "rf_power_fraction" => {
                    catalog_tokens(&dump, "Get level:").contains(&"RFPOWER".into())
                }
                _ => capability_flag(&dump, "Can get PTT"),
            };
            let profile_allows_write = match id {
                "frequency_hz" => {
                    self.min_frequency_hz.is_some() && self.max_frequency_hz.is_some()
                }
                "mode" => !self.allowed_modes.is_empty(),
                "rf_power_fraction" => self.max_power_fraction.is_some(),
                _ => false,
            };
            let enum_values = if id == "mode" {
                catalog_tokens(&dump, "Mode list:")
            } else {
                Vec::new()
            };
            controls.push(ControlDescriptor {
                control_id: id.into(),
                supported: readable,
                readable,
                writable: readable && id != "ptt" && self.writes_enabled && profile_allows_write,
                access_class: if id == "ptt" {
                    "ptt_arbiter_only".into()
                } else if readable && self.writes_enabled && profile_allows_write {
                    "radio_control".into()
                } else {
                    "read_only".into()
                },
                description: description.into(),
                value_type: match id {
                    "frequency_hz" => "frequency_hz",
                    "mode" => "mode",
                    "rf_power_fraction" => "fraction",
                    _ => "boolean",
                }
                .into(),
                minimum: if id == "rf_power_fraction" {
                    parse_capability_range(&dump, "Set level:", "RFPOWER").map(|range| range.0)
                } else if id == "frequency_hz" {
                    self.min_frequency_hz.map(|value| value as f64)
                } else {
                    None
                },
                maximum: if id == "frequency_hz" {
                    self.max_frequency_hz.map(|value| value as f64)
                } else if id == "rf_power_fraction" {
                    match (
                        self.max_power_fraction,
                        parse_capability_range(&dump, "Set level:", "RFPOWER"),
                    ) {
                        (Some(profile_max), Some((_, backend_max))) => {
                            Some(profile_max.min(backend_max))
                        }
                        (Some(profile_max), None) => Some(profile_max),
                        _ => None,
                    }
                } else {
                    None
                },
                enum_values,
                side_effect_class: if id == "ptt" {
                    "transmit"
                } else {
                    "operational"
                }
                .into(),
            });
        }
        for (id, description) in [
            ("vfo", "Current VFO"),
            ("passband_hz", "Mode filter/passband width"),
            ("split_vfo", "Split enablement and TX VFO"),
            ("antenna_selection", "RX and TX antenna selectors"),
            ("memory_channel", "Current memory channel"),
            ("rit_hz", "Receiver incremental tuning offset"),
            ("xit_hz", "Transmitter incremental tuning offset"),
            ("split_frequency_hz", "Split transmit frequency"),
            ("split_mode", "Split transmit mode and passband"),
        ] {
            let (get_cap, set_cap) = match id {
                "vfo" => ("Can get VFO", "Can set VFO"),
                "passband_hz" => ("Can get Mode", "Can set Mode"),
                "split_vfo" => ("Can get Split VFO", "Can set Split VFO"),
                "antenna_selection" => ("Can get Ant", "Can set Ant"),
                "memory_channel" => ("Can get Mem", "Can set Mem"),
                "rit_hz" => ("Can get RIT", "Can set RIT"),
                "xit_hz" => ("Can get XIT", "Can set XIT"),
                "split_frequency_hz" => ("Can get Split Freq", "Can set Split Freq"),
                _ => ("Can get Split Mode", "Can set Split Mode"),
            };
            let readable = capability_flag(&dump, get_cap);
            let backend_settable = capability_flag(&dump, set_cap);
            let enum_values = if id == "vfo" {
                catalog_tokens(&dump, "VFO list:")
            } else {
                Vec::new()
            };
            controls.push(ControlDescriptor {
                control_id: id.into(),
                supported: readable,
                readable,
                writable: backend_settable
                    && self.writes_enabled
                    && self.control_enabled_by_profile(id),
                access_class: if backend_settable
                    && self.writes_enabled
                    && self.control_enabled_by_profile(id)
                {
                    "radio_control"
                } else {
                    "read_only"
                }
                .into(),
                description: description.into(),
                value_type: match id {
                    "vfo" => "vfo",
                    "passband_hz" => "frequency_hz",
                    "split_vfo" => "split_vfo",
                    "antenna_selection" => "antenna_selection",
                    "memory_channel" => "integer",
                    "rit_hz" | "xit_hz" => "signed_hertz",
                    "split_frequency_hz" => "frequency_hz",
                    "split_mode" => "mode_passband",
                    _ => "unknown",
                }
                .into(),
                minimum: None,
                maximum: None,
                enum_values,
                side_effect_class: if id == "memory_channel" {
                    "disruptive"
                } else {
                    "operational"
                }
                .into(),
            });
        }
        for kind in ["level", "function", "parameter"] {
            let (get_prefix, set_prefix) = match kind {
                "level" => ("Get level:", "Set level:"),
                "function" => ("Get functions:", "Set functions:"),
                _ => ("Get parameters:", "Set parameters:"),
            };
            let get_names = catalog_tokens(&dump, get_prefix);
            let set_names = catalog_tokens(&dump, set_prefix);
            let names: std::collections::BTreeSet<String> =
                get_names.iter().chain(set_names.iter()).cloned().collect();
            for name in names {
                let control_id = format!("{kind}:{name}");
                let enabled = self.control_enabled_by_profile(&control_id);
                let readable = get_names.contains(&name);
                let settable = set_names.contains(&name);
                let rf_triggering_function = kind == "function"
                    && matches!(
                        name.as_str(),
                        "SEND_MORSE" | "SEND_VOICE_MEM" | "TBURST" | "TUNER" | "TUNING"
                    );
                let writable = settable
                    && enabled
                    && self.writes_enabled
                    && kind != "parameter"
                    && !rf_triggering_function
                    && !(kind == "level" && name == "RFPOWER" && self.max_power_fraction.is_none());
                let range_line = match kind {
                    "level" if settable => Some("Set level:"),
                    "level" => Some("Get level:"),
                    "parameter" => Some("Get parameters:"),
                    _ => None,
                };
                let numeric_range =
                    range_line.and_then(|prefix| parse_capability_range(&dump, prefix, &name));
                let (minimum, maximum) = if kind == "level" && name == "RFPOWER" {
                    (
                        numeric_range.map(|range| range.0),
                        numeric_range
                            .zip(self.max_power_fraction)
                            .map(|(range, profile_max)| range.1.min(profile_max)),
                    )
                } else {
                    (
                        numeric_range.map(|range| range.0),
                        numeric_range.map(|range| range.1),
                    )
                };
                controls.push(ControlDescriptor {
                    control_id,
                    supported: readable || settable,
                    readable,
                    writable,
                    access_class: if writable {
                        "radio_control"
                    } else {
                        "read_only"
                    }
                    .into(),
                    description: format!("Hamlib {kind} {name}"),
                    value_type: match kind {
                        "function" => "boolean",
                        _ => "number",
                    }
                    .into(),
                    minimum,
                    maximum,
                    enum_values: if kind == "function" {
                        vec!["false".into(), "true".into()]
                    } else {
                        Vec::new()
                    },
                    side_effect_class: if rf_triggering_function {
                        "transmit"
                    } else if kind == "level" && name == "RFPOWER" {
                        "transmit_power"
                    } else if kind == "parameter" {
                        "service_level"
                    } else {
                        "operational"
                    }
                    .into(),
                });
            }
        }
        Ok(RadioCapabilities {
            backend_dump: dump,
            controls,
            observed_at_utc: chrono::Utc::now().to_rfc3339(),
        })
    }

    async fn state(&self) -> Result<RadioState, RadioError> {
        let state = self.read_state().await;
        if state.frequency_hz.is_none() && state.mode.is_none() {
            return Err(RadioError::Unavailable(
                "no radio state fields could be read".into(),
            ));
        }
        Ok(state)
    }

    async fn control(&self, control_id: &str) -> Result<ControlValue, RadioError> {
        match control_id {
            "frequency_hz" => self
                .scalar("f")
                .await?
                .parse::<u64>()
                .map(ControlValue::FrequencyHz)
                .map_err(|e| RadioError::Protocol(e.to_string())),
            "vfo" => self.scalar("v").await.map(ControlValue::Text),
            "mode" => self
                .command("m", false)
                .await?
                .first()
                .cloned()
                .map(ControlValue::Mode)
                .ok_or_else(|| RadioError::Protocol("mode missing".into())),
            "passband_hz" => self
                .command("m", false)
                .await?
                .get(1)
                .ok_or_else(|| RadioError::Protocol("passband missing".into()))?
                .parse::<u32>()
                .map(ControlValue::PassbandHz)
                .map_err(|error| RadioError::Protocol(error.to_string())),
            "split_vfo" => {
                let values = self.command("s", true).await?;
                let enabled = values
                    .first()
                    .ok_or_else(|| RadioError::Protocol("split state missing".into()))?
                    .parse::<u8>()
                    .map_err(|error| RadioError::Protocol(error.to_string()))?
                    != 0;
                let tx_vfo = values
                    .get(1)
                    .ok_or_else(|| RadioError::Protocol("split TX VFO missing".into()))?
                    .clone();
                Ok(ControlValue::SplitVfo { enabled, tx_vfo })
            }
            "antenna_selection" => self
                .scalar("y")
                .await?
                .parse::<u32>()
                .map(ControlValue::AntennaIndex)
                .map_err(|error| RadioError::Protocol(error.to_string())),
            "memory_channel" => self
                .scalar("e")
                .await?
                .parse::<u64>()
                .map(ControlValue::Integer)
                .map_err(|error| RadioError::Protocol(error.to_string())),
            "rit_hz" => self
                .scalar("j")
                .await?
                .parse::<i64>()
                .map(ControlValue::SignedHertz)
                .map_err(|error| RadioError::Protocol(error.to_string())),
            "xit_hz" => self
                .scalar("z")
                .await?
                .parse::<i64>()
                .map(ControlValue::SignedHertz)
                .map_err(|error| RadioError::Protocol(error.to_string())),
            "split_frequency_hz" => self
                .scalar("i")
                .await?
                .parse::<u64>()
                .map(ControlValue::FrequencyHz)
                .map_err(|error| RadioError::Protocol(error.to_string())),
            "split_mode" => {
                let values = self.command("x", true).await?;
                let mode = values
                    .first()
                    .ok_or_else(|| RadioError::Protocol("split mode missing".into()))?
                    .clone();
                let passband_hz = values
                    .get(1)
                    .ok_or_else(|| RadioError::Protocol("split passband missing".into()))?
                    .parse::<u32>()
                    .map_err(|error| RadioError::Protocol(error.to_string()))?;
                Ok(ControlValue::ModePassband { mode, passband_hz })
            }
            "rf_power_fraction" => self
                .scalar("l RFPOWER")
                .await?
                .parse::<f64>()
                .map(ControlValue::RfPowerFraction)
                .map_err(|e| RadioError::Protocol(e.to_string())),
            "ptt" => self
                .scalar("t")
                .await?
                .parse::<u8>()
                .map(|value| ControlValue::PttState(value != 0))
                .map_err(|e| RadioError::Protocol(e.to_string())),
            other => {
                let (kind, name) = Self::parse_generic_id(other)
                    .ok_or_else(|| RadioError::Unsupported(other.into()))?;
                let query = match kind {
                    "level" => "l ?",
                    "function" => "u ?",
                    "parameter" => "p ?",
                    _ => return Err(RadioError::Unsupported(other.into())),
                };
                if !self.discovered_tokens(query).await?.contains(&name) {
                    return Err(RadioError::Unsupported(other.into()));
                }
                if kind == "level" || kind == "parameter" {
                    let command = if kind == "level" { "l" } else { "p" };
                    self.scalar(&format!("{command} {name}"))
                        .await?
                        .parse::<f64>()
                        .map(ControlValue::Number)
                        .map_err(|e| RadioError::Protocol(e.to_string()))
                } else {
                    self.scalar(&format!("u {name}"))
                        .await?
                        .parse::<u8>()
                        .map(|value| ControlValue::Boolean(value != 0))
                        .map_err(|e| RadioError::Protocol(e.to_string()))
                }
            }
        }
    }

    async fn set_control(
        &self,
        control_id: &str,
        value: ControlValue,
        expected_revision: Option<u64>,
    ) -> Result<RadioState, RadioError> {
        self.require_writes_enabled()?;
        if !matches!(
            control_id,
            "frequency_hz" | "mode" | "rf_power_fraction" | "ptt"
        ) && !self.control_enabled_by_profile(control_id)
        {
            return Err(RadioError::PolicyDenied(format!(
                "{control_id} is not enabled in LINKWISE_ENABLED_HAMLIB_CONTROLS"
            )));
        }
        let _guard = self.command_lock.lock().await;
        let current_revision = *self.revision.lock().await;
        if expected_revision.is_some_and(|expected| expected != current_revision) {
            return Err(RadioError::PolicyDenied(format!(
                "stale state revision; current revision is {current_revision}"
            )));
        }
        let ptt = self
            .command_unlocked("t", false)
            .await?
            .first()
            .cloned()
            .ok_or_else(|| RadioError::Protocol("PTT state missing".into()))?;
        if ptt.trim() != "0" {
            return Err(RadioError::PolicyDenied(
                "radio is transmitting; control write denied".into(),
            ));
        }
        let expected_value = value.clone();
        let command = match (control_id, value) {
            ("frequency_hz", ControlValue::FrequencyHz(hz)) => {
                let (Some(min), Some(max)) = (self.min_frequency_hz, self.max_frequency_hz) else {
                    return Err(RadioError::PolicyDenied("frequency writes require configured LINKWISE_MIN_FREQ_HZ and LINKWISE_MAX_FREQ_HZ".into()));
                };
                if hz < min || hz > max {
                    return Err(RadioError::PolicyDenied(
                        "frequency is outside the installed station profile".into(),
                    ));
                }
                format!("F {hz}")
            }
            ("vfo", ControlValue::Text(vfo)) => {
                let vfo = vfo.trim().to_ascii_uppercase();
                if !vfo.is_empty()
                    && vfo
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                    && self.discovered_tokens_unlocked("V ?").await?.contains(&vfo)
                {
                    format!("V {vfo}")
                } else {
                    return Err(RadioError::Unsupported(format!(
                        "invalid or unsupported VFO {vfo}"
                    )));
                }
            }
            ("mode", ControlValue::Mode(mode)) => {
                let mode = mode.trim().to_ascii_uppercase();
                if self.allowed_modes.is_empty()
                    || !self.allowed_modes.iter().any(|allowed| allowed == &mode)
                {
                    return Err(RadioError::PolicyDenied(
                        "mode is not in LINKWISE_ALLOWED_MODES".into(),
                    ));
                }
                format!("M {mode} 0")
            }
            ("passband_hz", ControlValue::PassbandHz(passband)) => {
                if passband > 100_000 {
                    return Err(RadioError::PolicyDenied(
                        "passband exceeds safety bound".into(),
                    ));
                }
                let mode = self
                    .command_unlocked("m", false)
                    .await?
                    .first()
                    .cloned()
                    .ok_or_else(|| RadioError::Protocol("mode missing".into()))?;
                format!("M {mode} {passband}")
            }
            ("split_vfo", ControlValue::SplitVfo { enabled, tx_vfo }) => {
                let tx_vfo = tx_vfo.trim().to_ascii_uppercase();
                if tx_vfo.is_empty()
                    || !tx_vfo
                        .bytes()
                        .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
                    || !self
                        .discovered_tokens_unlocked("V ?")
                        .await?
                        .contains(&tx_vfo)
                {
                    return Err(RadioError::Unsupported(format!(
                        "invalid or unsupported split TX VFO {tx_vfo}"
                    )));
                }
                if enabled {
                    let (Some(min), Some(max)) = (self.min_frequency_hz, self.max_frequency_hz)
                    else {
                        return Err(RadioError::PolicyDenied(
                            "split mode requires station frequency limits".into(),
                        ));
                    };
                    let tx_frequency = self
                        .command_unlocked("i", false)
                        .await?
                        .first()
                        .ok_or_else(|| RadioError::Protocol("split frequency missing".into()))?
                        .parse::<u64>()
                        .map_err(|error| RadioError::Protocol(error.to_string()))?;
                    if tx_frequency < min || tx_frequency > max {
                        return Err(RadioError::PolicyDenied(
                            "current split transmit frequency is outside the station profile"
                                .into(),
                        ));
                    }
                }
                format!("S {} {tx_vfo}", u8::from(enabled))
            }
            ("antenna_selection", ControlValue::AntennaIndex(antenna)) => format!("Y {antenna}"),
            ("memory_channel", ControlValue::Integer(channel)) => {
                if channel > i32::MAX as u64 {
                    return Err(RadioError::PolicyDenied(
                        "memory channel out of range".into(),
                    ));
                }
                format!("E {channel}")
            }
            ("rit_hz", ControlValue::SignedHertz(offset)) => {
                if offset.unsigned_abs() > 100_000 {
                    return Err(RadioError::PolicyDenied(
                        "RIT offset exceeds 100 kHz".into(),
                    ));
                }
                format!("J {offset}")
            }
            ("xit_hz", ControlValue::SignedHertz(offset)) => {
                if offset.unsigned_abs() > 100_000 {
                    return Err(RadioError::PolicyDenied(
                        "XIT offset exceeds 100 kHz".into(),
                    ));
                }
                format!("Z {offset}")
            }
            ("split_frequency_hz", ControlValue::FrequencyHz(hz)) => {
                let (Some(min), Some(max)) = (self.min_frequency_hz, self.max_frequency_hz) else {
                    return Err(RadioError::PolicyDenied(
                        "split frequency writes require station frequency limits".into(),
                    ));
                };
                if hz < min || hz > max {
                    return Err(RadioError::PolicyDenied(
                        "split frequency is outside the installed station profile".into(),
                    ));
                }
                format!("I {hz}")
            }
            ("split_mode", ControlValue::ModePassband { mode, passband_hz }) => {
                let mode = mode.trim().to_ascii_uppercase();
                if self.allowed_modes.is_empty()
                    || !self.allowed_modes.iter().any(|allowed| allowed == &mode)
                    || passband_hz > 100_000
                {
                    return Err(RadioError::PolicyDenied(
                        "split mode or passband is outside the installed station profile".into(),
                    ));
                }
                format!("X {mode} {passband_hz}")
            }
            ("rf_power_fraction", ControlValue::RfPowerFraction(value)) => {
                let Some(max) = self.max_power_fraction else {
                    return Err(RadioError::PolicyDenied(
                        "power writes require LINKWISE_MAX_POWER_FRACTION".into(),
                    ));
                };
                let dump = self.command_unlocked("1", true).await?.join("\n");
                let (backend_min, backend_max) =
                    parse_capability_range(&dump, "Set level:", "RFPOWER").unwrap_or((0.0, 1.0));
                if !value.is_finite()
                    || !(backend_min..=backend_max.min(max).min(1.0)).contains(&value)
                {
                    return Err(RadioError::PolicyDenied(
                        "power fraction exceeds the installed station profile".into(),
                    ));
                }
                format!("L RFPOWER {value}")
            }
            (other, ControlValue::Number(value)) => {
                let (_, name) = Self::parse_generic_id(other)
                    .filter(|(kind, _)| *kind == "level")
                    .ok_or_else(|| RadioError::Unsupported(other.into()))?;
                let query = "L ?";
                if !self
                    .discovered_tokens_unlocked(query)
                    .await?
                    .contains(&name)
                {
                    return Err(RadioError::Unsupported(other.into()));
                }
                if !value.is_finite() {
                    return Err(RadioError::PolicyDenied("non-finite level value".into()));
                }
                let dump = self.command_unlocked("1", true).await?.join("\n");
                let Some((minimum, maximum)) = parse_capability_range(&dump, "Set level:", &name)
                else {
                    return Err(RadioError::PolicyDenied(format!(
                        "Hamlib did not report a writable numeric range for level {name}"
                    )));
                };
                if value < minimum || value > maximum {
                    return Err(RadioError::PolicyDenied(format!(
                        "{name} must be within Hamlib range {minimum}..={maximum}"
                    )));
                }
                if name == "RFPOWER" {
                    let Some(max) = self.max_power_fraction else {
                        return Err(RadioError::PolicyDenied(
                            "RFPOWER writes require LINKWISE_MAX_POWER_FRACTION".into(),
                        ));
                    };
                    if !(0.0..=max.min(1.0)).contains(&value) {
                        return Err(RadioError::PolicyDenied(
                            "RFPOWER value exceeds the installed station profile".into(),
                        ));
                    }
                }
                format!("L {name} {value}")
            }
            (other, ControlValue::Boolean(value)) => {
                let (_, name) = Self::parse_generic_id(other)
                    .filter(|(kind, _)| *kind == "function")
                    .ok_or_else(|| RadioError::Unsupported(other.into()))?;
                if matches!(
                    name.as_str(),
                    "SEND_MORSE" | "SEND_VOICE_MEM" | "TBURST" | "TUNER" | "TUNING"
                ) {
                    return Err(RadioError::PolicyDenied(
                        "potentially transmit-triggering Hamlib function is not directly callable"
                            .into(),
                    ));
                }
                if !self
                    .discovered_tokens_unlocked("U ?")
                    .await?
                    .contains(&name)
                {
                    return Err(RadioError::Unsupported(other.into()));
                }
                format!("U {name} {}", u8::from(value))
            }
            ("ptt", _) => {
                return Err(RadioError::PolicyDenied(
                    "PTT writes are available only through the PTT arbiter".into(),
                ));
            }
            (other, _) => return Err(RadioError::Unsupported(other.into())),
        };

        // The lock is held across write/readback so this adapter never interleaves CAT mutations.
        let mut stream = timeout(self.timeout, TcpStream::connect(&self.address))
            .await
            .map_err(|_| RadioError::Unavailable("rigctld connect timed out".into()))?
            .map_err(|e| RadioError::Unavailable(e.to_string()))?;
        stream
            .write_all(format!("{command}\n").as_bytes())
            .await
            .map_err(|e| RadioError::Unavailable(e.to_string()))?;
        stream
            .flush()
            .await
            .map_err(|e| RadioError::Unavailable(e.to_string()))?;
        let mut reader = BufReader::new(stream);
        let mut response = String::new();
        timeout(self.timeout, reader.read_line(&mut response))
            .await
            .map_err(|_| RadioError::Unavailable("rigctld write timed out".into()))?
            .map_err(|e| RadioError::Unavailable(e.to_string()))?;
        if response.trim() != "RPRT 0" {
            return Err(RadioError::Protocol(format!(
                "write returned {}",
                response.trim()
            )));
        }
        *self.revision.lock().await = current_revision.saturating_add(1);
        drop(_guard);
        let actual = self.control(control_id).await?;
        let matches = match (&expected_value, &actual) {
            (ControlValue::RfPowerFraction(expected), ControlValue::RfPowerFraction(actual)) => {
                (expected - actual).abs() <= 0.01
            }
            (expected, actual) => expected == actual,
        };
        if !matches {
            return Err(RadioError::ReadbackMismatch(format!(
                "requested {expected_value:?}, read back {actual:?}"
            )));
        }
        let state = self.read_state().await;
        if state.field_errors.contains_key(control_id) {
            return Err(RadioError::ReadbackMismatch(format!(
                "could not verify {control_id}"
            )));
        }
        Ok(state)
    }

    async fn request_ptt(&self, request: PttRequest) -> Result<PttObservation, RadioError> {
        if request.action == PttAction::KeyUp {
            return Err(RadioError::PolicyDenied(
                "PTT key-up is unavailable until the session authorization and one-use permit arbiter is configured".into(),
            ));
        }
        let _guard = self.command_lock.lock().await;
        let operation = async {
            let mut stream = TcpStream::connect(&self.address)
                .await
                .map_err(|error| RadioError::Unavailable(error.to_string()))?;
            stream
                .write_all(b"T 0\n")
                .await
                .map_err(|error| RadioError::Unavailable(error.to_string()))?;
            stream
                .flush()
                .await
                .map_err(|error| RadioError::Unavailable(error.to_string()))?;
            let mut response = String::new();
            BufReader::new(stream)
                .read_line(&mut response)
                .await
                .map_err(|error| RadioError::Unavailable(error.to_string()))?;
            if response.trim() != "RPRT 0" {
                return Err(RadioError::Protocol(format!(
                    "PTT release returned {}",
                    response.trim()
                )));
            }
            Ok(())
        };
        timeout(self.timeout, operation)
            .await
            .map_err(|_| RadioError::Unavailable("PTT release timed out".into()))??;
        drop(_guard);
        let observed = self
            .control("ptt")
            .await
            .ok()
            .and_then(|value| match value {
                ControlValue::PttState(value) => Some(value),
                _ => None,
            });
        Ok(PttObservation {
            requested_action: PttAction::KeyDown,
            observed_ptt_asserted: observed,
            release_confirmed: observed == Some(false),
            observed_at_utc: chrono::Utc::now().to_rfc3339(),
        })
    }
}

/// No-device backend used by Compose development and simulation profiles.
/// Values are mutable in memory; PTT key-up is never allowed.
/// In-memory radio backend for MCP development; it does not model rigctld or RF.
#[derive(Clone)]
pub struct SimulatedRadio {
    state: Arc<Mutex<RadioState>>,
}

impl SimulatedRadio {
    pub fn from_env() -> Self {
        Self {
            state: Arc::new(Mutex::new(RadioState {
                radio_info: Some("Linkwise simulated radio (no hardware connection)".into()),
                frequency_hz: Some(28_120_000),
                mode: Some("USB".into()),
                passband_hz: Some(2_400),
                ptt_asserted: Some(false),
                rf_power_fraction: Some(0.0),
                observed_at_utc: chrono::Utc::now().to_rfc3339(),
                state_revision: 0,
                field_errors: BTreeMap::new(),
            })),
        }
    }
}

#[async_trait]
impl RadioControlPort for SimulatedRadio {
    async fn capabilities(&self) -> Result<RadioCapabilities, RadioError> {
        Ok(RadioCapabilities {
            backend_dump: "Linkwise simulated radio (no hardware connection)".into(),
            controls: vec![
                ControlDescriptor {
                    control_id: "frequency_hz".into(),
                    supported: true,
                    readable: true,
                    writable: true,
                    access_class: "simulation_only".into(),
                    description: "Simulated VFO frequency".into(),
                    value_type: "frequency_hz".into(),
                    minimum: None,
                    maximum: None,
                    enum_values: Vec::new(),
                    side_effect_class: "simulation_only".into(),
                },
                ControlDescriptor {
                    control_id: "mode".into(),
                    supported: true,
                    readable: true,
                    writable: true,
                    access_class: "simulation_only".into(),
                    description: "Simulated operating mode".into(),
                    value_type: "mode".into(),
                    minimum: None,
                    maximum: None,
                    enum_values: Vec::new(),
                    side_effect_class: "simulation_only".into(),
                },
                ControlDescriptor {
                    control_id: "rf_power_fraction".into(),
                    supported: true,
                    readable: true,
                    writable: true,
                    access_class: "simulation_only".into(),
                    description: "Simulated normalized RF power".into(),
                    value_type: "fraction".into(),
                    minimum: Some(0.0),
                    maximum: Some(1.0),
                    enum_values: Vec::new(),
                    side_effect_class: "simulation_only".into(),
                },
                ControlDescriptor {
                    control_id: "ptt".into(),
                    supported: true,
                    readable: true,
                    writable: false,
                    access_class: "ptt_arbiter_only".into(),
                    description: "PTT state (always receive in simulation)".into(),
                    value_type: "boolean".into(),
                    minimum: None,
                    maximum: None,
                    enum_values: vec!["false".into(), "true".into()],
                    side_effect_class: "ptt_arbiter_only".into(),
                },
            ],
            observed_at_utc: chrono::Utc::now().to_rfc3339(),
        })
    }
    async fn state(&self) -> Result<RadioState, RadioError> {
        Ok(self.state.lock().await.clone())
    }
    async fn control(&self, control_id: &str) -> Result<ControlValue, RadioError> {
        let state = self.state.lock().await;
        match control_id {
            "frequency_hz" => state
                .frequency_hz
                .map(ControlValue::FrequencyHz)
                .ok_or_else(|| RadioError::Unavailable("simulated frequency unavailable".into())),
            "mode" => state
                .mode
                .clone()
                .map(ControlValue::Mode)
                .ok_or_else(|| RadioError::Unavailable("simulated mode unavailable".into())),
            "rf_power_fraction" => state
                .rf_power_fraction
                .map(ControlValue::RfPowerFraction)
                .ok_or_else(|| RadioError::Unavailable("simulated power unavailable".into())),
            "ptt" => Ok(ControlValue::PttState(false)),
            other => Err(RadioError::Unsupported(other.into())),
        }
    }
    async fn set_control(
        &self,
        control_id: &str,
        value: ControlValue,
        expected_revision: Option<u64>,
    ) -> Result<RadioState, RadioError> {
        let mut state = self.state.lock().await;
        if expected_revision.is_some_and(|revision| revision != state.state_revision) {
            return Err(RadioError::PolicyDenied(
                "stale simulated state revision".into(),
            ));
        }
        match (control_id, value) {
            ("frequency_hz", ControlValue::FrequencyHz(hz))
                if (150_000..=1_500_000_000).contains(&hz) =>
            {
                state.frequency_hz = Some(hz)
            }
            ("frequency_hz", ControlValue::FrequencyHz(_)) => {
                return Err(RadioError::PolicyDenied(
                    "simulated frequency must be between 150 kHz and 1.5 GHz".into(),
                ));
            }
            ("mode", ControlValue::Mode(mode))
                if [
                    "AM", "CW", "USB", "LSB", "RTTY", "FM", "WFM", "CWR", "RTTYR",
                ]
                .contains(&mode.trim().to_ascii_uppercase().as_str()) =>
            {
                state.mode = Some(mode.trim().to_ascii_uppercase())
            }
            ("mode", ControlValue::Mode(_)) => {
                return Err(RadioError::PolicyDenied(
                    "unsupported simulated mode".into(),
                ));
            }
            ("rf_power_fraction", ControlValue::RfPowerFraction(power))
                if power.is_finite() && (0.0..=1.0).contains(&power) =>
            {
                state.rf_power_fraction = Some(power)
            }
            ("rf_power_fraction", ControlValue::RfPowerFraction(_)) => {
                return Err(RadioError::PolicyDenied(
                    "simulated power must be between 0 and 1".into(),
                ));
            }
            ("ptt", _) => {
                return Err(RadioError::PolicyDenied(
                    "PTT key-up is unavailable in simulation".into(),
                ));
            }
            (other, _) => return Err(RadioError::Unsupported(other.into())),
        }
        state.state_revision = state.state_revision.saturating_add(1);
        state.observed_at_utc = chrono::Utc::now().to_rfc3339();
        Ok(state.clone())
    }

    async fn request_ptt(&self, request: PttRequest) -> Result<PttObservation, RadioError> {
        if request.action == PttAction::KeyUp {
            return Err(RadioError::PolicyDenied(
                "PTT key-up is disabled in simulation".into(),
            ));
        }
        Ok(PttObservation {
            requested_action: PttAction::KeyDown,
            observed_ptt_asserted: Some(false),
            release_confirmed: true,
            observed_at_utc: chrono::Utc::now().to_rfc3339(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_hamlib_control_ids_are_allowlisted() {
        assert_eq!(
            RigctldRadio::parse_generic_id("level:af"),
            Some(("level", "AF".into()))
        );
        assert_eq!(
            RigctldRadio::parse_generic_id("function:nb"),
            Some(("function", "NB".into()))
        );
        assert_eq!(RigctldRadio::parse_generic_id("level:RAW COMMAND"), None);
        assert_eq!(RigctldRadio::parse_generic_id("vfo:VFOA"), None);
    }

    #[test]
    fn discovered_tokens_are_normalized_without_accepting_empty_responses() {
        assert_eq!(
            parse_discovered_tokens("l ?", "AF, RFPOWER  MICGAIN").unwrap(),
            vec!["AF", "RFPOWER", "MICGAIN"]
        );
        assert!(parse_discovered_tokens("l ?", " \n ").is_err());
    }

    #[test]
    fn hamlib_numeric_ranges_are_parsed_from_dump_caps() {
        let dump =
            "Set level: AF(0..1/0.1) RFPOWER(0.05..1/0.01)\nSet parameters: BACKLIGHT(0..1/0.01)";
        assert_eq!(
            parse_capability_range(dump, "Set level:", "RFPOWER"),
            Some((0.05, 1.0))
        );
        assert_eq!(
            parse_capability_range(dump, "Set parameters:", "BACKLIGHT"),
            Some((0.0, 1.0))
        );
        assert_eq!(parse_capability_range(dump, "Set level:", "UNKNOWN"), None);
    }

    #[test]
    fn dump_caps_catalog_parser_handles_tabs_and_numeric_descriptors() {
        let dump = "Can get Frequency:\tY\nMode list: AM USB\nGet level: RFPOWER(0.05..1/0.01) AF(0..1/0.1)";
        assert!(capability_flag(dump, "Can get Frequency"));
        assert!(!capability_flag(dump, "Can set Frequency"));
        assert_eq!(catalog_tokens(dump, "Mode list:"), vec!["AM", "USB"]);
        assert_eq!(catalog_tokens(dump, "Get level:"), vec!["RFPOWER", "AF"]);
    }

    #[tokio::test]
    async fn simulated_state_is_well_formed_and_controls_are_memory_only() {
        let radio = SimulatedRadio::from_env();
        let state = radio.state().await.expect("simulated state");
        assert_eq!(state.frequency_hz, Some(28_120_000));
        assert_eq!(state.mode.as_deref(), Some("USB"));
        assert_eq!(state.ptt_asserted, Some(false));
        assert!(state.field_errors.is_empty());
        let changed = radio
            .set_control(
                "frequency_hz",
                ControlValue::FrequencyHz(7_100_000),
                Some(0),
            )
            .await
            .expect("simulated writes update in-memory state");
        assert_eq!(changed.frequency_hz, Some(7_100_000));
        assert_eq!(changed.state_revision, 1);
        assert_eq!(
            radio.control("frequency_hz").await.unwrap(),
            ControlValue::FrequencyHz(7_100_000)
        );
        assert!(
            radio
                .set_control(
                    "frequency_hz",
                    ControlValue::FrequencyHz(7_100_000),
                    Some(0)
                )
                .await
                .is_err(),
            "stale revisions are rejected"
        );
    }

    #[tokio::test]
    async fn simulated_ptt_only_allows_release() {
        let radio = SimulatedRadio::from_env();
        let released = radio
            .request_ptt(PttRequest {
                action: PttAction::KeyDown,
                session_id: None,
                transmit_permit_id: None,
            })
            .await
            .expect("key-down should be permitted");
        assert!(released.release_confirmed);
        assert_eq!(released.observed_ptt_asserted, Some(false));

        let keyed_up = radio
            .request_ptt(PttRequest {
                action: PttAction::KeyUp,
                session_id: Some("session".into()),
                transmit_permit_id: Some("permit".into()),
            })
            .await;
        assert!(matches!(keyed_up, Err(RadioError::PolicyDenied(_))));
    }

    #[tokio::test]
    async fn simulated_controls_are_writable_except_ptt() {
        let radio = SimulatedRadio::from_env();
        let capabilities = radio.capabilities().await.expect("capabilities");
        assert!(!capabilities.controls.is_empty());
        assert!(capabilities.controls.iter().all(|control| {
            control.supported
                && control.readable
                && (control.writable == (control.control_id != "ptt"))
        }));
    }
}
