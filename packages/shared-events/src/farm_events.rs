use serde::{Deserialize, Serialize};
use shared_types::{FarmId, FarmerId, CropId, CropType, HarvestId, ActivityId};

// ─── Farm Domain Events ───────────────────────────────────────────────────────
// NATS subjects: agrisense.farm.*

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmRegistered {
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub name: String,
    pub location: shared_types::GeoLocation,
    pub area_hectares: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CropPlanted {
    pub crop_id: CropId,
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub crop_type: CropType,
    pub planted_at: chrono::DateTime<chrono::Utc>,
    pub area_hectares: f64,
    pub seed_variety: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarvestRecorded {
    pub harvest_id: HarvestId,
    pub crop_id: CropId,
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub yield_kg: f64,
    pub quality_grade: String,
    pub harvested_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivityLogged {
    pub activity_id: ActivityId,
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub activity_type: String, // fertilizing, irrigation, spraying, etc.
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InventoryLow {
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub item_name: String,
    pub current_quantity: f64,
    pub unit: String,
    pub threshold: f64,
}
