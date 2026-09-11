//! Built-in MCP tools executed by brain-service itself.
//!
//! In this slice only Level 1 (read) tools exist — `farm.get_current_crop`.
//! Level 2 (recommendation) and Level 3 (side effects: order, loan) tools
//! arrive in later slices together with the policy + confirmation engine.
//!
//! Tool execution NEVER invents domain facts: every value comes from the
//! farmer context built from PostgreSQL (deterministic source of truth).

use serde_json::json;
use shared_mcp::McpTool;

use crate::context::{friendly_crop_name, FarmerContext};

/// Tool definitions registered into the MCP registry at startup.
/// Other services will register their own tools via /mcp/register later.
pub fn builtin_tools() -> Vec<McpTool> {
    vec![McpTool {
        name: "farm.get_current_crop".into(),
        description: "Get the farmer's current growing crop: type, planting date, age in days, status, and expected harvest. Use when the farmer asks about their crop status, crop age, or harvest readiness.".into(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "farmer_phone": {
                    "type": "string",
                    "description": "Farmer's WhatsApp phone number (E.164)"
                }
            },
            "required": ["farmer_phone"]
        }),
        service: "farm-service".into(),
        required_permission: Some("farmer_owned".into()),
    }]
}

/// Execute a builtin tool for the given farmer context.
/// The context was built from the caller's phone, so ownership is implicit
/// (Level 1 read permission — FarmerOwned).
pub async fn execute_tool(
    _pool: &shared_db::DbPool,
    tool_name: &str,
    ctx: &FarmerContext,
) -> anyhow::Result<serde_json::Value> {
    match tool_name {
        "farm.get_current_crop" => Ok(get_current_crop(ctx)),
        other => anyhow::bail!("Unknown builtin tool: {}", other),
    }
}

fn get_current_crop(ctx: &FarmerContext) -> serde_json::Value {
    let Some(crop) = &ctx.crop else {
        return json!({
            "tool": "farm.get_current_crop",
            "crop": null,
            "message": "Belum ada tanaman aktif yang tercatat untuk petani ini."
        });
    };

    json!({
        "tool": "farm.get_current_crop",
        "crop": {
            "crop_name": friendly_crop_name(crop),
            "crop_type": crop.crop_type,
            "seed_variety": crop.seed_variety,
            "planted_at": crop.planted_at.to_string(),
            "age_days": crop.age_days,
            "status": crop.status,
            "area_hectares": crop.area_hectares,
            "cultivation_system": crop.cultivation_system,
            "cultivation_unit_count": crop.cultivation_unit_count,
            "area_per_unit_hectares": crop.area_per_unit_hectares,
            "expected_harvest_at": crop.expected_harvest_at.map(|d| d.to_string()),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::CropInfo;
    use chrono::NaiveDate;

    fn crop() -> CropInfo {
        CropInfo {
            crop_type: "other".into(),
            seed_variety: Some("Melon (Golden Langkawi)".into()),
            planted_at: NaiveDate::from_ymd_opt(2026, 7, 15).unwrap(),
            age_days: 32,
            status: "growing".into(),
            area_hectares: Some("0.0500".into()),
            cultivation_system: Some("greenhouse".into()),
            cultivation_unit_count: Some(10),
            area_per_unit_hectares: Some("0.0050".into()),
            expected_harvest_at: None,
        }
    }

    #[test]
    fn tool_returns_age_and_name() {
        let ctx = FarmerContext {
            phone: "+6281234567890".into(),
            user_id: None,
            farmer_name: Some("Pak Tani".into()),
            farm_name: Some("Kebun Melon".into()),
            crop: Some(crop()),
        };
        let out = get_current_crop(&ctx);
        assert_eq!(out["crop"]["crop_name"], "Melon (Golden Langkawi)");
        assert_eq!(out["crop"]["age_days"], 32);
        assert_eq!(out["crop"]["status"], "growing");
    }

    #[test]
    fn tool_handles_no_crop() {
        let ctx = FarmerContext {
            phone: "+6281234567890".into(),
            user_id: None,
            farmer_name: None,
            farm_name: None,
            crop: None,
        };
        let out = get_current_crop(&ctx);
        assert!(out["crop"].is_null());
    }
}
