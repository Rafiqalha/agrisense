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
            resource_owner_id.is_some() && caller_id.is_some() && resource_owner_id == caller_id
        }
        McpPermission::AdminOnly => matches!(caller_role, UserRole::Admin | UserRole::SuperAdmin),
        McpPermission::PartnerOnly => matches!(
            caller_role,
            UserRole::Kios | UserRole::Supplier | UserRole::BankPartner
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn farmer_owned_requires_the_authenticated_owner() {
        assert!(can_call(
            &McpPermission::FarmerOwned,
            &UserRole::Farmer,
            Some("user-1"),
            Some("user-1")
        ));
        assert!(!can_call(
            &McpPermission::FarmerOwned,
            &UserRole::Farmer,
            Some("user-1"),
            Some("user-2")
        ));
        assert!(!can_call(
            &McpPermission::FarmerOwned,
            &UserRole::Farmer,
            Some("user-1"),
            None
        ));
    }

    #[test]
    fn privileged_permissions_are_role_scoped() {
        assert!(can_call(
            &McpPermission::AdminOnly,
            &UserRole::Admin,
            None,
            None
        ));
        assert!(!can_call(
            &McpPermission::AdminOnly,
            &UserRole::Farmer,
            None,
            None
        ));
        assert!(can_call(
            &McpPermission::PartnerOnly,
            &UserRole::BankPartner,
            None,
            None
        ));
    }
}
