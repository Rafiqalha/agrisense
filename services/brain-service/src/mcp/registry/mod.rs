use shared_mcp::{McpTool, McpServiceManifest};
use std::collections::HashMap;
use tokio::sync::RwLock;

pub struct McpRegistry {
    tools: RwLock<HashMap<String, McpTool>>,
    services: RwLock<HashMap<String, McpServiceManifest>>,
}

impl McpRegistry {
    pub fn new() -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            services: RwLock::new(HashMap::new()),
        }
    }

    pub async fn register_service(&self, manifest: McpServiceManifest) {
        let mut tools = self.tools.write().await;
        for tool in &manifest.tools {
            tools.insert(tool.name.clone(), tool.clone());
        }
        let mut services = self.services.write().await;
        services.insert(manifest.service_name.clone(), manifest);
        tracing::info!("MCP registry updated, total tools: {}", tools.len());
    }

    pub async fn get_tool(&self, name: &str) -> Option<McpTool> {
        let tools = self.tools.read().await;
        tools.get(name).cloned()
    }

    pub async fn list_tools(&self) -> Vec<McpTool> {
        let tools = self.tools.read().await;
        tools.values().cloned().collect()
    }
}
