#!/usr/bin/env bash
# devtools/sdlc_common.sh
# Shared SDLC Issue Lifecycle for generic orchestrators (Argon, MiniMax, or any model).
# Repo-independent: operates on issues/issue-<N>-*.md and regression-tests/verify-issue-<N>.sh
# relative to the repo root that contains devtools/. Copy devtools/ to any repo to reuse.
#
# Rules:
#   1. Orchestrator delegates implementation to a coder agent (Plans 1..4 via ai-coder.sh).
#   2. Orchestrator runs verification (regression-tests/verify-issue-<N>.sh).
#   3. On verification failure: re-instructs coder with failure log (up to MAX_RETRIES).
#   4. On verification pass: archives issue file to issues/archive/<YYYY>/<MM>/<DD>/ and
#      creates a SINGLE commit containing implementation + archived issue with subject:
#        feat(issue-<N>): implement <slug> (<duration> secs, 5h-token +<delta>% by <coder-model>)

normalize_issue_num() {
    echo "$1" | sed -E 's/^issue-//; s/^([0-9]+).*/\1/'
}

extract_5h_used_pct() {
    local quota_json="$1"
    local plan_id="$2"
    printf '%s' "$quota_json" | python3 -c "
import json, re, sys
pid = '${plan_id}'.strip()
try:
    data = json.load(sys.stdin)
    q = data.get(pid, {}).get('quota', {})
    detail = q.get('detail', '')
    m_used = re.search(r'5h used:\s*(\d+)%', detail)
    m_rem = re.search(r'5h rem:\s*(\d+)%', detail)
    if m_used:
        print(int(m_used.group(1)))
    elif m_rem:
        print(100 - int(m_rem.group(1)))
    else:
        rem = int(q.get('remaining_pct', 100))
        print(max(0, 100 - rem))
except Exception:
    print(0)
"
}

select_coder_plan() {
    if [ -n "${EXPLICIT_PLAN:-}" ]; then
        echo "$EXPLICIT_PLAN"
    elif [ "${MODE:-best}" = "rr" ]; then
        "${QUOTA_CHECKER}" --next-rr
    else
        "${QUOTA_CHECKER}" --best
    fi
}

sdlc_run_issue_lifecycle() {
    local ISSUE_N="$1"
    local ORCH_NAME="${ORCHESTRATOR_MODEL:-unknown-orchestrator}"

    local ISSUE_FILE=""
    ISSUE_FILE="$(ls "$REPO_ROOT"/issues/issue-"${ISSUE_N}"-*.md 2>/dev/null | head -n 1 || true)"
    if [ -z "$ISSUE_FILE" ]; then
        ISSUE_FILE="$(find "$REPO_ROOT/issues/archive" -name "issue-${ISSUE_N}-*.md" 2>/dev/null | head -n 1 || true)"
    fi
    if [ -z "$ISSUE_FILE" ]; then
        echo "❌ Error: issue file not found: issues/issue-${ISSUE_N}-*.md" >&2
        return 1
    fi
    local ISSUE_BASENAME ISSUE_SLUG VERIFY_SCRIPT
    ISSUE_BASENAME="$(basename "$ISSUE_FILE")"
    ISSUE_SLUG="$(echo "$ISSUE_BASENAME" | sed -E 's/^issue-[0-9]+-//; s/\.md$//')"
    VERIFY_SCRIPT="$REPO_ROOT/regression-tests/verify-issue-${ISSUE_N}.sh"

    echo "=============================================================================="
    echo " 🧠 [Orchestrator: ${ORCH_NAME}] Issue mode: issue-${ISSUE_N} (${ISSUE_BASENAME})"
    echo " Coder Pool: ${CODER_POOL_IDS:-1,2,3,4} | Mode: ${MODE:-best} | Dry-run: ${DRY_RUN:-0} | Max-retries: ${MAX_RETRIES:-2}"
    echo "=============================================================================="

    # 1. before: timestamp + quota snapshot
    local START_TS START_EPOCH QUOTA_JSON PLAN_ID CODER_MODEL BEFORE_QUOTA BEFORE_5H_PCT
    START_TS="$(date --iso-8601=seconds)"
    START_EPOCH="$(date +%s)"
    QUOTA_JSON="$("${QUOTA_CHECKER}" --json)"
    PLAN_ID="$(select_coder_plan)"
    CODER_MODEL="$(printf '%s' "$QUOTA_JSON" | python3 -c "
import json, sys
data = json.load(sys.stdin)
pid = '${PLAN_ID}'.strip()
print(data.get(pid, {}).get('model', 'unknown'))
")"
    BEFORE_QUOTA="$(printf '%s' "$QUOTA_JSON" | python3 -c "
import json, sys
data = json.load(sys.stdin)
parts = []
for pid in ('1','2','3','4'):
    q = data.get(pid, {}).get('quota', {})
    parts.append(f\"plan{pid}={q.get('remaining_pct',0)}%({q.get('status','?')})\")
print(','.join(parts))
")"
    BEFORE_5H_PCT="$(extract_5h_used_pct "$QUOTA_JSON" "$PLAN_ID")"

    echo "📊 [before] start_ts=${START_TS} plan=${PLAN_ID} model=${CODER_MODEL} 5h_used=${BEFORE_5H_PCT}%"
    echo "📊 [before] before_quota=${BEFORE_QUOTA}"

    # 2-3. delegate to coder + verify/re-instruct loop
    local CODER_PROMPT="issues/${ISSUE_BASENAME} 파일의 요구사항을 구현하라. 관련 코드를 수정/추가하고 regression-tests/verify-issue-${ISSUE_N}.sh 검수를 통과시킨 뒤 issues/${ISSUE_BASENAME} 의 '## 구현 결과' 섹션을 채워 넣어라 (git commit이나 git mv는 오케스트레이터가 수행하므로 직접 실행하지 말 것)."
    local attempt=0
    local VERIFY_LOG=""
    local LAST_VERIFY_RC=0

    if [ "${DRY_RUN:-0}" -eq 1 ]; then
        echo "[DRY-RUN] Would run: ./devtools/ai-coder.sh --mode ${MODE:-best} --plan ${PLAN_ID} \"${CODER_PROMPT}\""
        echo "[DRY-RUN] Would run: ./regression-tests/verify-issue-${ISSUE_N}.sh"
        echo "[DRY-RUN] Skipping live LLM invocation and verify execution (dry-run mode)."
    else
        local ERROR_CONTEXT=""
        while true; do
            if [ -z "${EXPLICIT_PLAN:-}" ] && [ "$attempt" -gt 0 ]; then
                PLAN_ID="$(select_coder_plan)"
            fi
            local TRY_PROMPT="$CODER_PROMPT"
            if [ -n "$ERROR_CONTEXT" ]; then
                TRY_PROMPT="$CODER_PROMPT 재지시: 이전 검수 실패 로그를 반영하여 수정하라. 실패 로그: $ERROR_CONTEXT"
            fi
            echo "🚀 [attempt $((attempt + 1))] Executing task with Plan ${PLAN_ID} (${CODER_MODEL})..."
            set +e
            "$AI_CODER" --mode "${MODE:-best}" --plan "$PLAN_ID" "$TRY_PROMPT"
            set -e
            if [ -x "$VERIFY_SCRIPT" ]; then
                echo "🔍 Running verification: $VERIFY_SCRIPT (attempt $((attempt + 1)))"
                set +e
                VERIFY_LOG="$("$VERIFY_SCRIPT" 2>&1)"
                LAST_VERIFY_RC=$?
                set -e
                echo "$VERIFY_LOG"
                if [ "$LAST_VERIFY_RC" -eq 0 ]; then
                    echo "✅ Verification passed on attempt $((attempt + 1))."
                    break
                fi
                echo "❌ Verification failed (rc=${LAST_VERIFY_RC}), preparing re-instruction..."
                ERROR_CONTEXT="$(printf '%s' "$VERIFY_LOG" | tail -n 30 | tr '\n' ' ' | cut -c1-2000)"
            else
                echo "⚠️  Verify script not found: $VERIFY_SCRIPT — skipping verification."
                break
            fi
            attempt=$((attempt + 1))
            if [ "$attempt" -gt "${MAX_RETRIES:-2}" ]; then
                echo "❌ Verification still failing after $((MAX_RETRIES + 1)) attempts. Aborting." >&2
                return 1
            fi
            echo "🔁 Re-instructing coder (retry ${attempt}/${MAX_RETRIES}) with failure log..."
        done
    fi

    # 4. after: end timestamp + duration + quota snapshot
    local END_TS END_EPOCH DURATION_SEC AFTER_QUOTA_JSON AFTER_QUOTA AFTER_5H_PCT DELTA_5H_PCT
    END_TS="$(date --iso-8601=seconds)"
    END_EPOCH="$(date +%s)"
    DURATION_SEC=$((END_EPOCH - START_EPOCH))
    AFTER_QUOTA_JSON="$(python3 "${QUOTA_CHECKER}" --json 2>/dev/null || printf '%s' "$QUOTA_JSON")"
    AFTER_QUOTA="$(printf '%s' "$AFTER_QUOTA_JSON" | python3 -c "
import json, sys
try:
    data = json.load(sys.stdin)
    parts = []
    for pid in ('1','2','3','4'):
        q = data.get(pid, {}).get('quota', {})
        parts.append(f\"plan{pid}={q.get('remaining_pct',0)}%({q.get('status','?')})\")
    print(','.join(parts))
except Exception:
    print('${BEFORE_QUOTA}')
")"
    if [ -z "$AFTER_QUOTA" ]; then
        AFTER_QUOTA="$BEFORE_QUOTA"
    fi
    AFTER_5H_PCT="$(extract_5h_used_pct "$AFTER_QUOTA_JSON" "$PLAN_ID")"
    DELTA_5H_PCT=$((AFTER_5H_PCT - BEFORE_5H_PCT))
    if [ "$DELTA_5H_PCT" -lt 0 ]; then
        DELTA_5H_PCT=0
    fi

    echo "📊 [after] end_ts=${END_TS} duration_sec=${DURATION_SEC}s 5h_used=${AFTER_5H_PCT}% (delta=+${DELTA_5H_PCT}%)"
    echo "📊 [after] after_quota=${AFTER_QUOTA}"

    # 5. Single commit: archive issue + stage implementation + commit with unified subject
    local ARCH_DATE_PATH ARCH_DIR COMMIT_SUBJECT TRAILERS COMMIT_MSG
    ARCH_DATE_PATH="$(date +%Y/%m/%d)"
    ARCH_DIR="issues/archive/${ARCH_DATE_PATH}"
    COMMIT_SUBJECT="feat(issue-${ISSUE_N}): implement ${ISSUE_SLUG} (${DURATION_SEC} secs, 5h-token +${DELTA_5H_PCT}% by ${CODER_MODEL})"
    TRAILERS="Orchestrator: ${ORCH_NAME}
Coder-Model: ${CODER_MODEL}
Duration: ${DURATION_SEC}s (${START_TS} -> ${END_TS})
Quota-Before: ${BEFORE_QUOTA}
Quota-After: ${AFTER_QUOTA}"
    COMMIT_MSG="${COMMIT_SUBJECT}

${TRAILERS}"

    if [ "${DRY_RUN:-0}" -eq 1 ]; then
        echo "=============================================================================="
        echo "[DRY-RUN] Archive + Single Commit preview:"
        echo "[DRY-RUN] Would archive: mkdir -p ${ARCH_DIR} && git mv issues/${ISSUE_BASENAME} ${ARCH_DIR}/"
        echo "[DRY-RUN] Would commit (single commit):"
        echo "$COMMIT_MSG"
        echo "=============================================================================="
        return 0
    fi

    echo "📦 Archiving ${ISSUE_BASENAME} to ${ARCH_DIR}/ and staging single commit..."
    mkdir -p "$REPO_ROOT/${ARCH_DIR}"
    if [ -f "$REPO_ROOT/issues/${ISSUE_BASENAME}" ]; then
        if git -C "$REPO_ROOT" ls-files --error-unmatch "issues/${ISSUE_BASENAME}" >/dev/null 2>&1; then
            git -C "$REPO_ROOT" mv "issues/${ISSUE_BASENAME}" "${ARCH_DIR}/"
        else
            mv "$REPO_ROOT/issues/${ISSUE_BASENAME}" "$REPO_ROOT/${ARCH_DIR}/"
        fi
    fi

    git -C "$REPO_ROOT" reset HEAD >/dev/null 2>&1 || true
    git -C "$REPO_ROOT" add -A
    for other_issue in "$REPO_ROOT"/issues/issue-*.md; do
        [ -e "$other_issue" ] || continue
        if [ "$(basename "$other_issue")" != "$ISSUE_BASENAME" ]; then
            git -C "$REPO_ROOT" reset HEAD "$other_issue" >/dev/null 2>&1 || true
        fi
    done

    if git -C "$REPO_ROOT" diff --cached --quiet; then
        echo "⚠️  Nothing staged for commit — skipping."
    else
        git -C "$REPO_ROOT" commit -m "$COMMIT_MSG"
        echo "✅ Committed: ${COMMIT_SUBJECT}"
    fi
}

sdlc_run_all_pending() {
    shopt -s nullglob
    local pending_files=("$REPO_ROOT"/issues/issue-*.md)
    shopt -u nullglob
    if [ "${#pending_files[@]}" -eq 0 ]; then
        echo "✅ No pending issues found in issues/."
        return 0
    fi
    local sorted_issues=()
    mapfile -t sorted_issues < <(
        for f in "${pending_files[@]}"; do
            local bn num
            bn="$(basename "$f")"
            num="$(normalize_issue_num "$bn")"
            printf '%s\t%s\n' "$num" "$bn"
        done | sort -n -k1,1 | cut -f1
    )
    for num in "${sorted_issues[@]}"; do
        sdlc_run_issue_lifecycle "$num"
    done
}
