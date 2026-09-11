use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ─── Newtype IDs (type-safe identifiers) ─────────────────────────────────────
//
// Using newtype pattern prevents accidentally passing a FarmId where
// a FarmerId is expected. Compile-time safety at zero runtime cost.

macro_rules! define_id {
    ($name:ident) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }

        impl From<Uuid> for $name {
            fn from(id: Uuid) -> Self {
                Self(id)
            }
        }
    };
}

define_id!(FarmerId);
define_id!(FarmId);
define_id!(GreenhouseId);
define_id!(CropId);
define_id!(HarvestId);
define_id!(ActivityId);
define_id!(DiseaseId);
define_id!(PestId);
define_id!(RecommendationId);
define_id!(TransactionId);
define_id!(ProductId);
define_id!(OrderId);
define_id!(VendorId);
define_id!(ConversationId);
define_id!(MessageId);
define_id!(AgentRunId);
define_id!(WorkflowId);
define_id!(UserId);

// ─── Domain Enums ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CropType {
    Rice,
    Corn,
    Soybean,
    Sugarcane,
    Cassava,
    Tomato,
    Chili,
    Cabbage,
    Shallot,
    Melon,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FarmerStatus {
    Active,
    Inactive,
    Suspended,
    PendingVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FarmSize {
    Small,      // < 0.5 ha
    Medium,     // 0.5 - 2 ha
    Large,      // 2 - 10 ha
    Enterprise, // > 10 ha
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SeverityLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransactionType {
    Expense,
    Revenue,
    Transfer,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MessageChannel {
    WhatsApp,
    Sms,
    Email,
    Push,
    InApp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UserRole {
    Farmer,
    Kios,
    Supplier,
    BankPartner,
    IoTVendor,
    Admin,
    SuperAdmin,
}

// ─── Value Objects ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Money {
    pub amount_idr: i64, // stored in Rupiah, integer (avoid floating point)
}

impl Money {
    pub fn from_idr(amount: i64) -> Self {
        Self { amount_idr: amount }
    }

    pub fn format(&self) -> String {
        format!("Rp {:}", self.amount_idr)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhoneNumber {
    pub value: String, // E.164 format: +6281234567890
}

impl PhoneNumber {
    pub fn new(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaUrl {
    pub url: String,
    pub media_type: MediaType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaType {
    Image,
    Audio,
    Video,
    Document,
}

// ─── Common Result ─────────────────────────────────────────────────────────────

pub type AppResult<T> = Result<T, AppError>;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Validation error: {0}")]
    Validation(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("External service error: {0}")]
    ExternalService(String),
}
