#!/bin/bash
# ─── AgriSense: Start WhatsApp Webhook Tunnel ─────────────────────────────────
#
# Starts a Cloudflare Quick Tunnel that forwards Meta webhook requests
# to the local whatsapp-gateway on port 3001.
#
# Quick tunnel = no Cloudflare account needed, auto-generated URL.
# The URL changes on each restart — update Meta Dashboard accordingly.
#
# For persistent tunnel (production), use:
#   cloudflared tunnel login
#   cloudflared tunnel create agrisense
#   cloudflared tunnel route dns agrisense webhook.agrisense.id
#
# Usage:
#   ./scripts/tunnel.sh
#
# The public URL will be printed to stdout. Copy it to:
#   Meta Dashboard → WhatsApp → Configuration → Callback URL
#
# ─────────────────────────────────────────────────────────────────────────────

set -euo pipefail

PORT="${WHATSAPP_GATEWAY_PORT:-3001}"
CLOUDFLARED="${HOME}/.local/bin/cloudflared"

if ! command -v "$CLOUDFLARED" &>/dev/null; then
    CLOUDFLARED="cloudflared"
fi

echo "═══════════════════════════════════════════════════════════════"
echo "  🌾 AgriSense — WhatsApp Webhook Tunnel"
echo "═══════════════════════════════════════════════════════════════"
echo ""
echo "  Local:  http://localhost:${PORT}/webhook"
echo "  Tunnel: starting..."
echo ""
echo "  After the tunnel starts, copy the https://...trycloudflare.com URL"
echo "  to Meta Dashboard → WhatsApp → Configuration → Callback URL"
echo ""
echo "  Verify Token: ${WHATSAPP_WEBHOOK_VERIFY_TOKEN:-agrisense_wh_verify_2026_s3cure}"
echo ""
echo "═══════════════════════════════════════════════════════════════"
echo ""

exec "$CLOUDFLARED" tunnel --url "http://localhost:${PORT}" 2>&1
