#!/usr/bin/env bash
# devtools/ai-coder.sh
# Master AI Coder wrapper & multi-plan orchestrator (generic, repo-independent).
# Copy devtools/ to any repo and use as-is.
#
# Plans:
#   1) opencode => opencode.ai => opencode-go => muse-spark-1.3-contributor
#   2) pi => minimax => minimax-m3
#   3) agy => gemini 3.8 flash (gemini-3.8-flash-high)
#   4) agy => claude opus 5.5 (claude-opus-5-5-high)
#   5) agy => gemini argon (gemini-4-argon-high)
#
# Modes:
#   - default: Selects the plan with the highest remaining quota (best-quota)
#   - rr / --mode rr: Cycles through plans in round-robin order (skips exhausted plans)
#   - 1..5 / --plan 1..5: Explicitly invokes plan N
#   - quota / 0: Displays quota usage for all plans
#   - orchestrator / coord: Uses Argon as orchestrator and others as coders
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
QUOTA_CHECKER="${SCRIPT_DIR}/quota_checker.py"

usage() {
    cat << 'EOF'
Usage: devtools/ai-coder.sh [options] [command | prompt]

Commands:
  quota | 0              Display quota status table for all 5 plans
  best "<prompt>"        Execute prompt using plan with highest remaining quota (default)
  rr "<prompt>"          Execute prompt using round-robin among plans with available quota
  1..5 "<prompt>"        Execute prompt using specific plan (1 to 5)
  interactive [1-5]      Launch interactive session for selected plan (or best quota)
  orchestrator "<task>"  Run Argon orchestrator mode (Argon plans, coders execute)

Options:
  --mode <best|rr>       Selection mode (default: best)
  --plan <1-5>           Explicit plan override
  --interactive, -i      Start interactive TUI session
  --help, -h             Show this help message

Examples:
  ./devtools/ai-coder.sh quota
  ./devtools/ai-coder.sh "README.md에 아키텍처 설명 추가해줘"
  ./devtools/ai-coder.sh rr "fintools 테스트 코드 추가해줘"
  ./devtools/ai-coder.sh 3 "gemini로 빠른 리팩토링 수행해줘"
  ./devtools/ai-coder.sh orchestrator "25개 이슈 로드맵 중 issue-1 상세 구현 계획 수립"
EOF
}

MODE="best"
TARGET_PLAN=""
IS_INTERACTIVE=0
PROMPT_PARTS=()

# Parse arguments
while [ $# -gt 0 ]; do
    case "$1" in
        quota|0)
            exec "${QUOTA_CHECKER}" --display
            ;;
        orchestrator|coord)
            shift
            exec "${SCRIPT_DIR}/orchestrator-argon.sh" "$@"
            ;;
        rr)
            MODE="rr"
            shift
            ;;
        best)
            MODE="best"
            shift
            ;;
        [1-5])
            TARGET_PLAN="$1"
            shift
            ;;
        interactive|-i)
            IS_INTERACTIVE=1
            shift
            ;;
        --mode)
            MODE="$2"
            shift 2
            ;;
        --plan)
            TARGET_PLAN="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        *)
            PROMPT_PARTS+=("$1")
            shift
            ;;
    esac
done

PROMPT="${PROMPT_PARTS[*]:-}"

# If no target plan specified, select based on mode
if [ -z "${TARGET_PLAN}" ]; then
    if [ "${MODE}" == "rr" ]; then
        TARGET_PLAN=$("${QUOTA_CHECKER}" --next-rr)
        echo "🔄 [Round-Robin Mode] Selected Plan ${TARGET_PLAN}"
    else
        TARGET_PLAN=$("${QUOTA_CHECKER}" --best)
        echo "⭐ [Best Quota Mode] Selected Plan ${TARGET_PLAN} (Highest Remaining Quota)"
    fi
fi

TARGET_SCRIPT="${SCRIPT_DIR}/plan${TARGET_PLAN}-"*.sh

if [ ! -f ${TARGET_SCRIPT} ]; then
    echo "❌ Error: Target script for plan ${TARGET_PLAN} not found." >&2
    exit 1
fi

if [ "${IS_INTERACTIVE}" -eq 1 ] || [ -z "${PROMPT}" ]; then
    echo "🚀 Launching interactive session with Plan ${TARGET_PLAN}..."
    exec ${TARGET_SCRIPT}
else
    echo "🚀 Executing task with Plan ${TARGET_PLAN}..."
    exec ${TARGET_SCRIPT} "${PROMPT}"
fi
