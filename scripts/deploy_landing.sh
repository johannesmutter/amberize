#!/usr/bin/env bash
# Deploy the landing page to Fly.io.
#
# Fetches the latest download stats from GitHub before building,
# so the site always has up-to-date version numbers and download counts.
#
# Usage:
#   ./scripts/deploy_landing.sh          # deploy to default region (fra)
#   ./scripts/deploy_landing.sh fra      # deploy to a specific region
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"
LANDING_DIR="$ROOT_DIR/apps/landing"
REGION="${1:-}"

echo "==> Fetching download stats from GitHub..."
python3 "$SCRIPT_DIR/fetch_download_stats.py"
echo ""

echo "==> Deploying landing page to Fly.io..."
cd "$LANDING_DIR"
if [ -n "$REGION" ]; then
  fly deploy -r "$REGION"
else
  fly deploy
fi

echo ""
echo "==> Done. Site is live at https://amberize.fly.dev/"
