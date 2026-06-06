use crate::state::ApiState;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use mac_address::MacAddress;
use nm_core::device::{DeviceKind, OsHint};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::str::FromStr;
use std::time::Duration;
use tokio_stream::{wrappers::BroadcastStream, StreamExt};
use tower_http::trace::TraceLayer;
use uuid::Uuid;

pub fn build_router(state: ApiState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/agent", get(get_agent).put(update_agent))
        .route("/devices", get(list_devices))
        .route("/devices/{mac}", get(get_device).put(update_device))
        .route("/identities", get(list_identities).post(create_identity))
        .route("/events", get(list_events))
        .route("/events/stream", get(event_stream))
        .route("/speedtests", get(list_speedtests))
        .route("/speedtests/summary", get(speedtest_summary))
        .route("/speedtests/run", post(run_speedtest))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

#[derive(Serialize)]
struct HealthResponse {
    status: String,
    agent_name: String,
    network_name: Option<String>,
    link_state: String,
    gateway: Option<String>,
}

async fn health(State(state): State<ApiState>) -> Json<HealthResponse> {
    let link = state.link_monitor.status();
    Json(HealthResponse {
        status: "ok".into(),
        agent_name: state.agent_name.read().clone(),
        network_name: state.network_context.network_name(),
        link_state: format!("{:?}", link.state).to_lowercase(),
        gateway: link.gateway.map(|g| g.to_string()),
    })
}

#[derive(Serialize)]
struct AgentResponse {
    name: String,
}

async fn get_agent(State(state): State<ApiState>) -> Json<AgentResponse> {
    Json(AgentResponse {
        name: state.agent_name.read().clone(),
    })
}

#[derive(Deserialize)]
struct UpdateAgentRequest {
    name: String,
}

async fn update_agent(
    State(state): State<ApiState>,
    Json(req): Json<UpdateAgentRequest>,
) -> Result<Json<AgentResponse>, StatusCode> {
    if req.name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    *state.agent_name.write() = req.name.clone();
    state
        .store
        .set_agent_name(&req.name)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(AgentResponse { name: req.name }))
}

async fn list_devices(State(state): State<ApiState>) -> Result<Json<Vec<nm_core::Device>>, StatusCode> {
    let devices = state.store.list_devices().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(devices))
}

async fn get_device(
    State(state): State<ApiState>,
    Path(mac): Path<String>,
) -> Result<Json<nm_core::Device>, StatusCode> {
    let mac = MacAddress::from_str(&mac).map_err(|_| StatusCode::BAD_REQUEST)?;
    let device = state
        .store
        .get_device(&mac)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok(Json(device))
}

#[derive(Deserialize)]
struct UpdateDeviceRequest {
    user_label: Option<String>,
    identity_id: Option<Uuid>,
    kind: Option<String>,
    os_hint: Option<String>,
    do_not_scan: Option<bool>,
}

async fn update_device(
    State(state): State<ApiState>,
    Path(mac): Path<String>,
    Json(req): Json<UpdateDeviceRequest>,
) -> Result<Json<nm_core::Device>, StatusCode> {
    let mac = MacAddress::from_str(&mac).map_err(|_| StatusCode::BAD_REQUEST)?;
    let kind = req.kind.as_deref().map(parse_kind).transpose()?;
    let os_hint = req.os_hint.as_deref().map(parse_os).transpose()?;
    let device = state
        .store
        .update_device_label(&mac, req.user_label, req.identity_id, kind, os_hint, req.do_not_scan)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(device))
}

#[derive(Deserialize)]
struct CreateIdentityRequest {
    display_name: String,
    notes: Option<String>,
}

async fn list_identities(
    State(state): State<ApiState>,
) -> Result<Json<Vec<nm_core::Identity>>, StatusCode> {
    let identities = state
        .store
        .list_identities()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(identities))
}

async fn create_identity(
    State(state): State<ApiState>,
    Json(req): Json<CreateIdentityRequest>,
) -> Result<Json<nm_core::Identity>, StatusCode> {
    if req.display_name.trim().is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let identity = state
        .store
        .create_identity(&req.display_name, req.notes)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(identity))
}

#[derive(Deserialize)]
struct EventsQuery {
    limit: Option<usize>,
    kind: Option<String>,
}

async fn list_events(
    State(state): State<ApiState>,
    Query(q): Query<EventsQuery>,
) -> Result<Json<Vec<nm_core::EventRecord>>, StatusCode> {
    let limit = q.limit.unwrap_or(100).min(1000);
    let kind = q
        .kind
        .as_deref()
        .map(parse_event_kind)
        .transpose()?;
    let events = state
        .store
        .list_events(limit, kind)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(events))
}

async fn event_stream(
    State(state): State<ApiState>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(data) => Some(Ok(Event::default().data(data))),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Deserialize)]
struct SpeedTestsQuery {
    from: Option<String>,
    to: Option<String>,
    limit: Option<usize>,
}

async fn list_speedtests(
    State(state): State<ApiState>,
    Query(q): Query<SpeedTestsQuery>,
) -> Result<Json<Vec<nm_core::SpeedTestResult>>, StatusCode> {
    let from = q.from.as_deref().map(parse_time).transpose()?;
    let to = q.to.as_deref().map(parse_time).transpose()?;
    let limit = q.limit.unwrap_or(500).min(5000);
    let results = state
        .store
        .list_speed_tests(from, to, limit)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(results))
}

#[derive(Deserialize)]
struct SummaryQuery {
    from: Option<String>,
    to: Option<String>,
}

async fn speedtest_summary(
    State(state): State<ApiState>,
    Query(q): Query<SummaryQuery>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let from = q.from.as_deref().map(parse_time).transpose()?;
    let to = q.to.as_deref().map(parse_time).transpose()?;
    let summary = state
        .store
        .speed_test_summary(from, to)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(summary))
}

async fn run_speedtest(State(state): State<ApiState>) -> Result<Json<nm_core::SpeedTestResult>, StatusCode> {
    let scheduler = state
        .speedtest
        .as_ref()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let result = scheduler
        .run_manual()
        .await
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    state
        .store
        .insert_speed_test(&result)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(result))
}

fn parse_time(s: &str) -> Result<DateTime<Utc>, StatusCode> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

fn parse_kind(s: &str) -> Result<DeviceKind, StatusCode> {
    Ok(match s {
        "router" => DeviceKind::Router,
        "mobile" => DeviceKind::Mobile,
        "desktop" => DeviceKind::Desktop,
        "iot" => DeviceKind::IoT,
        "unknown" => DeviceKind::Unknown,
        _ => return Err(StatusCode::BAD_REQUEST),
    })
}

fn parse_os(s: &str) -> Result<OsHint, StatusCode> {
    Ok(match s {
        "android" => OsHint::Android,
        "ios" => OsHint::Ios,
        "linux" => OsHint::Linux,
        "macos" => OsHint::MacOS,
        "windows" => OsHint::Windows,
        "unknown" => OsHint::Unknown,
        _ => return Err(StatusCode::BAD_REQUEST),
    })
}

fn parse_event_kind(s: &str) -> Result<nm_core::EventKind, StatusCode> {
    use nm_core::EventKind;
    Ok(match s {
        "network_down" => EventKind::NetworkDown,
        "network_restored" => EventKind::NetworkRestored,
        "device_joined" => EventKind::DeviceJoined,
        "device_left" => EventKind::DeviceLeft,
        "device_returned" => EventKind::DeviceReturned,
        "ip_changed" => EventKind::IpChanged,
        "kind_refined" => EventKind::KindRefined,
        "speed_test_completed" => EventKind::SpeedTestCompleted,
        _ => return Err(StatusCode::BAD_REQUEST),
    })
}
