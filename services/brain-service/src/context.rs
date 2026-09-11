//! Farmer Context — the domain source of truth for the agent loop.
//!
//! Principle: AI is the reasoning layer, the database is the source of truth.
//! This builder reads the farmer's real, persistent state (profile, farm,
//! current crop) from PostgreSQL so the agent never has to guess or hallucinate
//! domain facts (crop type, planting date, crop age).
//!
//! Memory split (see locked architecture):
//!   - Domain memory  → PostgreSQL (this module)
//!   - Session memory → Redis (conversation state, future slice)
//!   - Semantic memory→ pgvector RAG (agronomy knowledge, future slice)

use anyhow::Result;
use chrono::{NaiveDate, Utc};
use sqlx::Row;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct FarmerContext {
    /// WhatsApp phone number (E.164) — the lookup key for the WhatsApp channel.
    #[allow(dead_code)] // Retained for channel-aware authorization and auditing.
    pub phone: String,
    /// identity.users.id — `None` when the farmer is not registered yet.
    pub user_id: Option<Uuid>,
    pub farmer_name: Option<String>,
    pub farm_name: Option<String>,
    /// Current growing crop, if any.
    pub crop: Option<CropInfo>,
}

#[derive(Debug, Clone)]
pub struct CropInfo {
    /// Raw enum text from farm.crop_type ('rice', 'other', ...).
    pub crop_type: String,
    pub seed_variety: Option<String>,
    pub planted_at: NaiveDate,
    /// Days since planting, computed from the DB date (deterministic).
    pub age_days: i64,
    pub status: String,
    /// Area as TEXT (avoids BigDecimal dependency for the demo slice).
    pub area_hectares: Option<String>,
    pub cultivation_system: Option<String>,
    pub cultivation_unit_count: Option<i32>,
    pub area_per_unit_hectares: Option<String>,
    pub expected_harvest_at: Option<NaiveDate>,
}

/// Build the farmer context by phone number.
///
/// Returns a context with `user_id: None` (gracefully) when the phone is not
/// registered — the agent loop must still respond politely in that case.
pub async fn build_farmer_context(pool: &shared_db::DbPool, phone: &str) -> Result<FarmerContext> {
    let mut tx = pool.begin().await?;
    set_local(&mut tx, "app.current_phone", phone).await?;
    let row = sqlx::query("SELECT * FROM identity.whatsapp_farmer_context()")
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;

    let Some(row) = row else {
        return Ok(FarmerContext {
            phone: phone.to_string(),
            user_id: None,
            farmer_name: None,
            farm_name: None,
            crop: None,
        });
    };

    context_from_row(row)
}

/// Build context from the authenticated identity rather than caller-provided
/// request data. Used by the public MCP call boundary.
pub async fn build_farmer_context_by_user_id(
    pool: &shared_db::DbPool,
    authenticated_user_id: Uuid,
) -> Result<FarmerContext> {
    let mut tx = pool.begin().await?;
    set_local(
        &mut tx,
        "app.current_user_id",
        &authenticated_user_id.to_string(),
    )
    .await?;
    let row = sqlx::query("SELECT * FROM identity.user_context()")
        .fetch_optional(&mut *tx)
        .await?;
    tx.commit().await?;

    let row = row.ok_or_else(|| anyhow::anyhow!("authenticated user does not exist"))?;
    context_from_row(row)
}

async fn set_local(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
    value: &str,
) -> Result<()> {
    sqlx::query("SELECT set_config($1, $2, TRUE)")
        .bind(key)
        .bind(value)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

fn context_from_row(row: sqlx::postgres::PgRow) -> Result<FarmerContext> {
    let phone: String = row.try_get("phone")?;
    let planted_at: Option<NaiveDate> = row.try_get("planted_at")?;
    let crop = planted_at
        .map(|planted_at| {
            Ok::<CropInfo, anyhow::Error>(CropInfo {
                crop_type: row.try_get("crop_type")?,
                seed_variety: row.try_get("seed_variety")?,
                planted_at,
                age_days: age_in_days(planted_at, Utc::now().date_naive()),
                status: row.try_get("crop_status")?,
                area_hectares: row.try_get("area_hectares")?,
                cultivation_system: row.try_get("cultivation_system")?,
                cultivation_unit_count: row.try_get("cultivation_unit_count")?,
                area_per_unit_hectares: row.try_get("area_per_unit_hectares")?,
                expected_harvest_at: row.try_get("expected_harvest_at")?,
            })
        })
        .transpose()?;

    Ok(FarmerContext {
        phone,
        user_id: Some(row.try_get("user_id")?),
        farmer_name: row.try_get("farmer_name")?,
        farm_name: row.try_get("farm_name")?,
        crop,
    })
}

/// Deterministic crop age in days (pure, testable).
pub fn age_in_days(planted: NaiveDate, today: NaiveDate) -> i64 {
    (today - planted).num_days()
}

/// Human-friendly crop name: prefer the free-text label for uncatalogued crops.
pub fn friendly_crop_name(crop: &CropInfo) -> String {
    if crop.crop_type == "other" {
        crop.seed_variety
            .clone()
            .unwrap_or_else(|| "tanaman".to_string())
    } else {
        crop.crop_type.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    fn date(s: &str) -> NaiveDate {
        NaiveDate::parse_from_str(s, "%Y-%m-%d").unwrap()
    }

    #[test]
    fn age_is_days_since_planting() {
        assert_eq!(age_in_days(date("2026-07-15"), date("2026-08-16")), 32);
        assert_eq!(age_in_days(date("2026-08-16"), date("2026-08-16")), 0);
    }

    #[test]
    fn friendly_name_prefers_variety_for_other() {
        let crop = CropInfo {
            crop_type: "other".into(),
            seed_variety: Some("Melon (Golden Langkawi)".into()),
            planted_at: date("2026-07-15"),
            age_days: 32,
            status: "growing".into(),
            area_hectares: None,
            cultivation_system: None,
            cultivation_unit_count: None,
            area_per_unit_hectares: None,
            expected_harvest_at: None,
        };
        assert_eq!(friendly_crop_name(&crop), "Melon (Golden Langkawi)");
    }

    #[test]
    fn friendly_name_falls_back_to_type() {
        let crop = CropInfo {
            crop_type: "rice".into(),
            seed_variety: None,
            planted_at: date("2026-07-15"),
            age_days: 32,
            status: "growing".into(),
            area_hectares: None,
            cultivation_system: None,
            cultivation_unit_count: None,
            area_per_unit_hectares: None,
            expected_harvest_at: None,
        };
        assert_eq!(friendly_crop_name(&crop), "rice");
    }
}
