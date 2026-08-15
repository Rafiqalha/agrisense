//! Disease Diagnosis Workflow
//!
//! Steps:
//!   1. detect_disease → ai-service (Gemini Vision via MCP)
//!   2. check_inventory → farm-service (via MCP/gRPC)
//!   3. recommend_product → agronomy-service (via MCP/gRPC)
//!   4. generate_voucher → marketplace-service (via MCP/gRPC)
//!   5. send_response → whatsapp-gateway (via platform-service)
//!
//! Each step is idempotent and can be retried independently.
