//! Orchestrator — the core message processing pipeline.
//!
//! Flow:
//!   IncomingMessage
//!     → IntentDetector
//!     → AgentSelector
//!     → MCP Tool Selection
//!     → Tool Execution (via registered tools)
//!     → Response Generation
//!     → Memory Update

use axum::{Router, Json, extract::State};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::mcp::registry::McpRegistry;
use crate::intents::IntentDetector;

pub struct Orchestrator {
    pub mcp_registry: McpRegistry,
    pub cache: shared_cache::CacheClient,
    pub nats: async_nats::Client,
}

impl Orchestrator {
    pub fn new(
        mcp_registry: McpRegistry,
        cache: shared_cache::CacheClient,
        nats: async_nats::Client,
    ) -> Self {
        Self { mcp_registry, cache, nats }
    }

    pub async fn process(&self, request: OrchestrateRequest) -> anyhow::Result<OrchestrateResponse> {
        tracing::info!(
            farmer_id = %request.farmer_id,
            conversation_id = %request.conversation_id,
            "Processing message"
        );

        // 1. Detect intent
        let intent = IntentDetector::detect(&request.message).await?;
        tracing::debug!(intent = ?intent, "Intent detected");

        // 2. Select agent based on intent
        let agent_type = crate::agents::supervisor::SupervisorAgent::select_agent(&intent);
        tracing::debug!(agent = %agent_type, "Agent selected");

        // 3. Execute via MCP tools
        // (Agent queries MCP registry, selects tools, executes)
        // This keeps brain-service as pure orchestrator — no AI logic here

        Ok(OrchestrateResponse {
            conversation_id: request.conversation_id,
            agent_used: agent_type,
            response: format!("Intent: {:?}", intent), // placeholder
            tools_used: vec![],
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct OrchestrateRequest {
    pub farmer_id: String,
    pub conversation_id: String,
    pub message: String,
    #[serde(default)]
    pub media_urls: Vec<String>,
    pub channel: String,
}

#[derive(Debug, Serialize)]
pub struct OrchestrateResponse {
    pub conversation_id: String,
    pub agent_used: String,
    pub response: String,
    pub tools_used: Vec<String>,
}

// Use Arc<Orchestrator> as Axum state — Orchestrator holds non-Clone resources
pub type SharedOrchestrator = Arc<Orchestrator>;

pub fn router() -> Router<SharedOrchestrator> {
    Router::new()
        .route("/", axum::routing::post(handle_orchestrate))
}

async fn handle_orchestrate(
    State(orchestrator): State<SharedOrchestrator>,
    Json(req): Json<OrchestrateRequest>,
) -> Json<OrchestrateResponse> {
    match orchestrator.process(req).await {
        Ok(resp) => Json(resp),
        Err(e) => {
            tracing::error!(error = %e, "Orchestration failed");
            Json(OrchestrateResponse {
                conversation_id: String::new(),
                agent_used: "error".into(),
                response: "Maaf, terjadi kesalahan. Coba lagi.".into(),
                tools_used: vec![],
            })
        }
    }
}
