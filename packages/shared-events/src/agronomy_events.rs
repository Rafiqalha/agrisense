use serde::{Deserialize, Serialize};
use shared_types::{DiseaseId, FarmId, FarmerId, RecommendationId, SeverityLevel, CropType};

// ─── Agronomy Domain Events ───────────────────────────────────────────────────
// NATS subjects: agrisense.agronomy.*

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiseaseDetected {
    pub detection_id: uuid::Uuid,
    pub disease_id: DiseaseId,
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub disease_name: String,
    pub crop_type: CropType,
    pub severity: SeverityLevel,
    pub confidence_score: f32,
    pub detection_method: DetectionMethod,
    pub image_url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DetectionMethod {
    VisionAi,
    TextDescription,
    ManualReport,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationGiven {
    pub recommendation_id: RecommendationId,
    pub farmer_id: FarmerId,
    pub farm_id: FarmId,
    pub trigger: String, // "disease_detected" | "nutrient_deficiency" | "scheduled"
    pub product_names: Vec<String>,
    pub dosage_instructions: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NutrientDeficiencyDetected {
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub nutrient: String, // N, P, K, Mg, etc.
    pub severity: SeverityLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PestAlertIssued {
    pub farm_id: FarmId,
    pub farmer_id: FarmerId,
    pub pest_name: String,
    pub affected_area_percent: f32,
    pub severity: SeverityLevel,
}
