# AgriSense Event Catalog

All events are published to NATS JetStream on the `AGRISENSE` stream.
Subject format: `agrisense.<domain>.<event_type>`

## Farmer Events (`agrisense.farmer.*`)

| Event | Subject | Payload |
|-------|---------|---------|
| FarmerCreated | `agrisense.farmer.farmer_created` | farmer_id, name, phone, region |
| FarmerVerified | `agrisense.farmer.farmer_verified` | farmer_id, method, verified_at |
| FarmerSuspended | `agrisense.farmer.farmer_suspended` | farmer_id, reason |

## Farm Events (`agrisense.farm.*`)

| Event | Subject | Payload |
|-------|---------|---------|
| FarmRegistered | `agrisense.farm.farm_registered` | farm_id, farmer_id, location, area |
| CropPlanted | `agrisense.farm.crop_planted` | crop_id, farm_id, crop_type |
| HarvestRecorded | `agrisense.farm.harvest_recorded` | harvest_id, yield_kg, quality |
| InventoryLow | `agrisense.farm.inventory_low` | farm_id, item, quantity |

## Agronomy Events (`agrisense.agronomy.*`)

| Event | Subject | Payload |
|-------|---------|---------|
| DiseaseDetected | `agrisense.agronomy.disease_detected` | disease_id, severity, confidence |
| RecommendationGiven | `agrisense.agronomy.recommendation_given` | products, dosage |
| PestAlertIssued | `agrisense.agronomy.pest_alert_issued` | pest_name, severity |

## Finance Events (`agrisense.finance.*`)

| Event | Subject | Payload |
|-------|---------|---------|
| TransactionRecorded | `agrisense.finance.transaction_recorded` | amount_idr, type, category |
| LoanRequested | `agrisense.finance.loan_requested` | amount, loan_type, credit_score |
| LoanApproved | `agrisense.finance.loan_approved` | approved_amount, interest_rate |
| CreditScoreUpdated | `agrisense.finance.credit_score_updated` | old_score, new_score |

## AI Events (`agrisense.ai.*`)

| Event | Subject | Payload |
|-------|---------|---------|
| IntentDetected | `agrisense.ai.intent_detected` | intent, confidence, entities |
| AgentRunStarted | `agrisense.ai.agent_run_started` | run_id, agent_type |
| AgentRunCompleted | `agrisense.ai.agent_run_completed` | response, tools_used, latency_ms |
| VisionAnalysisCompleted | `agrisense.ai.vision_analysis_completed` | result, confidence |

## Consumer Groups (NATS Durable Consumers)

| Consumer | Subscribes To | Service |
|----------|--------------|---------|
| analytics-ingestion | `agrisense.>` | analytics-service |
| notification-trigger | `agrisense.agronomy.>`, `agrisense.finance.>` | platform-service |
| ai-audit | `agrisense.ai.>` | brain-service |
