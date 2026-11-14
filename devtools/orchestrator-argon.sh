#!/usr/bin/env bash
# devtools/orchestrator-argon.sh
# Coordinator/Orchestrator script using Antigravity Gemini 4 Argon (Plan 5).
# Role: Argon acts as the Chief Architect / Coordinator, planning and supervising
# tasks, while excluding itself (Plan 5) and delegating coding implementation to
# the coder pool: Plans 1, 2, 3, 4 (via --mode best / --mode rr / --plan 1..4).
#
# SDLC Commit Rule (all orchestrators):
#   Single commit after verification passes (including archiving issues/issue-<N>-*.md
#   to issues/archive/<YYYY>/<MM>/<DD>/ via mkdir -p + git mv):
#     feat(issue-<N>): implement <slug> (<duration> secs, 5h-token +<delta>% by <coder-model>)
set -eo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ARGON_MODEL="gemini-4-argon-high"
ORCHESTRATOR_MODEL="${ARGON_MODEL}"
CODER_POOL_IDS="1,2,3,4"
QUOTA_CHECKER="${SCRIPT_DIR}/quota_checker.py"
AI_CODER="${SCRIPT_DIR}/ai-coder.sh"

# shellcheck source=devtools/sdlc_common.sh
source "${SCRIPT_DIR}/sdlc_common.sh"

usage() {
    cat << 'EOF'
Usage:
  devtools/orchestrator-argon.sh "<high-level task description>" [options]
  devtools/orchestrator-argon.sh --issue <N> [--mode best|rr] [--plan 1..4] [--dry-run] [--max-retries N]
  devtools/orchestrator-argon.sh issue-<N> [--mode best|rr] [--plan 1..4] [--dry-run] [--max-retries N]
  devtools/orchestrator-argon.sh --all [--mode best|rr] [--plan 1..4] [--dry-run] [--max-retries N]

Description:
  Uses Gemini 4 Argon (Plan 5) as Chief Orchestrator. Because Argon quota is
  precious/limited, Argon EXCLUDES itself (Plan 5) from coding execution and
  dispatches coding tasks strictly across Plans 1, 2, 3, 4:
    - Plan 1: OpenCode Go (muse-spark-1.3-contributor)
    - Plan 2: MiniMax (minimax-m3)
    - Plan 3: Antigravity Gemini 3.8 Flash
    - Plan 4: Antigravity Claude Opus 5.5

SDLC Issue Lifecycle (single-commit with archive + metrics):
  1. before: quota_checker.py --json records PLAN_ID(1..4)/model/before_quota/start_ts
  2. run:    ai-coder.sh --mode <best|rr> --plan <PLAN_ID> executes implementation
  3. verify: regression-tests/verify-issue-<N>.sh (retry loop with error log)
  4. after:  end_ts/duration_sec/after_quota measurement
  5. commit (single commit): mkdir -p issues/archive/<YYYY>/<MM>/<DD> &&
         git mv issues/issue-<N>-*.md issues/archive/<YYYY>/<MM>/<DD>/ &&
         git commit -m "feat(issue-<N>): implement <slug> (<duration> secs, 5h-token +<delta>% by <coder-model>)"
         with trailers Orchestrator/Coder-Model/Duration/Quota-Before/Quota-After

Options:
  --issue <N>            Target issue number (accepts "11" or "issue-11")
  --all                  Run all pending issues in issues/issue-*.md sequentially
  --mode <best|rr>       Coder selection mode across Plans 1, 2, 3, 4 (default: best)
  --plan <1..4>          Explicit coder plan override (1..4 only; 5 is forbidden)
  --dry-run              Simulate without LLM calls or git commits
  --max-retries N        Max re-instruction retries on verify failure (default: 2)
  -h, --help             Show this help message
EOF
}

ISSUE_RAW=""
RUN_ALL=0
MODE="best"
EXPLICIT_PLAN=""
DRY_RUN=0
MAX_RETRIES=2

ARGS=()
while [ $# -gt 0 ]; do
    case "$1" in
        -h|--help)
            usage
            exit 0
            ;;
        --issue)
            if [ $# -lt 2 ]; then
                echo "❌ Error: --issue requires an issue number (e.g. --issue 11)" >&2
                exit 1
            fi
            ISSUE_RAW="$2"
            shift 2
            ;;
        --issue=*)
            ISSUE_RAW="${1#--issue=}"
            shift
            ;;
        --all)
            RUN_ALL=1
            shift
            ;;
        --mode)
            MODE="$2"
            shift 2
            ;;
        --mode=*)
            MODE="${1#--mode=}"
            shift
            ;;
        --plan)
            EXPLICIT_PLAN="$2"
            shift 2
            ;;
        --plan=*)
            EXPLICIT_PLAN="${1#--plan=}"
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --max-retries)
            MAX_RETRIES="$2"
            shift 2
            ;;
        --max-retries=*)
            MAX_RETRIES="${1#--max-retries=}"
            shift
            ;;
        issue-[0-9]*)
            if [ -z "$ISSUE_RAW" ]; then
                ISSUE_RAW="$1"
            else
                ARGS+=("$1")
            fi
            shift
            ;;
        *)
            ARGS+=("$1")
            shift
            ;;
    esac
done

if [ -n "$EXPLICIT_PLAN" ]; then
    case "$EXPLICIT_PLAN" in
        1|2|3|4) ;;
        *)
            echo "❌ Error: orchestrator-argon excludes Plan 5 (Argon) and only allows coder plans 1, 2, 3, 4 (got: ${EXPLICIT_PLAN})." >&2
            exit 1
            ;;
    esac
fi

if [ "$RUN_ALL" -eq 1 ]; then
    sdlc_run_all_pending
    exit $?
fi

if [ -n "$ISSUE_RAW" ]; then
    ISSUE_NUM="$(normalize_issue_num "$ISSUE_RAW")"
    if ! echo "$ISSUE_NUM" | grep -Eq '^[0-9]+$'; then
        echo "❌ Error: invalid issue number: $ISSUE_RAW" >&2
        exit 1
    fi
    sdlc_run_issue_lifecycle "$ISSUE_NUM"
    exit $?
fi

if [ "${#ARGS[@]}" -eq 0 ]; then
    usage
    exit 0
fi

TASK="${ARGS[*]}"

echo "=============================================================================="
echo " 🧠 [Argon Orchestrator] Task: ${TASK}"
echo " Coordinator: Gemini 4 Argon (Plan 5 — Excludes self from coding)"
echo " Coder Pool (1..4): Plan 1 (OpenCode), Plan 2 (MiniMax), Plan 3 (Gemini 3.8), Plan 4 (Claude)"
echo "=============================================================================="

QUOTA_STATUS=$("${SCRIPT_DIR}/quota_checker.py" --display)

ORCHESTRATION_PROMPT=$(cat <<EOF
당신은 현재 리포지터리($(basename "${REPO_ROOT}"))의 수석 아키텍트이자 조정자(Orchestrator)인 Gemini 4 Argon입니다.
자신(Plan 5 Argon)은 토큰이 부족하므로 코딩 실행에서 제외하고, 반드시 코더 풀 1, 2, 3, 4(Plan 1: OpenCode Muse, Plan 2: MiniMax-m3, Plan 3: Gemini 3.8 Flash, Plan 4: Claude Opus)만 호출해야 합니다.

[전체 코더 풀 및 현재 쿼터 현황]:
${QUOTA_STATUS}

[사용자 작업 요청]:
${TASK}

[SDLC 오케스트레이션 규칙 (모든 오케스트레이터 공통)]:
1. 코딩 작업은 에이전트(Plans 1~4)에게 \`./devtools/ai-coder.sh --mode best\` (또는 \`./devtools/orchestrator-argon.sh --issue <N>\`)로 위임합니다.
2. 오케스트레이터는 \`./regression-tests/verify-issue-<N>.sh\`로 검증한 뒤 실패 시 에러 로그를 포함해 재작업 지시하고, 통과 시 이슈 파일을 \`issues/archive/<YYYY>/<MM>/<DD>/\`로 이동하여 다음과 같은 **단일 커밋(Single Commit)** 으로 커밋합니다:
   \`feat(issue-<N>): implement <slug> (<duration> secs, 5h-token +<delta>% by <coder-model>)\`
EOF
)

echo "📡 Invoking Gemini 4 Argon for task orchestration..."
agy -p "${ORCHESTRATION_PROMPT}" --model "${ARGON_MODEL}" --dangerously-skip-permissions
