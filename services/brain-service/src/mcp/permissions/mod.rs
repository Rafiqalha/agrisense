//! Permission checker for MCP tool calls.
//! Validates that the calling agent has permission to use a given tool.

use shared_mcp::McpPermission;
use shared_types::UserRole;

pub fn can_call(
    permission: &McpPermission,
    caller_role: &UserRole,
    resource_owner_id: Option<&str>,
    caller_id: Option<&str>,
) -> bool {
    match permission {
        McpPermission::Public => true,
        McpPermission::FarmerOwned => {
            resource_owner_id.is_some()
                && caller_id.is_some()
                && resource_owner_id == caller_id
        }
        McpPermission::AdminOnly => matches!(
            caller_role,
            UserRole::Admin | UserRole::SuperAdmin
        ),
        McpPermission::PartnerOnly => matches!(
            caller_role,
            UserRole::Kios | UserRole::Supplier | UserRole::BankPartner
        ),
    }
}
