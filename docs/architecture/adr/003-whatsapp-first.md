# ADR 003: WhatsApp First

**Date:** 2026-08-15
**Status:** Accepted

## Context
Indonesian farmers' primary digital interface is WhatsApp. Smartphone penetration is high,
but app download friction is a barrier.

## Decision
WhatsApp is the primary and first-class channel. UI/UX is designed for chat-first interactions.

## Rationale
- 100M+ Indonesian WhatsApp users
- Zero app install friction
- Familiar interface for farmers
- Supports: text, image (disease photo), voice note (accessibility)
- Meta Business API is stable and well-documented

## Channel Priority
1. WhatsApp (primary)
2. SMS (fallback for non-smartphone)
3. App (future, for power users)
4. Web (admin/partner only)

## Consequences
whatsapp-gateway is a critical service.
All user journeys are designed for conversational flow.
Media handling (image, voice) is a first-class concern from Day 1.
