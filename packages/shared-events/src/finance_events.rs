use serde::{Deserialize, Serialize};
use shared_types::{FarmerId, TransactionId, Money};

// ─── Finance Domain Events ────────────────────────────────────────────────────
// NATS subjects: agrisense.finance.*

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecorded {
    pub transaction_id: TransactionId,
    pub farmer_id: FarmerId,
    pub amount: Money,
    pub category: String,
    pub description: Option<String>,
    pub transaction_type: shared_types::TransactionType,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanRequested {
    pub request_id: uuid::Uuid,
    pub farmer_id: FarmerId,
    pub amount_requested: Money,
    pub loan_type: LoanType,
    pub purpose: String,
    pub credit_score: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoanType {
    Kur,        // Kredit Usaha Rakyat
    Commercial,
    MicroLoan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanApproved {
    pub request_id: uuid::Uuid,
    pub farmer_id: FarmerId,
    pub approved_amount: Money,
    pub interest_rate: f32,
    pub tenure_months: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoanDisbursed {
    pub loan_id: uuid::Uuid,
    pub farmer_id: FarmerId,
    pub amount: Money,
    pub disbursed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditScoreUpdated {
    pub farmer_id: FarmerId,
    pub old_score: f32,
    pub new_score: f32,
    pub reason: String,
}
