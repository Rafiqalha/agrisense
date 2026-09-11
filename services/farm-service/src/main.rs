use anyhow::{Context, Result};
use async_nats::jetstream;
use axum::{
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    routing::post,
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::{sync::Arc, time::Duration};
use tracing::{error, info, warn};
use uuid::Uuid;

const MAX_DESCRIPTION_CHARS: usize = 500;
const OUTBOX_BATCH_SIZE: i32 = 50;

#[derive(Clone)]
struct AppState {
    db: shared_db::DbPool,
    brain_token: shared_auth::InternalToken,
    nats: async_nats::Client,
    jetstream: jetstream::Context,
}

#[derive(Debug, Deserialize)]
struct RecordActivityRequest {
    /// Authenticated WhatsApp identity forwarded by Brain. Farm/crop IDs are
    /// deliberately absent and are resolved inside the database boundary.
    owner_phone: String,
    request_id: Uuid,
    activity_type: String,
    description: String,
    quantity: Option<String>,
    unit: Option<String>,
    performed_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct RecentActivitiesRequest {
    owner_phone: String,
    activity_type: Option<String>,
    #[serde(default = "default_recent_limit")]
    limit: i32,
}

fn default_recent_limit() -> i32 {
    5
}

#[derive(Debug, Clone, Serialize)]
struct ActivityResponse {
    activity_id: Uuid,
    farm_id: Uuid,
    crop_id: Uuid,
    activity_type: String,
    description: String,
    quantity: Option<String>,
    unit: Option<String>,
    performed_at: DateTime<Utc>,
    already_existed: bool,
}

#[derive(Debug, Clone, Serialize)]
struct RecentActivityResponse {
    activity_id: Uuid,
    activity_type: String,
    description: String,
    quantity: Option<String>,
    unit: Option<String>,
    performed_at: DateTime<Utc>,
}

#[derive(Debug)]
struct ClaimedEvent {
    id: Uuid,
    subject: String,
    payload: serde_json::Value,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();

    shared_observability::init(shared_observability::ObservabilityConfig {
        service_name: "farm-service".into(),
        log_level: std::env::var("LOG_LEVEL").unwrap_or_else(|_| "info".into()),
        log_format: if std::env::var("LOG_FORMAT").unwrap_or_default() == "json" {
            shared_observability::LogFormat::Json
        } else {
            shared_observability::LogFormat::Pretty
        },
        otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
    });

    info!(version = env!("CARGO_PKG_VERSION"), "Starting farm-service");

    let database_url = required_env("DATABASE_URL")?;
    let brain_farm_token = required_env("BRAIN_FARM_TOKEN")?;
    let nats_url = required_env("NATS_URL")?;
    if std::env::var("APP_ENV").as_deref() == Ok("production") {
        anyhow::ensure!(
            database_url.starts_with("postgres://agrisense_farm:"),
            "farm-service must use the dedicated agrisense_farm database role in production"
        );
        anyhow::ensure!(
            !database_url.contains("farm-local-only"),
            "FARM_DB_PASSWORD must be replaced before production"
        );
        anyhow::ensure!(
            brain_farm_token != "brain-farm-local-only-change-me-32bytes",
            "BRAIN_FARM_TOKEN must be replaced before production"
        );
    }

    let db = shared_db::create_pool(&database_url, 10).await?;
    let nats = async_nats::connect(&nats_url).await?;
    let jetstream = jetstream::new(nats.clone());
    jetstream
        .get_or_create_stream(jetstream::stream::Config {
            name: "AGRISENSE".into(),
            subjects: vec!["agrisense.>".into()],
            storage: jetstream::stream::StorageType::File,
            max_age: Duration::from_secs(7 * 24 * 60 * 60),
            max_bytes: 5 * 1024 * 1024 * 1024,
            ..Default::default()
        })
        .await
        .context("failed to initialize AGRISENSE JetStream")?;

    let state = Arc::new(AppState {
        db,
        brain_token: shared_auth::InternalToken::new(brain_farm_token)?,
        nats,
        jetstream,
    });
    tokio::spawn(run_outbox_publisher(state.clone()));
    info!("Connected to PostgreSQL and NATS JetStream");

    let app = Router::new()
        .route("/health", axum::routing::get(|| async { "ok" }))
        .route("/live", axum::routing::get(|| async { "ok" }))
        .route("/ready", axum::routing::get(readiness))
        .route("/internal/activities", post(record_activity))
        .route("/internal/activities/recent", post(recent_activities))
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(state);

    let port = std::env::var("FARM_SERVICE_PORT").unwrap_or_else(|_| "3003".into());
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!(addr = %addr, "farm-service listening");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn readiness(State(state): State<Arc<AppState>>) -> StatusCode {
    if shared_db::is_ready(&state.db).await
        && state.nats.connection_state() == async_nats::connection::State::Connected
    {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

async fn record_activity(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<RecordActivityRequest>,
) -> Result<Json<ActivityResponse>, (StatusCode, Json<serde_json::Value>)> {
    authorize(&headers, &state.brain_token)?;
    validate_record_request(&request).map_err(bad_request)?;

    match record_activity_in_db(&state.db, &request).await {
        Ok(activity) => Ok(Json(activity)),
        Err(error) => {
            error!(error = %error, request_id = %request.request_id, "Farm activity write failed");
            Err(internal_error("activity_recording_failed"))
        }
    }
}

async fn recent_activities(
    headers: HeaderMap,
    State(state): State<Arc<AppState>>,
    Json(request): Json<RecentActivitiesRequest>,
) -> Result<Json<Vec<RecentActivityResponse>>, (StatusCode, Json<serde_json::Value>)> {
    authorize(&headers, &state.brain_token)?;
    validate_phone(&request.owner_phone).map_err(bad_request)?;
    if !(1..=20).contains(&request.limit) {
        return Err(bad_request(anyhow::anyhow!(
            "limit must be between 1 and 20"
        )));
    }
    if let Some(activity_type) = request.activity_type.as_deref() {
        validate_activity_type(activity_type).map_err(bad_request)?;
    }

    match list_recent_in_db(&state.db, &request).await {
        Ok(activities) => Ok(Json(activities)),
        Err(error) => {
            error!(error = %error, "Recent farm activity query failed");
            Err(internal_error("activity_query_failed"))
        }
    }
}

fn authorize(
    headers: &HeaderMap,
    token: &shared_auth::InternalToken,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if token.authorize_header(authorization) {
        Ok(())
    } else {
        Err((
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        ))
    }
}

fn bad_request(error: anyhow::Error) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({
            "error": "invalid_request",
            "message": error.to_string(),
        })),
    )
}

fn internal_error(code: &str) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({
            "error": code,
            "message": "Farm operation could not be completed",
        })),
    )
}

fn validate_record_request(request: &RecordActivityRequest) -> Result<()> {
    validate_phone(&request.owner_phone)?;
    validate_activity_type(&request.activity_type)?;
    let description = request.description.trim();
    anyhow::ensure!(
        (2..=MAX_DESCRIPTION_CHARS).contains(&description.chars().count()),
        "description must contain 2 to {MAX_DESCRIPTION_CHARS} characters"
    );
    anyhow::ensure!(
        !description.chars().any(char::is_control),
        "description contains control characters"
    );
    anyhow::ensure!(
        request.quantity.is_some() == request.unit.is_some(),
        "quantity and unit must be supplied together"
    );
    if let Some(quantity) = request.quantity.as_deref() {
        validate_decimal(quantity)?;
    }
    if let Some(unit) = request.unit.as_deref() {
        anyhow::ensure!(
            (1..=30).contains(&unit.len())
                && unit.chars().all(|character| {
                    character.is_ascii_alphanumeric() || " ./%-".contains(character)
                }),
            "unit is invalid"
        );
    }
    let now = Utc::now();
    anyhow::ensure!(
        request.performed_at <= now + chrono::Duration::minutes(5)
            && request.performed_at >= now - chrono::Duration::days(3_653),
        "performed_at is outside the accepted range"
    );
    Ok(())
}

fn validate_phone(phone: &str) -> Result<()> {
    anyhow::ensure!(
        phone.starts_with('+')
            && (9..=16).contains(&phone.len())
            && phone[1..].bytes().all(|byte| byte.is_ascii_digit()),
        "owner_phone must use canonical E.164 format"
    );
    Ok(())
}

fn validate_activity_type(activity_type: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(
            activity_type,
            "watering" | "fertilizing" | "spraying" | "pruning" | "inspection"
        ),
        "unsupported activity type"
    );
    Ok(())
}

fn validate_decimal(value: &str) -> Result<()> {
    let mut decimal_points = 0;
    anyhow::ensure!(
        !value.is_empty()
            && value.len() <= 14
            && value.bytes().all(|byte| {
                if byte == b'.' {
                    decimal_points += 1;
                    decimal_points == 1
                } else {
                    byte.is_ascii_digit()
                }
            })
            && value
                .split_once('.')
                .is_none_or(|(_, fraction)| (1..=3).contains(&fraction.len()))
            && value.parse::<f64>().is_ok_and(|number| number > 0.0),
        "quantity must be a positive decimal"
    );
    Ok(())
}

async fn set_phone_scope(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    phone: &str,
) -> Result<()> {
    sqlx::query("SELECT set_config('app.current_phone', $1, TRUE)")
        .bind(phone)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

async fn record_activity_in_db(
    pool: &shared_db::DbPool,
    request: &RecordActivityRequest,
) -> Result<ActivityResponse> {
    let mut tx = pool.begin().await?;
    set_phone_scope(&mut tx, &request.owner_phone).await?;
    let row =
        sqlx::query("SELECT * FROM farm.record_activity_for_current_phone($1,$2,$3,$4,$5,$6)")
            .bind(request.request_id)
            .bind(&request.activity_type)
            .bind(request.description.trim())
            .bind(&request.quantity)
            .bind(&request.unit)
            .bind(request.performed_at)
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;

    Ok(ActivityResponse {
        activity_id: row.try_get("activity_id")?,
        farm_id: row.try_get("farm_id")?,
        crop_id: row.try_get("crop_id")?,
        activity_type: row.try_get("activity_type")?,
        description: row.try_get("description")?,
        quantity: row.try_get("quantity")?,
        unit: row.try_get("unit")?,
        performed_at: row.try_get("performed_at")?,
        already_existed: row.try_get("already_existed")?,
    })
}

async fn list_recent_in_db(
    pool: &shared_db::DbPool,
    request: &RecentActivitiesRequest,
) -> Result<Vec<RecentActivityResponse>> {
    let mut tx = pool.begin().await?;
    set_phone_scope(&mut tx, &request.owner_phone).await?;
    let rows = sqlx::query("SELECT * FROM farm.list_recent_activities_for_current_phone($1,$2)")
        .bind(&request.activity_type)
        .bind(request.limit)
        .fetch_all(&mut *tx)
        .await?;
    tx.commit().await?;

    rows.into_iter()
        .map(|row| {
            Ok(RecentActivityResponse {
                activity_id: row.try_get("activity_id")?,
                activity_type: row.try_get("activity_type")?,
                description: row.try_get("description")?,
                quantity: row.try_get("quantity")?,
                unit: row.try_get("unit")?,
                performed_at: row.try_get("performed_at")?,
            })
        })
        .collect()
}

async fn run_outbox_publisher(state: Arc<AppState>) {
    loop {
        if let Err(error) = publish_outbox_batch(&state).await {
            warn!(error = %error, "Farm activity outbox polling failed");
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

async fn publish_outbox_batch(state: &AppState) -> Result<()> {
    let events = claim_outbox(&state.db).await?;
    for event in events {
        let result = async {
            let payload = serde_json::to_vec(&event.payload)?;
            state
                .jetstream
                .publish(event.subject.clone(), payload.into())
                .await?
                .await?;
            Result::<()>::Ok(())
        }
        .await;

        match result {
            Ok(()) => mark_outbox_published(&state.db, event.id).await?,
            Err(error) => {
                warn!(event_id = %event.id, error = %error, "Farm activity event publish failed");
                mark_outbox_failed(&state.db, event.id, &error.to_string()).await?;
            }
        }
    }
    Ok(())
}

async fn claim_outbox(pool: &shared_db::DbPool) -> Result<Vec<ClaimedEvent>> {
    let rows = sqlx::query("SELECT * FROM farm.claim_activity_outbox($1)")
        .bind(OUTBOX_BATCH_SIZE)
        .fetch_all(pool)
        .await?;
    rows.into_iter()
        .map(|row| {
            Ok(ClaimedEvent {
                id: row.try_get("event_id")?,
                subject: row.try_get("nats_subject")?,
                payload: row.try_get("payload")?,
            })
        })
        .collect()
}

async fn mark_outbox_published(pool: &shared_db::DbPool, event_id: Uuid) -> Result<()> {
    sqlx::query("SELECT farm.mark_activity_outbox_published($1)")
        .bind(event_id)
        .execute(pool)
        .await?;
    Ok(())
}

async fn mark_outbox_failed(pool: &shared_db::DbPool, event_id: Uuid, error: &str) -> Result<()> {
    sqlx::query("SELECT farm.mark_activity_outbox_failed($1,$2)")
        .bind(event_id)
        .bind(error)
        .execute(pool)
        .await?;
    Ok(())
}

fn required_env(name: &str) -> Result<String> {
    let value = std::env::var(name).with_context(|| format!("{name} must be configured"))?;
    anyhow::ensure!(!value.trim().is_empty(), "{name} must not be blank");
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_request() -> RecordActivityRequest {
        RecordActivityRequest {
            owner_phone: "+628123456789".into(),
            request_id: Uuid::new_v4(),
            activity_type: "fertilizing".into(),
            description: "Pemupukan NPK pada seluruh greenhouse".into(),
            quantity: Some("25".into()),
            unit: Some("kg".into()),
            performed_at: Utc::now(),
        }
    }

    #[test]
    fn valid_activity_command_is_accepted() {
        assert!(validate_record_request(&valid_request()).is_ok());
    }

    #[test]
    fn owner_and_activity_type_are_fail_closed() {
        let mut request = valid_request();
        request.owner_phone = "628123456789".into();
        assert!(validate_record_request(&request).is_err());

        request.owner_phone = "+628123456789".into();
        request.activity_type = "delete_everything".into();
        assert!(validate_record_request(&request).is_err());
    }

    #[test]
    fn quantity_and_unit_must_be_consistent() {
        let mut request = valid_request();
        request.unit = None;
        assert!(validate_record_request(&request).is_err());
        request.unit = Some("kg".into());
        request.quantity = Some("-25".into());
        assert!(validate_record_request(&request).is_err());
    }

    #[test]
    fn descriptions_are_bounded_and_control_free() {
        let mut request = valid_request();
        request.description = "x".repeat(MAX_DESCRIPTION_CHARS + 1);
        assert!(validate_record_request(&request).is_err());
        request.description = "siram\nsemua".into();
        assert!(validate_record_request(&request).is_err());
    }
}
