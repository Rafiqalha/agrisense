//! Multi-step workflows for complex farmer requests.
//!
//! Example workflow: Disease Diagnosis
//!   1. detect_disease (vision)
//!   2. check_inventory (farm)
//!   3. recommend_product (agronomy)
//!   4. generate_voucher (marketplace)
//!
//! Workflows are pure orchestration — they call MCP tools sequentially.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WorkflowType {
    DiagnoseCropDisease,
    OnboardFarmer,
    RecordHarvest,
    ApplyForKur,
    PurchaseInputs,
}
