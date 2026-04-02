#!/usr/bin/env bash
# Production build: compile Tailwind CSS then build optimised WASM.
#
# First-time setup:
#   npm install -D @tailwindcss/cli     (Tailwind v4 CLI)
#   cargo install trunk
#
# Then just run:  ./build.sh

set -e

echo "▶ Compiling Tailwind CSS…"
# Tailwind v4 uses @tailwindcss/cli (not the old npx tailwindcss)
npx @tailwindcss/cli -i input.css -o output.css --minify

echo "▶ Patching index.html for production (swap CDN → compiled CSS)…"
# Replace the Play CDN block with a compiled stylesheet link
sed -i '' \
  's|<script src="https://cdn.tailwindcss.com">.*||' \
  index.html 2>/dev/null || true

echo "▶ Building WASM (release, opt-level=z)…"
trunk build --release

echo "✓ Output in ./dist/"
