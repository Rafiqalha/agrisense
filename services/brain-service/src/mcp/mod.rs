pub mod registry;
pub mod tools;
pub mod permissions;

use axum::{Router, Json, extract::State};
use shared_mcp::{McpServiceManifest, McpToolCall, McpToolResult};

pub fn router() -> Router<crate::orchestrator::SharedOrchestrator> {
    Router::new()
        .route("/manifest", axum::routing::get(get_manifest))
        .route("/register", axum::routing::post(register_service))
        .route("/tools", axum::routing::get(list_tools))
        .route("/call", axum::routing::post(call_tool))
}

async fn get_manifest() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "brain-service",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "AgriSense MCP Registry & Orchestrator",
    }))
}

async fn register_service(Json(manifest): Json<McpServiceManifest>) -> Json<serde_json::Value> {
    tracing::info!(service = %manifest.service_name, tools = manifest.tools.len(), "Service registered");
    Json(serde_json::json!({ "status": "registered" }))
}

async fn list_tools() -> Json<serde_json::Value> {
    // TODO: return from registry
    Json(serde_json::json!({ "tools": [] }))
}

async fn call_tool(Json(call): Json<McpToolCall>) -> Json<McpToolResult> {
    // TODO: route to correct service handler
    Json(McpToolResult {
        tool_name: call.tool_name,
        success: false,
        data: None,
        error: Some("Not yet implemented".into()),
        latency_ms: 0,
    })
}
