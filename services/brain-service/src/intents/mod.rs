//! Intent Detection
//!
//! Classifies incoming farmer messages into structured intents.
//! Delegates to ai-service for LLM-based classification.
//! Uses rule-based fallback for common patterns.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Intent {
    CheckStock,
    ReportDisease,
    AskWeather,
    RecordExpense,
    RecordRevenue,
    CheckHarvestStatus,
    AskFertilizerRecommendation,
    RequestLoan,
    CheckCreditScore,
    BuyProduct,
    CheckPrice,
    ReportActivity,
    Unknown,
}

pub struct IntentDetector;

impl IntentDetector {
    /// Detect intent from raw text.
    /// Priority: rule-based → LLM (via ai-service)
    pub async fn detect(message: &str) -> anyhow::Result<Intent> {
        let lower = message.to_lowercase();

        // Fast rule-based patterns for common intents
        if lower.contains("stok") || lower.contains("stock") || lower.contains("sisa") {
            return Ok(Intent::CheckStock);
        }
        if lower.contains("penyakit")
            || lower.contains("hama")
            || lower.contains("rusak")
            || lower.contains("layu")
        {
            return Ok(Intent::ReportDisease);
        }
        if lower.contains("cuaca") || lower.contains("hujan") || lower.contains("kemarau") {
            return Ok(Intent::AskWeather);
        }
        if lower.contains("catat") && (lower.contains("beli") || lower.contains("pengeluaran")) {
            return Ok(Intent::RecordExpense);
        }
        if lower.contains("jual") || lower.contains("penjualan") || lower.contains("pendapatan") {
            return Ok(Intent::RecordRevenue);
        }
        if lower.contains("pupuk") || lower.contains("rekomendasi") {
            return Ok(Intent::AskFertilizerRecommendation);
        }
        if lower.contains("pinjam") || lower.contains("kur") || lower.contains("kredit") {
            return Ok(Intent::RequestLoan);
        }
        if lower.contains("panen") || lower.contains("umur") || lower.contains("berapa lama") {
            return Ok(Intent::CheckHarvestStatus);
        }

        // Fallback to LLM classification via ai-service
        // TODO: call ai-service /classify endpoint
        Ok(Intent::Unknown)
    }
}
