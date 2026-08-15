use serde::{Deserialize, Serialize};
use shared_types::{FarmerId, PhoneNumber, UserRole};

// ─── Farmer Domain Events ─────────────────────────────────────────────────────
// NATS subjects: agrisense.farmer.*

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmerCreated {
    pub farmer_id: FarmerId,
    pub name: String,
    pub phone: PhoneNumber,
    pub region: String,
    pub referral_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmerVerified {
    pub farmer_id: FarmerId,
    pub verified_at: chrono::DateTime<chrono::Utc>,
    pub verification_method: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmerProfileUpdated {
    pub farmer_id: FarmerId,
    pub fields_changed: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmerSuspended {
    pub farmer_id: FarmerId,
    pub reason: String,
    pub suspended_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FarmerRoleChanged {
    pub farmer_id: FarmerId,
    pub old_role: UserRole,
    pub new_role: UserRole,
}
