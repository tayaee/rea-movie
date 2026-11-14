#!/usr/bin/env bash
# devtools/plan3-agy-gemini.sh
# Plan 3: agy (tool) => Antigravity => gemini 3.8 flash (model)
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL="gemini-3.8-flash-high"

echo "[Plan 3: Antigravity Gemini] Tool: agy | Model: ${MODEL}"

if [ $# -gt 0 ]; then
    PROMPT="$*"
    echo "[Plan 3] Running non-interactive prompt: ${PROMPT}"
    exec agy -p "${PROMPT}" --model "${MODEL}" --dangerously-skip-permissions
else
    echo "[Plan 3] Starting interactive session..."
    exec agy --model "${MODEL}" --dangerously-skip-permissions
fi
