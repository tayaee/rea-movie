#!/usr/bin/env bash
# devtools/plan1-opencode.sh
# Plan 1: opencode (tool) => opencode.ai (provider) => opencode-go (plan) => muse-spark-1.3-contributor (model)
# Generic agentic-coding launcher. Copy this file with devtools/ to any repo.
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL="opencode-go/muse-spark-1.3-contributor"

# OPENCODE_API_KEY must be provided via environment (or .env loaded by the caller).
if [ -z "${OPENCODE_API_KEY:-}" ]; then
    echo "Error: OPENCODE_API_KEY is not set in environment" >&2
    exit 1
fi

echo "[Plan 1: OpenCode Go] Tool: opencode | Provider: opencode.ai | Model: ${MODEL}"

if [ $# -gt 0 ]; then
    PROMPT="$*"
    echo "[Plan 1] Running non-interactive prompt: ${PROMPT}"
    exec opencode run -m "${MODEL}" --auto "${PROMPT}"
else
    echo "[Plan 1] Starting interactive session..."
    export OPENCODE_CONFIG_CONTENT="{\"model\":\"${MODEL}\"}"
    exec opencode --auto
fi
