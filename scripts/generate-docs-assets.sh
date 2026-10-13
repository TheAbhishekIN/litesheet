#!/usr/bin/env bash
# Regenerate the README screenshots (docs/screenshots), the sample workbook
# (samples/sales.xlsx) and the app icon (assets/icon.png) by rendering the
# real app headlessly. Needs a GPU (Metal on macOS).
set -euo pipefail
cd "$(dirname "$0")/.."
cargo test --release generate_docs_assets -- --ignored
echo "Updated docs/screenshots, samples/sales.xlsx and assets/icon.png"
