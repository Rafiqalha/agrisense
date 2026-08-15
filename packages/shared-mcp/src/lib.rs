use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ─── MCP Tool Definition ──────────────────────────────────────────────────────
//
// Every service registers its tools with the brain-service MCP registry.
// Tools are called by agents during orchestration.
//
// AgriSense MCP Tools:
//   get_farmer_profile        → platform-service
//   get_farm_status           → farm-service
//   get_crop_status           → farm-service
//   detect_disease            → ai-service (vision)
//   recommend_fertilizer      → agronomy-service
//   check_weather             → external (BMKG)
//   record_expense            → finance-service
//   check_credit_score        → finance-service
//   request_kur               → finance-service
//   list_marketplace_products → marketplace-service
//   get_harvest_history       → farm-service

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    /// Unique tool name (snake_case). Used by agents to call this tool.
    pub name: String,
    /// Human-readable description for the LLM to understand what this tool does.
    pub description: String,
    /// JSON Schema for input parameters.
    pub input_schema: Value,
    /// Which service owns this tool.
    pub service: String,
    /// Required permission to call this tool.
    pub required_permission: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolCall {
    pub tool_name: String,
    pub arguments: Value,
    pub caller_agent: String,
    pub farmer_id: Option<String>,
    pub correlation_id: Option<uuid::Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolResult {
    pub tool_name: String,
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    pub latency_ms: u64,
}

// ─── MCP Tool Handler Trait ───────────────────────────────────────────────────
//
// Each service implements this trait for each tool it exposes.
//

#[async_trait]
pub trait McpToolHandler: Send + Sync {
    /// Return the tool definition (used for registration)
    fn definition(&self) -> McpTool;

    /// Execute the tool with given arguments
    async fn execute(&self, call: McpToolCall) -> McpToolResult;
}

// ─── MCP Manifest ─────────────────────────────────────────────────────────────
//
// A service publishes its manifest to the brain-service on startup.
// The registry aggregates all manifests.
//

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServiceManifest {
    pub service_name: String,
    pub service_version: String,
    pub tools: Vec<McpTool>,
    pub endpoint: String,
}

// ─── Permission Levels ────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum McpPermission {
    Public,         // any agent can call
    FarmerOwned,    // only when farmer_id matches
    AdminOnly,      // internal tools
    PartnerOnly,    // kios/supplier/bank
}

// ─── Error ────────────────────────────────────────────────────────────────────

#[derive(Debug, thiserror::Error)]
pub enum McpError {
    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("Invalid arguments: {0}")]
    InvalidArguments(String),

    #[error("Tool execution failed: {0}")]
    ExecutionFailed(String),
}
