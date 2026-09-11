//! Deterministic farm-activity confirmation workflow.
//!
//! The language model never writes farm data. Brain validates and confirms a
//! bounded draft; farm-service resolves the authenticated owner's active crop
//! and performs the idempotent domain write.

use anyhow::{Context, Result};
use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use uuid::Uuid;

use crate::context::FarmerContext;

const MAX_DESCRIPTION_CHARS: usize = 500;

#[derive(Debug)]
pub struct ActivityOutcome {
    pub response: String,
    pub tools_used: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Draft {
    activity_type: Option<String>,
    description: Option<String>,
    quantity: Option<String>,
    unit: Option<String>,
    performed_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
struct Session {
    step: Step,
    draft: Draft,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    Details,
    Confirm,
}

impl Step {
    fn as_str(self) -> &'static str {
        match self {
            Self::Details => "details",
            Self::Confirm => "confirm",
        }
    }

    fn parse(value: &str) -> Result<Self> {
        match value {
            "details" => Ok(Self::Details),
            "confirm" => Ok(Self::Confirm),
            _ => anyhow::bail!("database returned an invalid activity command step"),
        }
    }
}

#[derive(Debug, Deserialize)]
struct FarmActivity {
    #[allow(dead_code)] // Kept for provenance/debug responses from farm-service.
    activity_id: Uuid,
    activity_type: String,
    description: String,
    quantity: Option<String>,
    unit: Option<String>,
    performed_at: DateTime<Utc>,
    #[allow(dead_code)]
    #[serde(default)]
    already_existed: bool,
}

/// Handles activity commands and deterministic history questions. `None`
/// means the normal agronomist loop should process the message.
pub async fn handle(
    pool: &shared_db::DbPool,
    context: &FarmerContext,
    farm_service_url: &str,
    farm_token: &shared_auth::InternalToken,
    phone: &str,
    message: &str,
    request_id: Uuid,
) -> Result<Option<ActivityOutcome>> {
    if let Some(response) = cached_response(pool, phone, request_id).await? {
        return Ok(Some(ActivityOutcome {
            response,
            tools_used: Vec::new(),
        }));
    }

    let mut session = load_session(pool, phone).await?;
    let normalized = normalize(message);

    if session.is_some() && is_cancel(&normalized) {
        let response =
            "Pencatatan aktivitas dibatalkan. Tidak ada data aktivitas yang disimpan.".to_owned();
        cancel_session(pool, phone, request_id, &response).await?;
        return Ok(Some(ActivityOutcome {
            response,
            tools_used: Vec::new(),
        }));
    }

    if let Some(ref mut active_session) = session {
        let outcome = match active_session.step {
            Step::Details => match parse_draft(message, Utc::now()) {
                Ok(draft) => {
                    active_session.draft = draft;
                    let response = confirmation(&active_session.draft)?;
                    save_session(
                        pool,
                        phone,
                        Step::Confirm,
                        &active_session.draft,
                        request_id,
                        &response,
                    )
                    .await?;
                    ActivityOutcome {
                        response,
                        tools_used: Vec::new(),
                    }
                }
                Err(_) => {
                    let response = activity_format_help();
                    save_session(
                        pool,
                        phone,
                        Step::Details,
                        &active_session.draft,
                        request_id,
                        &response,
                    )
                    .await?;
                    ActivityOutcome {
                        response,
                        tools_used: Vec::new(),
                    }
                }
            },
            Step::Confirm => {
                if is_affirmative(&normalized) {
                    let activity = record_via_farm_service(
                        farm_service_url,
                        farm_token,
                        phone,
                        request_id,
                        &active_session.draft,
                    )
                    .await?;
                    let response = completion_reply(&activity);
                    complete_session(pool, phone, request_id, &response).await?;
                    ActivityOutcome {
                        response,
                        tools_used: vec!["farm.record_activity".into()],
                    }
                } else if normalized == "ubah" {
                    let response = format!(
                        "Silakan kirim ulang detail aktivitasnya. {}",
                        activity_format_help()
                    );
                    save_session(
                        pool,
                        phone,
                        Step::Details,
                        &active_session.draft,
                        request_id,
                        &response,
                    )
                    .await?;
                    ActivityOutcome {
                        response,
                        tools_used: Vec::new(),
                    }
                } else {
                    let response =
                        "Balas *YA* untuk menyimpan, *UBAH* untuk memperbaiki, atau *BATAL*."
                            .to_owned();
                    save_session(
                        pool,
                        phone,
                        Step::Confirm,
                        &active_session.draft,
                        request_id,
                        &response,
                    )
                    .await?;
                    ActivityOutcome {
                        response,
                        tools_used: Vec::new(),
                    }
                }
            }
        };
        return Ok(Some(outcome));
    }

    if is_history_query(&normalized) {
        if context.crop.is_none() {
            return Ok(Some(ActivityOutcome {
                response: "Belum ada tanaman aktif. Ketik *DAFTAR TANAMAN* terlebih dahulu.".into(),
                tools_used: Vec::new(),
            }));
        }
        let activity_type = detect_activity_type(&normalized);
        let limit = if normalized.contains("riwayat") { 5 } else { 1 };
        let activities =
            list_recent_via_farm_service(farm_service_url, farm_token, phone, activity_type, limit)
                .await?;
        return Ok(Some(ActivityOutcome {
            response: history_reply(&activities, activity_type),
            tools_used: vec!["farm.list_recent_activities".into()],
        }));
    }

    if !is_record_command(&normalized) {
        return Ok(None);
    }
    if context.crop.is_none() {
        return Ok(Some(ActivityOutcome {
            response: "Belum ada tanaman aktif untuk menerima catatan. Ketik *DAFTAR TANAMAN* terlebih dahulu."
                .into(),
            tools_used: Vec::new(),
        }));
    }

    match parse_draft(message, Utc::now()) {
        Ok(draft) => {
            let response = confirmation(&draft)?;
            save_session(pool, phone, Step::Confirm, &draft, request_id, &response).await?;
            Ok(Some(ActivityOutcome {
                response,
                tools_used: Vec::new(),
            }))
        }
        Err(_) => {
            let response = activity_format_help();
            save_session(
                pool,
                phone,
                Step::Details,
                &Draft::default(),
                request_id,
                &response,
            )
            .await?;
            Ok(Some(ActivityOutcome {
                response,
                tools_used: Vec::new(),
            }))
        }
    }
}

fn parse_draft(message: &str, now: DateTime<Utc>) -> Result<Draft> {
    let description = strip_record_prefix(message).trim();
    anyhow::ensure!(
        (2..=MAX_DESCRIPTION_CHARS).contains(&description.chars().count()),
        "invalid activity description length"
    );
    anyhow::ensure!(
        !description.chars().any(char::is_control),
        "activity description contains control characters"
    );
    let lower = description.to_lowercase();
    anyhow::ensure!(
        !lower.contains("http://") && !lower.contains("https://"),
        "links are not accepted"
    );
    let activity_type = detect_activity_type(&lower).context("activity type not recognized")?;
    let (quantity, unit) = parse_quantity(description);
    let performed_at = if lower.contains("kemarin") {
        now - chrono::Duration::days(1)
    } else {
        now
    };

    Ok(Draft {
        activity_type: Some(activity_type.into()),
        description: Some(description.to_owned()),
        quantity,
        unit,
        performed_at: Some(performed_at),
    })
}

fn strip_record_prefix(message: &str) -> &str {
    let trimmed = message.trim();
    let first_word_end = trimmed.find(char::is_whitespace).unwrap_or(trimmed.len());
    if trimmed[..first_word_end].eq_ignore_ascii_case("catat") {
        trimmed[first_word_end..].trim_start()
    } else {
        trimmed
    }
}

fn detect_activity_type(message: &str) -> Option<&'static str> {
    if ["menyiram", "penyiraman", "siram"]
        .iter()
        .any(|term| message.contains(term))
    {
        Some("watering")
    } else if ["memupuk", "pemupukan", "pupuk"]
        .iter()
        .any(|term| message.contains(term))
    {
        Some("fertilizing")
    } else if ["menyemprot", "penyemprotan", "semprot"]
        .iter()
        .any(|term| message.contains(term))
    {
        Some("spraying")
    } else if ["memangkas", "pemangkasan", "pangkas"]
        .iter()
        .any(|term| message.contains(term))
    {
        Some("pruning")
    } else if ["inspeksi", "memeriksa", "periksa tanaman", "cek tanaman"]
        .iter()
        .any(|term| message.contains(term))
    {
        Some("inspection")
    } else {
        None
    }
}

fn is_record_command(message: &str) -> bool {
    if message.contains("pengeluaran")
        || message.contains("pendapatan")
        || message.starts_with("catat beli")
        || message.starts_with("catat jual")
    {
        return false;
    }
    message
        .split_whitespace()
        .next()
        .is_some_and(|word| word == "catat")
        && (detect_activity_type(message).is_some()
            || matches!(message, "catat" | "catat aktivitas"))
}

fn is_history_query(message: &str) -> bool {
    message.contains("riwayat aktivitas")
        || message.contains("aktivitas terakhir")
        || (message.contains("terakhir")
            && message.contains("kapan")
            && detect_activity_type(message).is_some())
}

fn parse_quantity(description: &str) -> (Option<String>, Option<String>) {
    let tokens = description
        .split_whitespace()
        .map(|token| {
            token.trim_matches(|character: char| {
                !character.is_ascii_alphanumeric() && character != ',' && character != '.'
            })
        })
        .collect::<Vec<_>>();

    for pair in tokens.windows(2) {
        let Some(number) = normalize_decimal(pair[0]) else {
            continue;
        };
        let Some(unit) = normalize_unit(pair[1]) else {
            continue;
        };
        return (Some(number), Some(unit.into()));
    }
    (None, None)
}

fn normalize_decimal(value: &str) -> Option<String> {
    let normalized = if value.contains(',') {
        value.replace('.', "").replace(',', ".")
    } else {
        value.to_owned()
    };
    if normalized.len() > 14
        || normalized.matches('.').count() > 1
        || normalized
            .split_once('.')
            .is_some_and(|(_, fraction)| !(1..=3).contains(&fraction.len()))
        || !normalized
            .bytes()
            .all(|byte| byte.is_ascii_digit() || byte == b'.')
    {
        return None;
    }
    normalized
        .parse::<f64>()
        .ok()
        .filter(|number| number.is_finite() && *number > 0.0)
        .map(|_| normalized)
}

fn normalize_unit(value: &str) -> Option<&'static str> {
    match value.to_lowercase().as_str() {
        "kg" | "kilogram" => Some("kg"),
        "g" | "gram" => Some("g"),
        "l" | "liter" | "litre" => Some("liter"),
        "ml" | "mililiter" => Some("ml"),
        "sak" => Some("sak"),
        "botol" => Some("botol"),
        "tangki" => Some("tangki"),
        _ => None,
    }
}

fn confirmation(draft: &Draft) -> Result<String> {
    let activity_type = required(&draft.activity_type, "activity_type")?;
    let description = required(&draft.description, "description")?;
    let performed_at = draft.performed_at.context("missing performed_at")?;
    let quantity = match (draft.quantity.as_deref(), draft.unit.as_deref()) {
        (Some(quantity), Some(unit)) => format!("\n• Jumlah: {quantity} {unit}"),
        _ => String::new(),
    };
    Ok(format!(
        "Mohon konfirmasi aktivitas berikut:\n\n• Aktivitas: {}\n• Tanggal: {}\n• Rincian: {}{}\n\nBalas *YA* untuk menyimpan, *UBAH* untuk memperbaiki, atau *BATAL*.",
        activity_label(activity_type),
        jakarta_date(performed_at),
        description,
        quantity,
    ))
}

fn completion_reply(activity: &FarmActivity) -> String {
    let quantity = match (activity.quantity.as_deref(), activity.unit.as_deref()) {
        (Some(quantity), Some(unit)) => {
            format!("\nJumlah: {} {unit}", display_quantity(quantity))
        }
        _ => String::new(),
    };
    format!(
        "Aktivitas berhasil disimpan. ✅\n\nAktivitas: {}\nTanggal: {}\nRincian: {}{}",
        activity_label(&activity.activity_type),
        jakarta_date(activity.performed_at),
        activity.description,
        quantity,
    )
}

fn history_reply(activities: &[FarmActivity], filter: Option<&str>) -> String {
    if activities.is_empty() {
        return match filter {
            Some(activity_type) => format!(
                "Belum ada catatan aktivitas {} untuk tanaman aktif Anda.",
                activity_label(activity_type).to_lowercase()
            ),
            None => "Belum ada aktivitas yang tercatat untuk tanaman aktif Anda.".into(),
        };
    }

    let mut lines = vec![if activities.len() == 1 {
        "Aktivitas terakhir yang tercatat:".to_owned()
    } else {
        "Riwayat aktivitas terbaru:".to_owned()
    }];
    for (index, activity) in activities.iter().enumerate() {
        let quantity = match (activity.quantity.as_deref(), activity.unit.as_deref()) {
            (Some(quantity), Some(unit)) => {
                format!(", {} {unit}", display_quantity(quantity))
            }
            _ => String::new(),
        };
        lines.push(format!(
            "{}. {} — {}{}\n   {}",
            index + 1,
            jakarta_date(activity.performed_at),
            activity_label(&activity.activity_type),
            quantity,
            activity.description,
        ));
    }
    lines.join("\n")
}

fn activity_format_help() -> String {
    "Gunakan format *CATAT* lalu rincian aktivitas. Contoh:\n• CATAT hari ini menyiram semua greenhouse\n• CATAT kemarin memupuk NPK 25 kg\n\nAktivitas yang didukung: penyiraman, pemupukan, penyemprotan, pemangkasan, dan inspeksi."
        .to_owned()
}

fn activity_label(activity_type: &str) -> &'static str {
    match activity_type {
        "watering" => "Penyiraman",
        "fertilizing" => "Pemupukan",
        "spraying" => "Penyemprotan",
        "pruning" => "Pemangkasan",
        "inspection" => "Inspeksi",
        _ => "Aktivitas",
    }
}

fn display_quantity(value: &str) -> &str {
    if value.contains('.') {
        value.trim_end_matches('0').trim_end_matches('.')
    } else {
        value
    }
}

fn jakarta_date(value: DateTime<Utc>) -> String {
    let jakarta = FixedOffset::east_opt(7 * 60 * 60).expect("valid Jakarta offset");
    value.with_timezone(&jakarta).format("%d-%m-%Y").to_string()
}

fn normalize(value: &str) -> String {
    value.trim().to_lowercase()
}

fn is_affirmative(value: &str) -> bool {
    matches!(value, "ya" | "iya" | "benar" | "simpan" | "ok" | "oke")
}

fn is_cancel(value: &str) -> bool {
    matches!(value, "batal" | "cancel" | "berhenti")
}

fn required<'a>(value: &'a Option<String>, field: &str) -> Result<&'a str> {
    value
        .as_deref()
        .with_context(|| format!("activity draft is missing {field}"))
}

async fn record_via_farm_service(
    farm_service_url: &str,
    farm_token: &shared_auth::InternalToken,
    phone: &str,
    request_id: Uuid,
    draft: &Draft,
) -> Result<FarmActivity> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .post(format!(
            "{}/internal/activities",
            farm_service_url.trim_end_matches('/')
        ))
        .bearer_auth(farm_token.expose_for_request())
        .json(&serde_json::json!({
            "owner_phone": phone,
            "request_id": request_id,
            "activity_type": required(&draft.activity_type, "activity_type")?,
            "description": required(&draft.description, "description")?,
            "quantity": draft.quantity,
            "unit": draft.unit,
            "performed_at": draft.performed_at.context("missing performed_at")?,
        }))
        .send()
        .await
        .context("failed to reach farm-service")?;
    let status = response.status();
    anyhow::ensure!(status.is_success(), "farm-service returned {status}");
    response
        .json()
        .await
        .context("farm-service returned an invalid activity response")
}

async fn list_recent_via_farm_service(
    farm_service_url: &str,
    farm_token: &shared_auth::InternalToken,
    phone: &str,
    activity_type: Option<&str>,
    limit: i32,
) -> Result<Vec<FarmActivity>> {
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?
        .post(format!(
            "{}/internal/activities/recent",
            farm_service_url.trim_end_matches('/')
        ))
        .bearer_auth(farm_token.expose_for_request())
        .json(&serde_json::json!({
            "owner_phone": phone,
            "activity_type": activity_type,
            "limit": limit,
        }))
        .send()
        .await
        .context("failed to reach farm-service")?;
    let status = response.status();
    anyhow::ensure!(status.is_success(), "farm-service returned {status}");
    response
        .json()
        .await
        .context("farm-service returned invalid activity history")
}

async fn load_session(pool: &shared_db::DbPool, phone: &str) -> Result<Option<Session>> {
    let mut tx = phone_transaction(pool, phone).await?;
    let row = sqlx::query("SELECT current_step, draft FROM ai.get_activity_command_session()")
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;
    row.map(|row| {
        let step: String = row.try_get("current_step")?;
        let draft: serde_json::Value = row.try_get("draft")?;
        Ok(Session {
            step: Step::parse(&step)?,
            draft: serde_json::from_value(draft)
                .context("invalid activity command draft in database")?,
        })
    })
    .transpose()
}

async fn cached_response(
    pool: &shared_db::DbPool,
    phone: &str,
    request_id: Uuid,
) -> Result<Option<String>> {
    let mut tx = phone_transaction(pool, phone).await?;
    let response = sqlx::query_scalar("SELECT ai.get_activity_command_response($1)")
        .bind(request_id)
        .fetch_one(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(response)
}

async fn save_session(
    pool: &shared_db::DbPool,
    phone: &str,
    step: Step,
    draft: &Draft,
    request_id: Uuid,
    response: &str,
) -> Result<()> {
    let mut tx = phone_transaction(pool, phone).await?;
    sqlx::query("SELECT ai.save_activity_command_session($1,$2,$3,$4)")
        .bind(step.as_str())
        .bind(serde_json::to_value(draft)?)
        .bind(request_id)
        .bind(response)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn cancel_session(
    pool: &shared_db::DbPool,
    phone: &str,
    request_id: Uuid,
    response: &str,
) -> Result<()> {
    let mut tx = phone_transaction(pool, phone).await?;
    sqlx::query("SELECT ai.cancel_activity_command_session($1,$2)")
        .bind(request_id)
        .bind(response)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn complete_session(
    pool: &shared_db::DbPool,
    phone: &str,
    request_id: Uuid,
    response: &str,
) -> Result<()> {
    let mut tx = phone_transaction(pool, phone).await?;
    sqlx::query("SELECT ai.complete_activity_command_session($1,$2)")
        .bind(request_id)
        .bind(response)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

async fn phone_transaction<'a>(
    pool: &'a shared_db::DbPool,
    phone: &str,
) -> Result<sqlx::Transaction<'a, sqlx::Postgres>> {
    let normalized = crate::onboarding::normalize_phone(phone)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT set_config('app.current_phone', $1, TRUE)")
        .bind(normalized)
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 5, 12, 0, 0).unwrap()
    }

    #[test]
    fn parses_supported_activity_without_an_llm() {
        let draft = parse_draft("CATAT hari ini memupuk NPK 25 kg", now()).unwrap();
        assert_eq!(draft.activity_type.as_deref(), Some("fertilizing"));
        assert_eq!(draft.quantity.as_deref(), Some("25"));
        assert_eq!(draft.unit.as_deref(), Some("kg"));
        assert_eq!(draft.performed_at, Some(now()));
    }

    #[test]
    fn yesterday_is_deterministic_and_greenhouse_count_is_not_quantity() {
        let draft = parse_draft("catat kemarin menyiram 10 greenhouse", now()).unwrap();
        assert_eq!(draft.activity_type.as_deref(), Some("watering"));
        assert_eq!(draft.quantity, None);
        assert_eq!(draft.unit, None);
        assert_eq!(draft.performed_at, Some(now() - chrono::Duration::days(1)));
    }

    #[test]
    fn unsupported_or_unbounded_activity_is_rejected() {
        assert!(parse_draft("CATAT membeli bibit", now()).is_err());
        assert!(parse_draft(&format!("CATAT menyiram {}", "x".repeat(501)), now()).is_err());
        assert!(parse_draft("CATAT menyiram https://example.com", now()).is_err());
    }

    #[test]
    fn record_and_history_detection_are_narrow() {
        assert!(is_record_command("catat menyiram greenhouse"));
        assert!(!is_record_command("catat pengeluaran pupuk"));
        assert!(is_history_query("kapan terakhir saya memupuk?"));
        assert!(is_history_query("riwayat aktivitas"));
        assert!(!is_history_query("apa pupuk yang bagus?"));
    }

    #[test]
    fn confirmation_requires_explicit_user_control() {
        let draft = parse_draft("CATAT memupuk NPK 25 kg", now()).unwrap();
        let response = confirmation(&draft).unwrap();
        assert!(response.contains("YA"));
        assert!(response.contains("UBAH"));
        assert!(response.contains("BATAL"));
        assert!(response.contains("25 kg"));
    }

    #[test]
    fn stored_decimal_quantity_is_human_readable() {
        assert_eq!(display_quantity("25.000"), "25");
        assert_eq!(display_quantity("25.500"), "25.5");
        assert_eq!(display_quantity("100"), "100");
    }
}
