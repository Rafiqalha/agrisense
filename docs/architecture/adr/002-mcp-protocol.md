# ADR 002: MCP as Core Integration Protocol

**Date:** 2026-08-15
**Status:** Accepted

## Context
AgriSense AI agents need to call domain services (farm lookup, disease check, etc.).
The architecture must allow model-agnostic tool calling.

## Decision
Use **Model Context Protocol (MCP)** as the standard interface for AI agent ↔ domain service communication.

## Rationale
- brain-service becomes an MCP Registry — all services register their tools
- Agents select tools from the registry without knowing service internals
- Swapping AI providers (Gemini → Claude) requires zero changes to tool definitions
- MCP is an open standard — future compatibility with external AI tools

## Tool Registry Design
```
brain-service/mcp/registry
  ← farm-service registers: get_farm_status, get_crop_status
  ← agronomy-service registers: detect_disease, recommend_fertilizer
  ← finance-service registers: record_expense, check_credit_score
  ← marketplace-service registers: list_products
```

## Consequences
AgriSense differentiator: agricultural MCP tool ecosystem.
External partners can register their own tools (IoT vendors, banks).
