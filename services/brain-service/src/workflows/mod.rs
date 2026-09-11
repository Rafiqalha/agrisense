//! Durable Workflow Engine
#![allow(dead_code)] // Workflow definitions are intentionally staged for a later milestone.
//!
//! Multi-step workflows persist their state to Postgres.
//! This means workflows survive crashes and can be resumed.
//!
//! Key design: workflow state is NOT just in RAM.
//!
//! Example: Diagnose Crop Disease
//!   Step 1: detect_disease (ai-service vision)
//!   Step 2: check_inventory (farm-service)
//!   Step 3: recommend_product (agronomy-service)
//!   Step 4: generate_voucher (marketplace-service)
//!   Step 5: send_whatsapp (platform-service)
//!
//! If Step 3 fails → workflow paused → retry from Step 3 (not from Step 1).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub mod diagnosis;
pub mod expense;
pub mod onboarding;

// ─── Workflow Type Registry ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowType {
    DiagnoseCropDisease,
    OnboardFarmer,
    RecordHarvest,
    ApplyForKur,
    PurchaseInputs,
    RecordExpense,
}

impl WorkflowType {
    /// Return the ordered steps for this workflow
    pub fn steps(&self) -> Vec<&'static str> {
        match self {
            Self::DiagnoseCropDisease => vec![
                "detect_disease",
                "check_inventory",
                "recommend_product",
                "generate_voucher",
                "send_response",
            ],
            Self::OnboardFarmer => vec![
                "collect_info",
                "verify_phone",
                "register_farmer",
                "register_farm",
                "welcome_message",
            ],
            Self::RecordHarvest => vec![
                "collect_harvest_data",
                "save_harvest",
                "update_crop_status",
                "emit_event",
            ],
            Self::ApplyForKur => vec![
                "check_eligibility",
                "calculate_credit_score",
                "submit_application",
                "notify_farmer",
            ],
            Self::PurchaseInputs => vec![
                "search_products",
                "check_kios_availability",
                "create_order",
                "notify_kios",
            ],
            Self::RecordExpense => vec![
                "parse_expense",
                "save_transaction",
                "update_cashflow",
                "confirm_to_farmer",
            ],
        }
    }
}

// ─── Workflow State Machine ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowRun {
    pub id: Uuid,
    pub conversation_id: Uuid,
    pub farmer_id: Uuid,
    pub workflow_type: WorkflowType,
    pub current_step: String,
    pub steps_completed: Vec<StepResult>,
    pub steps_remaining: Vec<String>,
    pub context: serde_json::Value,
    pub status: WorkflowStatus,
    pub error_message: Option<String>,
    pub retry_count: u16,
    pub started_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub timeout_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepResult {
    pub step: String,
    pub result_summary: serde_json::Value,
    pub completed_at: DateTime<Utc>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowStatus {
    Running,
    Paused,
    Completed,
    Failed,
    Cancelled,
    TimedOut,
}

impl WorkflowRun {
    /// Create a new workflow run
    pub fn new(conversation_id: Uuid, farmer_id: Uuid, workflow_type: WorkflowType) -> Self {
        let steps = workflow_type.steps();
        let current = steps[0].to_string();
        let remaining: Vec<String> = steps[1..].iter().map(|s| s.to_string()).collect();

        Self {
            id: Uuid::new_v4(),
            conversation_id,
            farmer_id,
            workflow_type,
            current_step: current,
            steps_completed: vec![],
            steps_remaining: remaining,
            context: serde_json::json!({}),
            status: WorkflowStatus::Running,
            error_message: None,
            retry_count: 0,
            started_at: Utc::now(),
            updated_at: Utc::now(),
            completed_at: None,
            timeout_at: Some(Utc::now() + chrono::Duration::hours(1)),
        }
    }

    /// Advance to the next step after successful completion
    pub fn advance(&mut self, result: StepResult) {
        self.steps_completed.push(result);
        self.updated_at = Utc::now();

        if let Some(next) = self.steps_remaining.first() {
            self.current_step = next.clone();
            self.steps_remaining.remove(0);
        } else {
            self.status = WorkflowStatus::Completed;
            self.completed_at = Some(Utc::now());
        }
    }

    /// Mark current step as failed
    pub fn fail(&mut self, error: String) {
        self.error_message = Some(error);
        self.retry_count += 1;
        self.updated_at = Utc::now();

        if self.retry_count >= 3 {
            self.status = WorkflowStatus::Failed;
        } else {
            self.status = WorkflowStatus::Paused;
        }
    }

    pub fn is_active(&self) -> bool {
        matches!(
            self.status,
            WorkflowStatus::Running | WorkflowStatus::Paused
        )
    }
}
