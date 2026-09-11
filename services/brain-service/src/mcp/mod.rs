pub mod permissions;
pub mod registry;
pub mod tools;

use axum::{extract::State, http::HeaderMap, http::StatusCode, Json, Router};
use shared_mcp::{McpPermission, McpServiceManifest, McpToolCall, McpToolResult};

use crate::orchestrator::SharedOrchestrator;

pub fn router() -> Router<SharedOrchestrator> {
    Router::new()
        .route("/manifest", axum::routing::get(get_manifest))
        .route("/register", axum::routing::post(register_service))
        .route("/tools", axum::routing::get(list_tools))
        .route("/call", axum::routing::post(call_tool))
}

async fn get_manifest(
    headers: HeaderMap,
    State(orch): State<SharedOrchestrator>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_service(&headers, &orch.security.mcp_registration_token)?;
    Ok(Json(serde_json::json!({
        "service": "brain-service",
        "version": env!("CARGO_PKG_VERSION"),
        "description": "AgriSense MCP Registry & Orchestrator",
    })))
}

/// Register a service manifest (tools) into the live registry.
async fn register_service(
    headers: HeaderMap,
    State(orch): State<SharedOrchestrator>,
    Json(manifest): Json<McpServiceManifest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_service(&headers, &orch.security.mcp_registration_token)?;
    let service_name = manifest.service_name.clone();
    let tool_count = manifest.tools.len();
    orch.mcp_registry.register_service(manifest).await;
    tracing::info!(service = %service_name, tool_count, "Service registered");
    Ok(Json(
        serde_json::json!({ "status": "registered", "tools": tool_count }),
    ))
}

/// List all registered tools — this is what agents see as capability space.
async fn list_tools(
    headers: HeaderMap,
    State(orch): State<SharedOrchestrator>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    require_service(&headers, &orch.security.mcp_registration_token)?;
    let tools = orch.mcp_registry.list_tools().await;
    Ok(Json(serde_json::json!({ "tools": tools })))
}

/// Execute a tool through the builtin executor.
/// Context is rebuilt from the authenticated JWT subject. Caller-provided
/// farmer identifiers are never used for ownership decisions.
async fn call_tool(
    headers: HeaderMap,
    State(orch): State<SharedOrchestrator>,
    Json(call): Json<McpToolCall>,
) -> Result<Json<McpToolResult>, StatusCode> {
    let started = std::time::Instant::now();
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    let bearer = shared_auth::bearer_value(authorization).ok_or(StatusCode::UNAUTHORIZED)?;
    let claims = orch
        .security
        .user_auth
        .validate_token(bearer)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
    let _permit = orch
        .request_slots
        .clone()
        .try_acquire_owned()
        .map_err(|_| StatusCode::TOO_MANY_REQUESTS)?;
    let user_id = uuid::Uuid::parse_str(&claims.sub).map_err(|_| StatusCode::UNAUTHORIZED)?;
    let tool = orch
        .mcp_registry
        .get_tool(&call.tool_name)
        .await
        .ok_or(StatusCode::NOT_FOUND)?;
    let permission = parse_permission(tool.required_permission.as_deref())?;

    let ctx = crate::context::build_farmer_context_by_user_id(&orch.db, user_id)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, %user_id, "Authenticated MCP user was not found");
            StatusCode::FORBIDDEN
        })?;
    let owner_id = ctx.user_id.map(|id| id.to_string());
    if !permissions::can_call(
        &permission,
        &claims.role,
        owner_id.as_deref(),
        Some(&user_id.to_string()),
    ) {
        return Err(StatusCode::FORBIDDEN);
    }

    let result = crate::mcp::tools::execute_tool(&orch.db, &call.tool_name, &ctx).await;

    match result {
        Ok(data) => Ok(Json(McpToolResult {
            tool_name: call.tool_name,
            success: true,
            data: Some(data),
            error: None,
            latency_ms: started.elapsed().as_millis() as u64,
        })),
        Err(e) => {
            tracing::warn!(error = %e, tool = %call.tool_name, "Tool call failed");
            Ok(Json(McpToolResult {
                tool_name: call.tool_name,
                success: false,
                data: None,
                error: Some(e.to_string()),
                latency_ms: started.elapsed().as_millis() as u64,
            }))
        }
    }
}

fn require_service(
    headers: &HeaderMap,
    token: &shared_auth::InternalToken,
) -> Result<(), StatusCode> {
    let authorization = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    token
        .authorize_header(authorization)
        .then_some(())
        .ok_or(StatusCode::UNAUTHORIZED)
}

fn parse_permission(value: Option<&str>) -> Result<McpPermission, StatusCode> {
    match value {
        Some("public") => Ok(McpPermission::Public),
        Some("farmer_owned") => Ok(McpPermission::FarmerOwned),
        Some("admin_only") => Ok(McpPermission::AdminOnly),
        Some("partner_only") => Ok(McpPermission::PartnerOnly),
        _ => Err(StatusCode::FORBIDDEN),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_or_missing_permissions_fail_closed() {
        assert_eq!(parse_permission(None), Err(StatusCode::FORBIDDEN));
        assert_eq!(
            parse_permission(Some("future_permission")),
            Err(StatusCode::FORBIDDEN)
        );
    }
}
