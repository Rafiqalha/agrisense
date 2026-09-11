use serde::{Deserialize, Serialize};
use shared_types::{AgentRunId, ConversationId, FarmerId};

// ─── AI Domain Events ─────────────────────────────────────────────────────────
// NATS subjects: agrisense.ai.*

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentDetected {
    pub conversation_id: ConversationId,
    pub farmer_id: FarmerId,
    pub raw_message: String,
    pub detected_intent: String,
    pub confidence: f32,
    pub entities: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunStarted {
    pub run_id: AgentRunId,
    pub conversation_id: ConversationId,
    pub farmer_id: FarmerId,
    pub agent_type: String,
    pub input_intent: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunCompleted {
    pub run_id: AgentRunId,
    pub conversation_id: ConversationId,
    pub farmer_id: FarmerId,
    pub response: String,
    pub tools_used: Vec<String>,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentRunFailed {
    pub run_id: AgentRunId,
    pub conversation_id: ConversationId,
    pub error_type: String,
    pub error_message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionAnalysisCompleted {
    pub analysis_id: uuid::Uuid,
    pub farmer_id: FarmerId,
    pub image_url: String,
    pub analysis_type: VisionAnalysisType,
    pub result: serde_json::Value,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisionAnalysisType {
    DiseaseDetection,
    PestIdentification,
    HarvestReadiness,
    NutrientDeficiency,
    General,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpeechTranscribed {
    pub transcription_id: uuid::Uuid,
    pub farmer_id: FarmerId,
    pub audio_url: String,
    pub transcript: String,
    pub language: String,
    pub confidence: f32,
}
