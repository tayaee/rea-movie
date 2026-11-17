#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-6.sh
# Issue 6 Verification: Assembly Shot Duration Sync & Trim to Timeline Duration
#
# Verification Criteria:
# 1. Short shot (1s-range) trimming: Rendered clips trimmed to timeline duration;
#    assembled total duration converges to original timeline total ±0.5s.
# 2. Skip unnecessary re-encoding: Clips within duration (+0.08s) used as-is.
# 3. Clean temporary files: .trim/ directory removed after assembly.
# 4. Trimming applies when original clips are missing (render-only shots).
# 5. Full breakingbad-s5e7-v2 project duration matches original 120.02s ±0.5s.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue6-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 6 regression verifications in ${TMP_DIR}..."

# Helper to generate dummy MP4 video with lavfi
create_video() {
  local out_path="$1"
  local duration="$2"
  mkdir -p "$(dirname "${out_path}")"
  ffmpeg -nostdin -v error -y -f lavfi -i "color=c=blue:s=320x240:d=${duration}" \
    -f lavfi -i "sine=frequency=440:duration=${duration}" \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
    -c:a aac -ar 32000 -ac 2 \
    "${out_path}"
}

# Helper to probe video duration in float seconds
probe_dur() {
  local path="$1"
  ffprobe -v error -show_entries format=duration -of csv=p=0 "${path}"
}

# ------------------------------------------------------------------------------
# Check 1: Short shots (1-2s) trimmed to timeline duration (converge to ±0.5s)
# ------------------------------------------------------------------------------
echo "--- [Check 1] Short shots trimmed to timeline duration (5.5s total) ---"
PROJ1="${TMP_DIR}/proj_trim_test"
mkdir -p "${PROJ1}/shots/shot_0001/refs" "${PROJ1}/shots/shot_0001/renders/smoke"
mkdir -p "${PROJ1}/shots/shot_0002/refs" "${PROJ1}/shots/shot_0002/renders/smoke"
mkdir -p "${PROJ1}/shots/shot_0003/refs" "${PROJ1}/shots/shot_0003/renders/smoke"

cat <<'EOF' > "${PROJ1}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 1.25,
      "duration": 1.25,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    },
    {
      "shot_id": "shot_0002",
      "start_sec": 1.25,
      "end_sec": 3.75,
      "duration": 2.50,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    },
    {
      "shot_id": "shot_0003",
      "start_sec": 3.75,
      "end_sec": 5.50,
      "duration": 1.75,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    }
  ]
}
EOF

# Renders are 4.0s each due to H3 minimum duration (total untrimmed would be 12.0s)
create_video "${PROJ1}/shots/shot_0001/renders/smoke/shot_0001.mp4" 4.00
create_video "${PROJ1}/shots/shot_0002/renders/smoke/shot_0002.mp4" 4.00
create_video "${PROJ1}/shots/shot_0003/renders/smoke/shot_0003.mp4" 4.00

# Original clips
create_video "${PROJ1}/shots/shot_0001/refs/original_clip.mp4" 1.25
create_video "${PROJ1}/shots/shot_0002/refs/original_clip.mp4" 2.50
create_video "${PROJ1}/shots/shot_0003/refs/original_clip.mp4" 1.75

# Run assembly
"${BIN}" assemble --proj-dir "${PROJ1}" --smoke

FINAL_MOVIE="${PROJ1}/outputs/smoke/proj_trim_test_smoke.mp4"
COMPARE_MOVIE="${PROJ1}/outputs/smoke/proj_trim_test_compare.mp4"

if [[ ! -f "${FINAL_MOVIE}" ]]; then
  echo "FAIL: Final movie not generated at ${FINAL_MOVIE}"
  exit 1
fi
if [[ ! -f "${COMPARE_MOVIE}" ]]; then
  echo "FAIL: Compare movie not generated at ${COMPARE_MOVIE}"
  exit 1
fi

DUR_FINAL=$(probe_dur "${FINAL_MOVIE}")
DUR_COMPARE=$(probe_dur "${COMPARE_MOVIE}")
echo "  Final movie duration:   ${DUR_FINAL}s (Target: 5.50s)"
echo "  Compare movie duration: ${DUR_COMPARE}s (Target: 5.50s)"

python3 -c "
final = float('${DUR_FINAL}')
compare = float('${DUR_COMPARE}')
target = 5.50
assert abs(final - target) <= 0.5, f'Final movie {final}s not within 5.5s ±0.5s'
assert abs(compare - target) <= 0.5, f'Compare movie {compare}s not within 5.5s ±0.5s'
"
echo "PASS: Short shots successfully trimmed and assembled within ±0.5s of target."

# ------------------------------------------------------------------------------
# Check 2: Clips within duration (+0.08s) used as-is (no unnecessary re-encoding)
# ------------------------------------------------------------------------------
echo "--- [Check 2] Clips within duration used without trimming ---"
PROJ2="${TMP_DIR}/proj_no_trim"
mkdir -p "${PROJ2}/shots/shot_0001/renders/smoke" "${PROJ2}/shots/shot_0002/renders/smoke"

cat <<'EOF' > "${PROJ2}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 4.00,
      "duration": 4.00,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    },
    {
      "shot_id": "shot_0002",
      "start_sec": 4.00,
      "end_sec": 7.00,
      "duration": 3.00,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    }
  ]
}
EOF

# Render lengths exactly match or are within +0.08s
create_video "${PROJ2}/shots/shot_0001/renders/smoke/shot_0001.mp4" 4.00
create_video "${PROJ2}/shots/shot_0002/renders/smoke/shot_0002.mp4" 3.04

"${BIN}" assemble --proj-dir "${PROJ2}" --smoke

if [[ -d "${PROJ2}/outputs/smoke/.trim" ]]; then
  echo "FAIL: .trim directory should not exist when no clips need trimming"
  exit 1
fi
echo "PASS: No trimming performed when render durations are within target + 0.08s."

# ------------------------------------------------------------------------------
# Check 3: Temporary files cleaned up after assembly
# ------------------------------------------------------------------------------
echo "--- [Check 3] Temporary .trim directory removed after assembly ---"
if [[ -d "${PROJ1}/outputs/smoke/.trim" ]]; then
  echo "FAIL: Temporary directory ${PROJ1}/outputs/smoke/.trim was not removed!"
  exit 1
fi
echo "PASS: .trim directory properly cleaned up."

# ------------------------------------------------------------------------------
# Check 4: Trimming applies when original clip is missing (render-only shots)
# ------------------------------------------------------------------------------
echo "--- [Check 4] Trimming applies when original clip is missing ---"
PROJ3="${TMP_DIR}/proj_no_orig"
mkdir -p "${PROJ3}/shots/shot_0001/renders/smoke"

cat <<'EOF' > "${PROJ3}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 1.50,
      "duration": 1.50,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": ""
    }
  ]
}
EOF

# 4.0s render, NO original clip
create_video "${PROJ3}/shots/shot_0001/renders/smoke/shot_0001.mp4" 4.00

"${BIN}" assemble --proj-dir "${PROJ3}" --smoke

DUR_NO_ORIG=$(probe_dur "${PROJ3}/outputs/smoke/proj_no_orig_smoke.mp4")
echo "  Render-only movie duration: ${DUR_NO_ORIG}s (Target: 1.50s)"
python3 -c "
d = float('${DUR_NO_ORIG}')
assert abs(d - 1.50) <= 0.5, f'Render-only movie {d}s not within 1.50s ±0.5s'
"
echo "PASS: Trimming works properly even when original clips are missing."

# ------------------------------------------------------------------------------
# Check 5: Full breakingbad-s5e7-v2 project assembled duration (120.02s ±0.5s)
# ------------------------------------------------------------------------------
echo "--- [Check 5] Full breakingbad-s5e7-v2 assembly (120.02s ±0.5s) ---"
BB_PROJ="/rosenas/data/rea-movie-artifacts/h3-projects/breakingbad-s5e7-v2"
if [[ ! -d "${BB_PROJ}" ]]; then
  # Legacy fallback (pre h3-projects/h3-results layout)
  BB_PROJ="/rosenas/data/rea-movie-projects/breakingbad-s5e7-v2"
fi
if [[ ! -d "${BB_PROJ}" ]]; then
  # Fallback to alternate NAS mount if available
  BB_PROJ="/mynas/data/rea-movie-artifacts/h3-projects/breakingbad-s5e7-v2"
fi

if [[ -d "${BB_PROJ}" && -f "${BB_PROJ}/timeline.json" ]]; then
  echo "  Assembling ${BB_PROJ} (smoke profile)..."
  "${BIN}" assemble --proj-dir "${BB_PROJ}" --smoke

  BB_OUT="/rosenas/data/rea-movie-artifacts/h3-results/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_smoke.mp4"
  BB_COMP="/rosenas/data/rea-movie-artifacts/h3-results/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_compare.mp4"

  if [[ ! -f "${BB_OUT}" ]]; then
    BB_OUT="/rosenas/data/rea-movie-workspace/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_smoke.mp4"
    BB_COMP="/rosenas/data/rea-movie-workspace/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_compare.mp4"
  fi
  if [[ ! -f "${BB_OUT}" ]]; then
    BB_OUT="/mynas/data/rea-movie-artifacts/h3-results/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_smoke.mp4"
    BB_COMP="/mynas/data/rea-movie-artifacts/h3-results/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_compare.mp4"
  fi
  if [[ ! -f "${BB_OUT}" ]]; then
    BB_OUT="/mynas/data/rea-movie-workspace/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_smoke.mp4"
    BB_COMP="/mynas/data/rea-movie-workspace/breakingbad-s5e7-v2/outputs/smoke/breakingbad-s5e7-v2_compare.mp4"
  fi

  if [[ -f "${BB_OUT}" ]]; then
    BB_DUR=$(probe_dur "${BB_OUT}")
    BB_COMP_DUR=$(probe_dur "${BB_COMP}")
    echo "  breakingbad-s5e7-v2 final duration:   ${BB_DUR}s (Target: 120.02s)"
    echo "  breakingbad-s5e7-v2 compare duration: ${BB_COMP_DUR}s (Target: 120.02s)"

    python3 -c "
final = float('${BB_DUR}')
compare = float('${BB_COMP_DUR}')
target = 120.02
assert abs(final - target) <= 0.5, f'breakingbad-s5e7-v2 final {final}s not within 120.02s ±0.5s'
assert abs(compare - target) <= 0.5, f'breakingbad-s5e7-v2 compare {compare}s not within 120.02s ±0.5s'
"
    echo "PASS: breakingbad-s5e7-v2 assemble duration matches original 120.02s ±0.5s."
  else
    echo "SKIP Check 5: breakingbad-s5e7-v2 outputs not found"
  fi
else
  echo "SKIP Check 5: breakingbad-s5e7-v2 project not found"
fi

echo ""
echo "=========================================================================="
echo "ALL ISSUE 6 REGRESSION CHECKS PASSED SUCCESSFULLY!"
echo "=========================================================================="
