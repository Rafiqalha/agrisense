# AgriSense Architecture Overview

> AgriSense is not a farming chatbot.
> It is an Operating System for agricultural economic activity, powered by conversation.

## Core Principles

1. **Domain-Driven** — Domain boundaries, not technology layers
2. **Event-Driven** — Services communicate via events, not direct calls
3. **MCP-Centric** — AI agents interact with the world via MCP tools
4. **AI-as-a-Service** — AI is a capability layer, not the center of the system

## Service Map

```
WhatsApp Business API
        ↓
whatsapp-gateway (Axum)
        ↓
brain-service (Orchestrator + MCP Registry)
    ↓           ↓           ↓           ↓
farm-service  agronomy   finance   platform
                ↓
             ai-service (AiProvider trait)
               ├── Gemini Vision
               ├── Whisper STT
               └── pgvector RAG
                        ↓
              analytics-service (event consumer)
```

## Data Flow: Disease Report

```
Farmer sends photo on WhatsApp
        ↓
whatsapp-gateway receives webhook
        ↓
brain-service: detect intent = REPORT_DISEASE
        ↓
supervisor → agronomist_agent
        ↓
MCP tool: detect_disease (ai-service)
        ↓
Gemini Vision analyzes image
        ↓
MCP tool: recommend_fertilizer (agronomy-service)
        ↓
MCP tool: list_products (marketplace-service)
        ↓
Response sent back → WhatsApp
        ↓
Event: DiseaseDetected → analytics-service (async)
```

## Database Isolation

One PostgreSQL cluster, isolated schemas:
- `identity` — platform-service
- `farm` — farm-service
- `agronomy` — agronomy-service (+ pgvector)
- `finance` — finance-service
- `ai` — brain-service, ai-service
- `analytics` — analytics-service (read-heavy)

Each service owns its schema. Cross-domain = events, not JOINs.
