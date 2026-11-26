//! MCP transport and tool mappings for the Linkwise radio-control port.

use std::sync::Arc;
use std::{fs::OpenOptions, io::Write, path::Path};

use rmcp::{
    ServiceExt,
    handler::server::wrapper::{Json, Parameters},
    schemars::JsonSchema,
    tool, tool_router,
    transport::stdio,
};
use serde::Deserialize;

use crate::{
    olm::{self, Olm},
    radio::{ApiResult, ControlValue, PttAction, PttRequest, RadioControlPort, RadioError},
};
use chrono::{TimeDelta, Utc};
use serde_json::Value;
use uuid::Uuid;

#[derive(Debug, Deserialize, JsonSchema)]
/// Request body for a capability-discovered control read.
pub struct GetControlRequest {
    /// Exact `control_id` returned by `radio_get_controls`, for example
    /// `frequency_hz`, `vfo`, or `level:RFPOWER`.
    pub control_id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
/// Request body for a policy-checked typed control write.
pub struct SetControlRequest {
    /// Exact writable `control_id` returned by `radio_get_controls`.
    pub control_id: String,
    /// Value must use the matching tagged ControlValue variant from the
    /// descriptor's `value_type`.
    pub value: ControlValue,
    /// Optional optimistic concurrency check. Copy `state_revision` from the
    /// most recent `radio_get_state` result; a mismatch denies the write.
    pub expected_state_revision: Option<u64>,
}

#[derive(Debug, serde::Serialize, schemars::JsonSchema)]
/// PTT state and route metadata exposed to MCP clients.
pub struct PttState {
    /// Backend route label used for the observation.
    pub route: String,
    /// Requested PTT state, if a request is associated with this observation.
    pub requested_state: Option<bool>,
    /// Observed PTT state; `None` means unknown/unavailable.
    pub observed_state: Option<bool>,
    /// Whether the backend supports state readback.
    pub readback_supported: bool,
    /// Whether an independent release watchdog is active (currently false).
    pub watchdog_active: bool,
    /// UTC RFC 3339 observation timestamp.
    pub observed_at_utc: String,
}

#[derive(Deserialize, JsonSchema)]
struct RankingRequest {
    /// Omit to create a new ranking, or pass an existing ranking ID to page it.
    ranking_id: Option<String>,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct PrepareCandidate {
    ranking_id: String,
    channel_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct GatewayRequest {
    gateway: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize, JsonSchema)]
struct PreparedHandle {
    prepared_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct StartLink {
    prepared_id: String,
    grant_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct AttemptHandle {
    attempt_id: String,
}

#[derive(Deserialize, JsonSchema)]
struct HistoryRequest {
    /// Lookback in seconds, up to 30 days. Defaults to one hour.
    window_seconds: Option<u32>,
    limit: Option<usize>,
}

fn parse_id(value: &str) -> Result<Uuid, olm::Error> {
    Uuid::parse_str(value).map_err(|_| olm::Error::Invalid("invalid operation or ranking UUID"))
}

fn history_start(seconds: Option<u32>) -> chrono::DateTime<Utc> {
    Utc::now() - TimeDelta::seconds(seconds.unwrap_or(3600).clamp(1, 2_592_000).into())
}

fn station_result<T: serde::Serialize>(result: Result<T, olm::Error>) -> Json<ApiResult<Value>> {
    let result = result.and_then(|value| Ok(serde_json::to_value(value)?));
    Json(match result {
        Ok(value) => ApiResult::ok(value),
        Err(error) => {
            let mut response = ApiResult::failed(error.to_string());
            response.status = match error {
                olm::Error::Invalid(_) => "invalid_request",
                olm::Error::Denied(_) => "denied",
                olm::Error::Busy(_) => "busy",
                _ => "failed",
            }
            .into();
            response
        }
    })
}

/// Append one JSON Lines audit record. Callers decide whether audit failure
/// must deny the operation or whether a safety release should proceed.
fn append_audit(record: &serde_json::Value) -> Result<(), String> {
    let path = std::env::var("LINKWISE_AUDIT_PATH")
        .unwrap_or_else(|_| "/var/lib/linkwise/audit.jsonl".into());
    if let Some(parent) = Path::new(&path).parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|error| error.to_string())?;
    writeln!(
        file,
        "{}",
        serde_json::to_string(record).map_err(|error| error.to_string())?
    )
    .map_err(|error| error.to_string())
}

#[derive(Clone)]
pub struct RadioMcp {
    radio: Arc<dyn RadioControlPort>,
    station: Option<Arc<Olm>>,
}

impl RadioMcp {
    pub fn new(radio: Arc<dyn RadioControlPort>) -> Self {
        Self {
            radio,
            station: None,
        }
    }

    fn station(&self) -> Result<&Arc<Olm>, olm::Error> {
        self.station
            .as_ref()
            .ok_or(olm::Error::Denied("station profile is unconfigured"))
    }
}

/// Convert backend error variants to stable MCP status values.
fn error_result<T>(error: RadioError) -> Json<ApiResult<T>> {
    let status = match error {
        RadioError::PolicyDenied(_) => "denied",
        RadioError::Unsupported(_) => "unsupported",
        RadioError::Unavailable(_) => "unavailable",
        RadioError::ReadbackMismatch(_) => "readback_mismatch",
        RadioError::Protocol(_) => "failed",
    };
    let mut result = ApiResult::failed(error.to_string());
    result.status = status.into();
    Json(result)
}

#[tool_router(server_handler)]
impl RadioMcp {
    #[tool(
        annotations(
            title = "Read radio state",
            read_only_hint = true,
            open_world_hint = true
        ),
        description = "Read observed radio state. Returns frequency, mode, PTT, power, timestamp, state_revision, and per-field errors. Use this first to get the current revision; the revision is Linkwise-local and may not detect changes made by another controller."
    )]
    async fn radio_get_state(&self) -> Json<ApiResult<crate::radio::RadioState>> {
        match self.radio.state().await {
            Ok(state) => Json(ApiResult::ok(state)),
            Err(error) => error_result(error),
        }
    }

    #[tool(
        annotations(
            title = "Read radio capabilities",
            read_only_hint = true,
            open_world_hint = true
        ),
        description = "Read the configured backend's raw capability dump and Linkwise control descriptors. Use radio_get_controls for the concise catalog; this call includes the full backend dump for diagnostics."
    )]
    async fn radio_get_capabilities(&self) -> Json<ApiResult<crate::radio::RadioCapabilities>> {
        match self.radio.capabilities().await {
            Ok(value) => Json(ApiResult::ok(value)),
            Err(error) => error_result(error),
        }
    }

    #[tool(
        annotations(
            title = "List radio controls",
            read_only_hint = true,
            open_world_hint = true
        ),
        description = "List backend-discovered radio controls. Check supported, readable, writable, value_type, minimum, maximum, enum_values, access_class, and side_effect_class before selecting a control. Use the returned control_id verbatim with radio_get_control or radio_set_control; writable=false means a set request will be denied."
    )]
    async fn radio_get_controls(&self) -> Json<ApiResult<Vec<crate::radio::ControlDescriptor>>> {
        match self.radio.capabilities().await {
            Ok(value) => Json(ApiResult::ok(value.controls)),
            Err(error) => error_result(error),
        }
    }

    #[tool(
        annotations(
            title = "Read radio control",
            read_only_hint = true,
            open_world_hint = true
        ),
        description = "Read one radio control. First obtain its exact control_id from radio_get_controls. Returns a tagged ControlValue; unsupported controls or controls without a readable value return a non-ok status."
    )]
    async fn radio_get_control(
        &self,
        Parameters(request): Parameters<GetControlRequest>,
    ) -> Json<ApiResult<ControlValue>> {
        match self.radio.control(&request.control_id).await {
            Ok(value) => Json(ApiResult::ok(value)),
            Err(error) => error_result(error),
        }
    }

    #[tool(
        annotations(
            title = "Read PTT state",
            read_only_hint = true,
            open_world_hint = true
        ),
        description = "Read PTT state and route metadata. watchdog_active is currently false; this tool does not authorize transmission."
    )]
    async fn radio_get_ptt_state(&self) -> Json<ApiResult<PttState>> {
        match self.radio.control("ptt").await {
            Ok(ControlValue::PttState(observed_state)) => Json(ApiResult::ok(PttState {
                route: "radio_backend".into(),
                requested_state: None,
                observed_state: Some(observed_state),
                readback_supported: true,
                watchdog_active: false,
                observed_at_utc: chrono::Utc::now().to_rfc3339(),
            })),
            Ok(_) => error_result(RadioError::Protocol(
                "PTT control returned a non-PTT value".into(),
            )),
            Err(error) => error_result(error),
        }
    }

    #[tool(
        annotations(
            title = "Set radio control",
            read_only_hint = false,
            open_world_hint = true
        ),
        description = "Request a typed radio-control write. First inspect radio_get_controls and use an exact writable control_id and matching tagged ControlValue. Writes can be denied by backend support, station limits, disabled CAT writes, missing per-control enablement, PTT state, stale expected_state_revision, or audit failure. Success includes observed state after readback; it does not guarantee another controller has not changed the radio later."
    )]
    async fn radio_set_control(
        &self,
        Parameters(request): Parameters<SetControlRequest>,
    ) -> Json<ApiResult<crate::radio::RadioState>> {
        if self
            .station
            .as_ref()
            .is_some_and(|station| station.is_shadow())
        {
            let mut result = ApiResult::failed("the shadow station owns CAT writes");
            result.status = "denied".into();
            return Json(result);
        }
        let correlation_id = uuid::Uuid::new_v4().to_string();
        let request_record = serde_json::json!({
            "event": "radio_control_requested", "correlation_id": correlation_id.clone(),
            "observed_at_utc": chrono::Utc::now().to_rfc3339(), "control_id": request.control_id.clone(),
            "value": request.value.clone(), "expected_state_revision": request.expected_state_revision
        });
        if let Err(error) = append_audit(&request_record) {
            let mut result = ApiResult::failed(format!(
                "AUDIT_FAILURE: control write denied because request could not be recorded: {error}"
            ));
            result.status = "audit_failure".into();
            result.correlation_id = correlation_id;
            return Json(result);
        }
        let result = match self
            .radio
            .set_control(
                &request.control_id,
                request.value,
                request.expected_state_revision,
            )
            .await
        {
            Ok(state) => ApiResult::ok(state),
            Err(error) => {
                let mut result = ApiResult::failed(error.to_string());
                result.status = match error {
                    RadioError::PolicyDenied(_) => "denied",
                    RadioError::Unsupported(_) => "unsupported",
                    RadioError::Unavailable(_) => "unavailable",
                    RadioError::ReadbackMismatch(_) => "readback_mismatch",
                    RadioError::Protocol(_) => "failed",
                }
                .into();
                result
            }
        };
        let mut result = result;
        result.correlation_id = correlation_id.clone();
        if let Err(error) = append_audit(&serde_json::json!({
            "event": "radio_control_completed", "correlation_id": correlation_id.clone(),
            "observed_at_utc": chrono::Utc::now().to_rfc3339(), "control_id": request.control_id,
            "status": result.status.clone(), "errors": result.errors.clone()
        })) {
            result.status = "audit_failure".into();
            result.errors.push(format!(
                "action result could not be appended to audit log: {error}"
            ));
        }
        Json(result)
    }

    #[tool(
        annotations(
            title = "Request PTT state",
            read_only_hint = false,
            open_world_hint = true
        ),
        description = "Request PTT key-up or key-down. In the current implementation, key-up is denied because the session/permit arbiter is not implemented; key-down requests radio release. Use radio_get_ptt_state to inspect observed PTT. The watchdog is inactive, so this server is not qualified for transmitting."
    )]
    async fn radio_ptt_request(
        &self,
        Parameters(request): Parameters<PttRequest>,
    ) -> Json<ApiResult<crate::radio::PttObservation>> {
        let correlation_id = uuid::Uuid::new_v4().to_string();
        let requested_action = request.action;
        let record = serde_json::json!({
            "event": "ptt_request", "correlation_id": correlation_id.clone(),
            "observed_at_utc": chrono::Utc::now().to_rfc3339(), "request": request
        });
        // Release takes precedence over the audit file, including when the file stalls.
        let release = if requested_action == PttAction::KeyDown {
            Some(self.radio.request_ptt(request.clone()).await)
        } else {
            None
        };
        let audit_error = append_audit(&record).err();
        // Safety release must continue even when the audit volume is unavailable.
        // Transmit requests remain fail-closed because they can increase risk.
        if let Some(error) = audit_error
            .as_ref()
            .filter(|_| request.action == crate::radio::PttAction::KeyUp)
        {
            let mut result = ApiResult::failed(format!(
                "AUDIT_FAILURE: PTT request denied because it could not be recorded: {error}"
            ));
            result.status = "audit_failure".into();
            result.correlation_id = correlation_id;
            return Json(result);
        }
        let observation = match release {
            Some(observation) => observation,
            None => self.radio.request_ptt(request).await,
        };
        let mut result = match observation {
            Ok(value) if requested_action == PttAction::KeyDown && !value.release_confirmed => {
                let mut result = ApiResult::ok(value);
                result.status = "ptt_release_unconfirmed".into();
                result
                    .errors
                    .push("radio release could not be confirmed".into());
                result
            }
            Ok(value) => ApiResult::ok(value),
            Err(error) => {
                let mut result = ApiResult::failed(error.to_string());
                result.status = match error {
                    RadioError::PolicyDenied(_) => "denied",
                    RadioError::Unsupported(_) => "unsupported",
                    RadioError::Unavailable(_) => "unavailable",
                    RadioError::ReadbackMismatch(_) => "readback_mismatch",
                    RadioError::Protocol(_) => "failed",
                }
                .into();
                result
            }
        };
        result.correlation_id = correlation_id;
        if let Some(error) = audit_error {
            result.warnings.push(format!(
                "PTT release proceeded, but audit logging failed: {error}"
            ));
            if result.status == "ok" {
                result.status = "audit_failure".into();
            }
        }
        if let Err(error) = append_audit(&serde_json::json!({
            "event": "ptt_completed", "correlation_id": result.correlation_id.clone(),
            "observed_at_utc": chrono::Utc::now().to_rfc3339(),
            "requested_action": requested_action,
            "status": result.status.clone(), "errors": result.errors.clone(),
            "data": result.data
        })) {
            result.status = "audit_failure".into();
            result.errors.push(format!(
                "PTT result could not be appended to audit log: {error}"
            ));
        }
        Json(result)
    }

    #[tool(
        description = "Refresh the installed RMS inventory source. Returns source health, age, coverage, and separate gateway and channel counts. This operation leaves station control unchanged."
    )]
    async fn winlink_refresh_inventory(&self) -> Json<ApiResult<Value>> {
        station_result(async { self.station()?.refresh().await }.await)
    }

    #[tool(
        annotations(read_only_hint = true),
        description = "Read the last RMS inventory snapshot and eligibility counts. A listed gateway is directory evidence."
    )]
    async fn winlink_get_inventory_summary(&self) -> Json<ApiResult<Value>> {
        station_result(async { Ok(self.station()?.inventory().await) }.await)
    }

    #[tool(
        annotations(read_only_hint = true),
        description = "List the current authorized inventory channels for one RMS callsign. Dial and center frequencies retain their source semantics."
    )]
    async fn winlink_get_gateway_channels(
        &self,
        Parameters(request): Parameters<GatewayRequest>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                self.station()?
                    .gateway_channels(
                        &request.gateway,
                        request.offset.unwrap_or(0),
                        request.limit.unwrap_or(25),
                    )
                    .await
            }
            .await,
        )
    }

    #[tool(
        annotations(read_only_hint = true),
        description = "Read the station profile ID, shadow mode, inhibit state, and current preparation or attempt."
    )]
    async fn link_get_status(&self) -> Json<ApiResult<Value>> {
        station_result(async { Ok(self.station()?.status().await) }.await)
    }

    #[tool(
        description = "Rank channels using installed policy, inventory freshness, and attributable forecasts. Page an existing ranking by passing its ID."
    )]
    async fn link_rank_gateways(
        &self,
        Parameters(request): Parameters<RankingRequest>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                self.station()?
                    .rank(
                        request.ranking_id.as_deref().map(parse_id).transpose()?,
                        request.offset.unwrap_or(0),
                        request.limit.unwrap_or(25),
                    )
                    .await
            }
            .await,
        )
    }

    #[tool(
        description = "Acquire an expiring shadow-station lease, configure the simulated radio, and verify the selected candidate's readback."
    )]
    async fn link_prepare_candidate(
        &self,
        Parameters(request): Parameters<PrepareCandidate>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                self.station()?
                    .prepare(parse_id(&request.ranking_id)?, &request.channel_id)
                    .await
            }
            .await,
        )
    }

    #[tool(
        description = "Have installed policy revalidate a prepared handle and issue an expiring shadow-only authorization."
    )]
    async fn link_request_session_authorization(
        &self,
        Parameters(request): Parameters<PreparedHandle>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                self.station()?
                    .authorize(parse_id(&request.prepared_id)?)
                    .await
            }
            .await,
        )
    }

    #[tool(
        description = "Consume one prepared handle and authorization, and start an asynchronous radio-free session using the installed shadow backend."
    )]
    async fn link_start_session(
        &self,
        Parameters(request): Parameters<StartLink>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                self.station()?
                    .start(
                        parse_id(&request.prepared_id)?,
                        parse_id(&request.grant_id)?,
                    )
                    .await
            }
            .await,
        )
    }

    #[tool(
        annotations(read_only_hint = true),
        description = "Read a shadow session's lifecycle and available outcome stages."
    )]
    async fn link_get_session(
        &self,
        Parameters(request): Parameters<AttemptHandle>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                self.station()?
                    .attempt(parse_id(&request.attempt_id)?)
                    .await
            }
            .await,
        )
    }

    #[tool(
        description = "Cancel an active shadow session and wait for local cleanup before returning. Repeated calls return the terminal outcome."
    )]
    async fn link_abort_session(
        &self,
        Parameters(request): Parameters<AttemptHandle>,
    ) -> Json<ApiResult<Value>> {
        station_result(async { self.station()?.abort(parse_id(&request.attempt_id)?).await }.await)
    }

    #[tool(
        annotations(read_only_hint = true),
        description = "List bounded payload-free session history for the chosen lookback interval."
    )]
    async fn link_get_history(
        &self,
        Parameters(request): Parameters<HistoryRequest>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                Ok(self
                    .station()?
                    .history(
                        history_start(request.window_seconds),
                        request.limit.unwrap_or(25),
                    )
                    .await)
            }
            .await,
        )
    }

    #[tool(
        annotations(read_only_hint = true),
        description = "Report attempt, connection and client-transfer counts and denominators for the chosen lookback interval."
    )]
    async fn link_get_metrics(
        &self,
        Parameters(request): Parameters<HistoryRequest>,
    ) -> Json<ApiResult<Value>> {
        station_result(
            async {
                Ok(self
                    .station()?
                    .metrics(history_start(request.window_seconds))
                    .await)
            }
            .await,
        )
    }
}

pub async fn serve(
    radio: Arc<dyn RadioControlPort>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    serve_station(radio, None).await
}

pub async fn serve_station(
    radio: Arc<dyn RadioControlPort>,
    station: Option<Arc<Olm>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let refresh = station.as_ref().map(Olm::start_refresh);
    let result: Result<(), Box<dyn std::error::Error + Send + Sync>> = async {
        let service = RadioMcp {
            radio,
            station: station.clone(),
        }
        .serve(stdio())
        .await?;
        service.waiting().await?;
        Ok(())
    }
    .await;
    if let Some(refresh) = refresh {
        refresh.abort();
        let _ = refresh.await;
    }
    if let Some(station) = station {
        station.shutdown().await?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_value_json_uses_documented_tagged_shapes() {
        let examples = [
            (
                ControlValue::FrequencyHz(7_100_000),
                serde_json::json!({"type": "frequency_hz", "value": 7100000}),
            ),
            (
                ControlValue::Mode("USB".into()),
                serde_json::json!({"type": "mode", "value": "USB"}),
            ),
            (
                ControlValue::SplitVfo {
                    enabled: true,
                    tx_vfo: "VFOB".into(),
                },
                serde_json::json!({"type": "split_vfo", "value": {"enabled": true, "tx_vfo": "VFOB"}}),
            ),
            (
                ControlValue::SignedHertz(-250),
                serde_json::json!({"type": "signed_hertz", "value": -250}),
            ),
        ];

        for (value, expected) in examples {
            let encoded = serde_json::to_value(&value).expect("serialize ControlValue");
            assert_eq!(encoded, expected);
            let decoded: ControlValue =
                serde_json::from_value(encoded).expect("deserialize ControlValue");
            assert_eq!(decoded, value);
        }
    }
}
