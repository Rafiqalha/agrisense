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
    pub expected_harvest_at: Option<NaiveDate>,
}

/// Build the farmer context by phone number.
///
/// Returns a context with `user_id: None` (gracefully) when the phone is not
/// registered — the agent loop must still respond politely in that case.
pub async fn build_farmer_context(pool: &shared_db::DbPool, phone: &str) -> Result<FarmerContext> {
    let row = sqlx::query(
        "SELECT u.id AS user_id, u.name AS user_name, f.name AS farm_name
         FROM identity.users u
         LEFT JOIN farm.farmers fr ON fr.user_id = u.id
         LEFT JOIN farm.farms f ON f.farmer_id = fr.id
         WHERE u.phone = $1
         LIMIT 1",
    )
    .bind(phone)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(FarmerContext {
            phone: phone.to_string(),
            user_id: None,
            farmer_name: None,
            farm_name: None,
            crop: None,
        });
    };

    let user_id: Option<Uuid> = row.try_get("user_id").ok();
    let farmer_name: Option<String> = row.try_get("user_name").ok();
    let farm_name: Option<String> = row.try_get("farm_name").ok();

    let crop = if user_id.is_some() {
        fetch_current_crop(pool, phone).await?
    } else {
        None
    };

    Ok(FarmerContext {
        phone: phone.to_string(),
        user_id,
        farmer_name,
        farm_name,
        crop,
    })
}

async fn fetch_current_crop(pool: &shared_db::DbPool, phone: &str) -> Result<Option<CropInfo>> {
    let row = sqlx::query(
        "SELECT c.crop_type::TEXT, c.seed_variety, c.planted_at, c.status,
                c.area_hectares::TEXT, c.expected_harvest_at
         FROM farm.crops c
         JOIN farm.farms f ON f.id = c.farm_id
         JOIN farm.farmers fr ON fr.id = f.farmer_id
         JOIN identity.users u ON u.id = fr.user_id
         WHERE u.phone = $1 AND c.status = 'growing'
         ORDER BY c.planted_at DESC
         LIMIT 1",
    )
    .bind(phone)
    .fetch_optional(pool)
    .await?;

    let Some(row) = row else {
        return Ok(None);
    };

    let planted_at: NaiveDate = row.try_get("planted_at")?;
    let age_days = age_in_days(planted_at, Utc::now().date_naive());

    Ok(Some(CropInfo {
        crop_type: row.try_get("crop_type")?,
        seed_variety: row.try_get("seed_variety").ok(),
        planted_at,
        age_days,
        status: row.try_get("status")?,
        area_hectares: row.try_get("area_hectares").ok(),
        expected_harvest_at: row.try_get("expected_harvest_at").ok(),
    }))
}

/// Deterministic crop age in days (pure, testable).
pub fn age_in_days(planted: NaiveDate, today: NaiveDate) -> i64 {
    (today - planted).num_days()
}

/// Human-friendly crop name: prefer seed variety for 'other' crops
/// (e.g. melon is stored as crop_type='other' + seed_variety='Melon').
pub fn friendly_crop_name(crop: &CropInfo) -> String {
    if crop.crop_type == "other" {
        crop
            .seed_variety
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
            expected_harvest_at: None,
        };
        assert_eq!(friendly_crop_name(&crop), "rice");
    }
}
