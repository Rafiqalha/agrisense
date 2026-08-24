//! Orchestrator — the minimum viable agent loop.
//!
//! Locked architecture: no agent framework, no planner, no autonomous swarm.
//! One loop that actually works:
//!
//!   Message
//!     → FarmerContext (PostgreSQL — domain source of truth)
//!     → Intent (rule-based, LLM fallback later)
//!     → Tool selection + execution (MCP, Level 1 read tools)
//!     → LLM reasoning (via ai-service /generate)
//!     → Agent run audit (ai.agent_runs)
//!     → Response
//!
//! The AI model NEVER invents domain facts: crop type, planting date and
//! crop age come from the database via tools. The LLM only reasons over them.

use axum::{Router, Json, extract::State};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

use crate::context::{FarmerContext, friendly_crop_name};
use crate::mcp::registry::McpRegistry;
use crate::intents::{Intent, IntentDetector};

pub struct Orchestrator {
    pub mcp_registry: McpRegistry,
    pub cache: shared_cache::CacheClient,
    pub nats: async_nats::Client,
    pub db: shared_db::DbPool,
    pub ai_service_url: String,
}

impl Orchestrator {
    pub fn new(
        mcp_registry: McpRegistry,
        cache: shared_cache::CacheClient,
        nats: async_nats::Client,
        db: shared_db::DbPool,
        ai_service_url: String,
    ) -> Self {
        Self { mcp_registry, cache, nats, db, ai_service_url }
    }

    /// The minimum viable agent loop.
    pub async fn process(&self, request: OrchestrateRequest) -> anyhow::Result<OrchestrateResponse> {
        let started = std::time::Instant::now();
        tracing::info!(
            farmer_id = %request.farmer_id,
            conversation_id = %request.conversation_id,
            "Processing message"
        );

        // 1. Domain context (source of truth — never hallucinated)
        let ctx = crate::context::build_farmer_context(&self.db, &request.farmer_id).await?;
        tracing::debug!(user_id = ?ctx.user_id, crop = ?ctx.crop.as_ref().map(|c| &c.crop_type), "Farmer context loaded");

        // 2. Intent
        let intent = IntentDetector::detect(&request.message).await?;
        tracing::debug!(intent = ?intent, "Intent detected");

        // 3. Tool selection + execution (Level 1 read tools only in this slice)
        let mut tools_called: Vec<serde_json::Value> = Vec::new();
        let tool_result: Option<serde_json::Value> = match intent {
            Intent::CheckHarvestStatus | Intent::CheckStock => {
                let args = json!({ "farmer_phone": request.farmer_id });
                match crate::mcp::tools::execute_tool(&self.db, "farm.get_current_crop", &ctx).await {
                    Ok(value) => {
                        tools_called.push(json!({
                            "name": "farm.get_current_crop",
                            "args": args,
                            "result_summary": value,
                            "success": true,
                        }));
                        Some(value)
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "Tool execution failed");
                        tools_called.push(json!({
                            "name": "farm.get_current_crop",
                            "args": args,
                            "error": e.to_string(),
                            "success": false,
                        }));
                        None
                    }
                }
            }
            _ => None,
        };

        // 4. LLM reasoning (via ai-service — the AI capability layer)
        let user_prompt = build_user_prompt(&ctx, &request.message, tool_result.as_ref());
        let gen = self
            .call_ai_generate(SYSTEM_PROMPT.to_string(), user_prompt)
            .await?;

        // 5. Audit — persist the run BEFORE responding (system correctness)
        if let Some(user_id) = ctx.user_id {
            let conversation_id =
                crate::audit::get_or_create_conversation(&self.db, user_id, "whatsapp").await;
            let audit = crate::audit::AgentRunAudit {
                conversation_id: conversation_id.ok(),
                farmer_id: user_id,
                agent_type: "agronomist".into(),
                input_intent: Some(format!("{:?}", intent)),
                input_message: request.message.clone(),
                response: gen.content.clone(),
                model_provider: Some("gemini".into()),
                model_name: Some(gen.model.clone()),
                tools_called: if tools_called.is_empty() { None } else { Some(tools_called.clone()) },
                latency_ms: started.elapsed().as_millis() as i64,
                success: true,
            };
            if let Err(e) = crate::audit::insert_agent_run(&self.db, &audit).await {
                tracing::warn!(error = %e, "Failed to persist agent run audit");
            }
        } else {
            tracing::warn!(phone = %request.farmer_id, "Unknown farmer — audit skipped");
        }

        Ok(OrchestrateResponse {
            conversation_id: request.conversation_id,
            agent_used: "agronomist".into(),
            response: gen.content,
            tools_used: tools_called
                .iter()
                .filter_map(|t| t["name"].as_str().map(String::from))
                .collect(),
        })
    }

    /// Call the ai-service /generate endpoint (AI capability layer).
    /// brain-service itself has NO direct model access — this keeps
    /// provider swapping a deployment decision, not a code change.
    async fn call_ai_generate(&self, system_prompt: String, user_prompt: String) -> anyhow::Result<AiGenerateResponse> {
        let url = format!("{}/generate", self.ai_service_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "system_prompt": system_prompt,
            "messages": [{ "role": "user", "content": user_prompt }],
            "temperature": 0.3,
        });

        let client = reqwest::Client::new();
        let resp = client.post(&url).json(&body).send().await?;
        let status = resp.status();
        let json: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            anyhow::bail!("ai-service /generate failed ({}): {}", status, json);
        }
        Ok(serde_json::from_value(json)?)
    }
}

// ─── Prompts ──────────────────────────────────────────────────────────────────

const SYSTEM_PROMPT: &str = "\
Anda adalah asisten pertanian AgriSense untuk petani Indonesia. \
Anda menjawab dalam Bahasa Indonesia yang santun, ringkas, dan praktis.

Aturan penting:
1. Data domain dari tool (umur tanaman, jenis, tanggal tanam, status) adalah SUMBER KEBENARAN. \
Jangan pernah mengarang angka atau fakta yang bertentangan dengan data yang diberikan.
2. Jika data tanaman tersedia, sebutkan umur tanaman dan informasinya secara alami.
3. Jika data tidak tersedia, katakan jujur dan tawarkan langkah berikutnya.
4. Jangan menyebutkan bahwa Anda membaca data dari database atau tool — bicaralah seperti asisten yang mengenal kebun petani.";

/// Build the user prompt: farmer context + question + tool result (if any).
fn build_user_prompt(
    ctx: &FarmerContext,
    message: &str,
    tool_result: Option<&serde_json::Value>,
) -> String {
    let mut ctx_lines = Vec::new();
    ctx_lines.push(format!("- Nama: {}", ctx.farmer_name.as_deref().unwrap_or("(belum dikenal)")));
    if let Some(farm) = &ctx.farm_name {
        ctx_lines.push(format!("- Kebun: {}", farm));
    }
    if let Some(crop) = &ctx.crop {
        ctx_lines.push(format!(
            "- Tanaman aktif: {} (umur {} hari, ditanam {}, status {})",
            friendly_crop_name(crop),
            crop.age_days,
            crop.planted_at,
            crop.status,
        ));
    } else {
        ctx_lines.push("- Tanaman aktif: tidak ada".to_string());
    }

    let mut prompt = format!(
        "Konteks petani:\n{}\n\nPertanyaan petani: {}\n",
        ctx_lines.join("\n"),
        message
    );

    if let Some(result) = tool_result {
        prompt.push_str(&format!(
            "\nHasil tool (data domain, gunakan sebagai fakta):\n{}",
            serde_json::to_string_pretty(result).unwrap_or_default()
        ));
    }

    prompt
}

// ─── DTOs ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct OrchestrateRequest {
    /// WhatsApp phone number for the WhatsApp channel (farmer lookup key).
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
    /// The reply text — whatsapp-gateway sends this back to the farmer.
    pub response: String,
    pub tools_used: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct AiGenerateResponse {
    content: String,
    model: String,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{CropInfo, FarmerContext};
    use chrono::NaiveDate;

    fn ctx_with_crop() -> FarmerContext {
        FarmerContext {
            phone: "+6281234567890".into(),
            user_id: None,
            farmer_name: Some("Pak Tani".into()),
            farm_name: Some("Kebun Melon".into()),
            crop: Some(CropInfo {
                crop_type: "other".into(),
                seed_variety: Some("Melon (Golden Langkawi)".into()),
                planted_at: NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
                age_days: 32,
                status: "growing".into(),
                area_hectares: None,
                expected_harvest_at: None,
            }),
        }
    }

    #[test]
    fn user_prompt_embeds_domain_facts_and_tool_result() {
        let ctx = ctx_with_crop();
        let tool = serde_json::json!({ "tool": "farm.get_current_crop", "crop": { "age_days": 32 } });
        let prompt = build_user_prompt(&ctx, "Berapa umur tanaman melon saya?", Some(&tool));
        assert!(prompt.contains("umur 32 hari"));
        assert!(prompt.contains("Melon (Golden Langkawi)"));
        assert!(prompt.contains("farm.get_current_crop"));
        assert!(prompt.contains("Berapa umur tanaman melon saya?"));
    }

    #[test]
    fn user_prompt_without_tool_still_has_context() {
        let ctx = ctx_with_crop();
        let prompt = build_user_prompt(&ctx, "Halo", None);
        assert!(prompt.contains("umur 32 hari"));
        assert!(!prompt.contains("farm.get_current_crop"));
    }
}
