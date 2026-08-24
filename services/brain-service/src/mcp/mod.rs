pub mod registry;
pub mod tools;
pub mod permissions;

use axum::{Router, Json, extract::State};
use shared_mcp::{McpServiceManifest, McpToolCall, McpToolResult};

use crate::orchestrator::SharedOrchestrator;

pub fn router() -> Router<SharedOrchestrator> {
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

/// Register a service manifest (tools) into the live registry.
async fn register_service(
    State(orch): State<SharedOrchestrator>,
    Json(manifest): Json<McpServiceManifest>,
) -> Json<serde_json::Value> {
    let service_name = manifest.service_name.clone();
    let tool_count = manifest.tools.len();
    orch.mcp_registry.register_service(manifest).await;
    tracing::info!(service = %service_name, tool_count, "Service registered");
    Json(serde_json::json!({ "status": "registered", "tools": tool_count }))
}

/// List all registered tools — this is what agents see as capability space.
async fn list_tools(State(orch): State<SharedOrchestrator>) -> Json<serde_json::Value> {
    let tools = orch.mcp_registry.list_tools().await;
    Json(serde_json::json!({ "tools": tools }))
}

/// Execute a tool through the builtin executor.
/// Context is rebuilt from the caller's phone so ownership is verified
/// implicitly (Level 1 FarmerOwned read tools).
async fn call_tool(
    State(orch): State<SharedOrchestrator>,
    Json(call): Json<McpToolCall>,
) -> Json<McpToolResult> {
    let started = std::time::Instant::now();
    let phone = call.farmer_id.clone().unwrap_or_default();

    let result = match crate::context::build_farmer_context(&orch.db, &phone).await {
        Ok(ctx) => crate::mcp::tools::execute_tool(&orch.db, &call.tool_name, &ctx).await,
        Err(e) => Err(e),
    };

    match result {
        Ok(data) => Json(McpToolResult {
            tool_name: call.tool_name,
            success: true,
            data: Some(data),
            error: None,
            latency_ms: started.elapsed().as_millis() as u64,
        }),
        Err(e) => {
            tracing::warn!(error = %e, tool = %call.tool_name, "Tool call failed");
            Json(McpToolResult {
                tool_name: call.tool_name,
                success: false,
                data: None,
                error: Some(e.to_string()),
                latency_ms: started.elapsed().as_millis() as u64,
            })
        }
    }
}
