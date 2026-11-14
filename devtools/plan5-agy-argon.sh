#!/usr/bin/env bash
# devtools/plan5-agy-argon.sh
# Plan 5: agy (tool) => Antigravity => gemini argon (model)
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL="gemini-4-argon-high"

echo "[Plan 5: Antigravity Argon] Tool: agy | Model: ${MODEL}"

if [ $# -gt 0 ]; then
    PROMPT="$*"
    echo "[Plan 5] Running non-interactive prompt: ${PROMPT}"
    exec agy -p "${PROMPT}" --model "${MODEL}" --dangerously-skip-permissions
else
    echo "[Plan 5] Starting interactive session..."
    exec agy --model "${MODEL}" --dangerously-skip-permissions
fi
