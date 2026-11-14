#!/usr/bin/env bash
# devtools/plan4-agy-claude.sh
# Plan 4: agy (tool) => Antigravity => claude opus 5.5 (model)
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
MODEL="claude-opus-5-5-high"

FALLBACK_ENABLED=0
PROMPT_ARGS=()

for arg in "$@"; do
    if [ "$arg" == "--fallback" ]; then
        FALLBACK_ENABLED=1
    elif [ "$arg" == "--no-fallback" ]; then
        FALLBACK_ENABLED=0
    else
        PROMPT_ARGS+=("$arg")
    fi
done

PROMPT="${PROMPT_ARGS[*]:-}"

echo "[Plan 4: Antigravity Claude] Tool: agy | Model: ${MODEL}"

# Check quota first if prompt mode
if [ -n "${PROMPT}" ]; then
    # Test if agy call succeeds
    echo "[Plan 4] Attempting execution with ${MODEL}..."
    TMP_ERR=$(mktemp)
    if agy -p "${PROMPT}" --model "${MODEL}" --dangerously-skip-permissions 2>"$TMP_ERR"; then
        rm -f "$TMP_ERR"
        exit 0
    fi

    # Check if failed due to RESOURCE_EXHAUSTED / quota
    if grep -q -E "RESOURCE_EXHAUSTED|Individual quota reached" "$TMP_ERR" 2>/dev/null; then
        echo "⚠️  [Plan 4 Quota Notice] Claude Opus 5.5 weekly quota is currently EXHAUSTED (0% remaining)."
        cat "$TMP_ERR" >&2
        rm -f "$TMP_ERR"

        if [ "${FALLBACK_ENABLED}" -eq 1 ] || [ "${AUTO_FALLBACK:-1}" -eq 1 ]; then
            BEST_PLAN=$("${SCRIPT_DIR}/quota_checker.py" --best 2>/dev/null || echo "3")
            echo "➡️  [Plan 4 Fallback] Automatically delegating to highest-quota plan (Plan ${BEST_PLAN})..."
            exec "${SCRIPT_DIR}"/plan${BEST_PLAN}-*.sh "${PROMPT}"
        else
            echo "❌ [Plan 4] Quota exhausted and fallback disabled. Exiting with error."
            exit 42
        fi
    else
        cat "$TMP_ERR" >&2
        rm -f "$TMP_ERR"
        exit 1
    fi
else
    echo "[Plan 4] Starting interactive session..."
    exec agy --model "${MODEL}" --dangerously-skip-permissions
fi
