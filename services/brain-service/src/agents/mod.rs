pub mod supervisor;

// Domain agents — stub modules for now.
// Each agent knows which MCP tools to call for its domain.
// Agents do NOT contain AI logic; they delegate to ai-service.

pub mod agronomist;
pub mod farm;
pub mod finance;
pub mod marketplace;
