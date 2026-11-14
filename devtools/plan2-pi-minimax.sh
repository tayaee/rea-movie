#!/usr/bin/env bash
# devtools/plan2-pi-minimax.sh
# Plan 2: pi (tool) => minimax (provider) => minimax-m3 (model)
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROVIDER="minimax"
MODEL="minimax-m3"

# Load MINIMAX_API_KEY from repo-root .env if not already set in environment.
if [ -z "${MINIMAX_API_KEY:-}" ]; then
    REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
    if [ -f "${REPO_ROOT}/.env" ]; then
        ENV_KEY="$(grep -E '^[[:space:]]*MINIMAX_API_KEY=' "${REPO_ROOT}/.env" 2>/dev/null | tail -n 1 | cut -d= -f2- || true)"
        ENV_KEY="$(printf '%s' "${ENV_KEY}" | tr -d '\r' | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//' -e 's/^"//' -e 's/"$//' -e "s/^'//" -e "s/'$//")"
        if [ -n "${ENV_KEY}" ]; then
            export MINIMAX_API_KEY="${ENV_KEY}"
        fi
    fi
fi

# Fail fast when no key is available from environment or .env.
if [ -z "${MINIMAX_API_KEY:-}" ]; then
    echo "Error: MINIMAX_API_KEY is not set in environment or .env" >&2
    exit 1
fi

echo "[Plan 2: MiniMax] Tool: pi | Provider: ${PROVIDER} | Model: ${MODEL}"

if [ $# -gt 0 ]; then
    PROMPT="$*"
    echo "[Plan 2] Running non-interactive prompt: ${PROMPT}"
    exec pi --provider "${PROVIDER}" --model "${MODEL}" -p "${PROMPT}"
else
    echo "[Plan 2] Starting interactive session..."
    exec pi --provider "${PROVIDER}" --model "${MODEL}"
fi
