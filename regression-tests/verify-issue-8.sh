#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-8.sh
# Issue 8 Verification: input-details/ Cache Reuse Architecture & series-watch Pipeline
#
# Verification Criteria:
# 1. Cache creation & reuse:
#    - First decompile populates <root>/input-details/{index.json, probe.json,
#      scene_cuts.json, keyframes/, shots_cache/}.
#    - Second decompile of the same video in another project (-v2) achieves
#      100% cache reuse (skips ffprobe, skips scene cut detection, reuses shot assets).
# 2. series-watch & series context:
#    - `rea-movie series-watch` generates <root>/story/{world.json, characters.json,
#      timeline-days.md} with extracted speaker tags, palettes, and continuity draft.
#    - Subsequent decompile does NOT emit "Warning: no series context".
#    - Idempotent re-run skips existing story files unless --force.
#    - MCP tool `series_watch` successfully executes.
# 3. scripts/reverse-url.sh cache reuse:
#    - Uses <output-root>/input-details/ + <output-root>/input/ directory layout.
#    - Skips yt-dlp download and .vtt->.srt conversion on cached video IDs.
#    - Runs series-watch for playlists and respects `--episodes` selection.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue8-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 8 regression verifications in ${TMP_DIR}..."

# Helper to generate dummy MP4 video with a sharp scene change at 2.0s
create_test_video() {
  local out_path="$1"
  mkdir -p "$(dirname "${out_path}")"
  ffmpeg -nostdin -v error -y \
    -f lavfi -i "color=c=blue:s=320x240:d=2" \
    -f lavfi -i "sine=frequency=440:duration=2" \
    -f lavfi -i "color=c=red:s=320x240:d=2" \
    -f lavfi -i "sine=frequency=880:duration=2" \
    -filter_complex "[0:v][1:a][2:v][3:a]concat=n=2:v=1:a=1[v][a]" \
    -map "[v]" -map "[a]" \
    -c:v libx264 -preset ultrafast -pix_fmt yuv420p \
    -c:a aac -ar 32000 -ac 2 \
    "${out_path}"
}

# Helper to generate dummy SRT subtitle
create_test_srt() {
  local out_path="$1"
  mkdir -p "$(dirname "${out_path}")"
  cat <<'EOF' > "${out_path}"
1
00:00:00,400 --> 00:00:01,800
WALTER: We need to cook.

2
00:00:02,100 --> 00:00:03,600
[JESSE] Yeah, science!
EOF
}

# ==============================================================================
# Check 1: input-details/ Cache Creation & 100% Reuse on Second Decompile (-v1, -v2)
# ==============================================================================
echo "--- [Check 1] input-details/ cache creation and reuse across projects ---"

ROOT1="${TMP_DIR}/series1"
mkdir -p "${ROOT1}/input" "${ROOT1}/input-details" "${ROOT1}/h3-projects"
VIDEO1="${ROOT1}/input/sample_episode [dQw4w9WgXcQ].mp4"
create_test_video "${VIDEO1}"
create_test_srt "${ROOT1}/input/sample_episode [dQw4w9WgXcQ].ko.srt"

PROJ_V1="${ROOT1}/h3-projects/sample-v1"
PROJ_V2="${ROOT1}/h3-projects/sample-v2"

# 1-1. First decompile: should populate input-details/ cache
echo "Executing first decompile (v1)..."
"${BIN}" decompile "${VIDEO1}" -o "${PROJ_V1}" --offline > "${TMP_DIR}/dc1.log" 2>&1

# Verify cache layout
[[ -f "${ROOT1}/input-details/index.json" ]] || { echo "FAIL: index.json not found in input-details/"; exit 1; }
grep -q '"source_id": "yt-dQw4w9WgXcQ"' "${ROOT1}/input-details/index.json" || {
  echo "FAIL: yt-dQw4w9WgXcQ not indexed in index.json"; exit 1;
}

SOURCE_DIR="${ROOT1}/input-details/yt-dQw4w9WgXcQ"
[[ -d "${SOURCE_DIR}" ]] || { echo "FAIL: Source cache dir ${SOURCE_DIR} not found"; exit 1; }
[[ -f "${SOURCE_DIR}/probe.json" ]] || { echo "FAIL: probe.json not cached"; exit 1; }
[[ -f "${SOURCE_DIR}/scene_cuts.json" ]] || { echo "FAIL: scene_cuts.json not cached"; exit 1; }
[[ -d "${SOURCE_DIR}/keyframes" ]] || { echo "FAIL: keyframes/ dir not created"; exit 1; }
[[ -d "${SOURCE_DIR}/shots_cache" ]] || { echo "FAIL: shots_cache/ dir not created"; exit 1; }

# Verify v1 extracted shot assets
[[ -f "${PROJ_V1}/shots/shot_0001/refs/first_frame.jpg" ]] || { echo "FAIL: shot_0001 first_frame missing in v1"; exit 1; }
grep -q "Source cache assets:" "${TMP_DIR}/dc1.log" || { echo "FAIL: Cache stats not logged in v1"; exit 1; }

# 1-2. Second decompile: should 100% reuse cached assets into project v2
echo "Executing second decompile (v2) of the same video..."
"${BIN}" decompile "${VIDEO1}" -o "${PROJ_V2}" --offline > "${TMP_DIR}/dc2.log" 2>&1

# Verify cache reuse log messages
grep -q "reuse probe.json (yt-dQw4w9WgXcQ)" "${TMP_DIR}/dc2.log" || {
  echo "FAIL: probe.json was not reused in v2 decompile"; exit 1;
}
grep -q "reuse scene_cuts.json (yt-dQw4w9WgXcQ" "${TMP_DIR}/dc2.log" || {
  echo "FAIL: scene_cuts.json was not reused in v2 decompile"; exit 1;
}
grep -q "Source cache assets: .* reused, 0 extracted (yt-dQw4w9WgXcQ)" "${TMP_DIR}/dc2.log" || {
  echo "FAIL: shot assets were not 100% reused in v2 decompile"; exit 1;
}

# Verify v2 has identical extracted references
[[ -f "${PROJ_V2}/shots/shot_0001/refs/first_frame.jpg" ]] || { echo "FAIL: shot_0001 first_frame missing in v2"; exit 1; }
[[ -f "${PROJ_V2}/shots/shot_0001/refs/contact_sheet.jpg" ]] || { echo "FAIL: shot_0001 contact_sheet missing in v2"; exit 1; }
cmp -s "${PROJ_V1}/shots/shot_0001/refs/first_frame.jpg" "${PROJ_V2}/shots/shot_0001/refs/first_frame.jpg" || {
  echo "FAIL: v1 and v2 anchor frames differ"; exit 1;
}

echo "PASS: Check 1 (input-details/ cache creation and 100% reuse) passed."

# ==============================================================================
# Check 2: rea-movie series-watch & Series Context Warning Elimination
# ==============================================================================
echo "--- [Check 2] series-watch pre-analysis & series context verification ---"

ROOT2="${TMP_DIR}/series2"
mkdir -p "${ROOT2}/input" "${ROOT2}/input-details" "${ROOT2}/h3-projects"
EP1_VID="${ROOT2}/input/EP1 [vid00000001].mp4"
EP2_VID="${ROOT2}/input/EP2 [vid00000002].mp4"
create_test_video "${EP1_VID}"
create_test_video "${EP2_VID}"
create_test_srt "${ROOT2}/input/EP1 [vid00000001].ko.srt"
create_test_srt "${ROOT2}/input/EP2 [vid00000002].ko.srt"

# 2-1. Run series-watch in offline mode
echo "Running series-watch for series2..."
"${BIN}" series-watch --out-root "${ROOT2}" --source-dir "${ROOT2}/input" --offline > "${TMP_DIR}/sw1.log" 2>&1

STORY_DIR="${ROOT2}/story"
[[ -f "${STORY_DIR}/world.json" ]] || { echo "FAIL: story/world.json not generated"; exit 1; }
[[ -f "${STORY_DIR}/characters.json" ]] || { echo "FAIL: story/characters.json not generated"; exit 1; }
[[ -f "${STORY_DIR}/timeline-days.md" ]] || { echo "FAIL: story/timeline-days.md not generated"; exit 1; }

# Verify world.json contents
grep -q '"episode_count": 2' "${STORY_DIR}/world.json" || {
  echo "FAIL: world.json does not report 2 episodes"; exit 1;
}
grep -q '"source_id": "yt-vid00000001"' "${STORY_DIR}/world.json" || {
  echo "FAIL: world.json missing episode 1 source_id"; exit 1;
}

# Verify characters.json extracted speaker tags (WALTER, JESSE)
grep -q '"name": "WALTER"' "${STORY_DIR}/characters.json" || {
  echo "FAIL: characters.json did not extract speaker WALTER"; exit 1;
}
grep -q '"name": "JESSE"' "${STORY_DIR}/characters.json" || {
  echo "FAIL: characters.json did not extract speaker JESSE"; exit 1;
}

# Verify timeline-days.md contains table rows
grep -q "| 01 |" "${STORY_DIR}/timeline-days.md" || {
  echo "FAIL: timeline-days.md missing row for EP 01"; exit 1;
}
grep -q "| 02 |" "${STORY_DIR}/timeline-days.md" || {
  echo "FAIL: timeline-days.md missing row for EP 02"; exit 1;
}

# 2-2. Idempotent skip without --force
echo "Re-running series-watch without --force..."
"${BIN}" series-watch --out-root "${ROOT2}" --source-dir "${ROOT2}/input" --offline > "${TMP_DIR}/sw2.log" 2>&1
grep -q "series context already present .* skipping" "${TMP_DIR}/sw2.log" || {
  echo "FAIL: series-watch did not skip existing story directory"; exit 1;
}

# 2-3. Verify decompile does NOT emit "Warning: no series context"
echo "Running decompile on EP1 inside series2..."
"${BIN}" decompile "${EP1_VID}" -o "${ROOT2}/h3-projects/EP1" --offline > "${TMP_DIR}/dc_ep1.log" 2>&1
if grep -q "Warning: no series context" "${TMP_DIR}/dc_ep1.log"; then
  echo "FAIL: 'Warning: no series context' was emitted when story/world.json is present"; exit 1;
fi

# 2-4. Cross-check: decompile in a standalone root MUST emit the warning
echo "Running cross-check decompile without series context..."
STANDALONE_DIR="${TMP_DIR}/standalone_root"
mkdir -p "${STANDALONE_DIR}/h3-projects"
"${BIN}" decompile "${EP1_VID}" -o "${STANDALONE_DIR}/h3-projects/ep_standalone" --offline > "${TMP_DIR}/dc_standalone.log" 2>&1
grep -q "Warning: no series context" "${TMP_DIR}/dc_standalone.log" || {
  echo "FAIL: Expected 'Warning: no series context' when story/world.json is missing"; exit 1;
}

# 2-5. Verify MCP series_watch tool via JSON-RPC stdio
echo "Verifying MCP series_watch tool..."
MCP_REQ=$(cat <<EOF
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"series_watch","arguments":{"out_root":"${ROOT2}","offline":true}}}
EOF
)
MCP_RESP=$(echo "${MCP_REQ}" | "${BIN}" mcp)
echo "${MCP_RESP}" | grep -q 'story_dir' || {
  echo "FAIL: MCP series_watch tool response missing story_dir: ${MCP_RESP}"; exit 1;
}

echo "PASS: Check 2 (series-watch & series context) passed."

# ==============================================================================
# Check 3: scripts/reverse-url.sh input-details/ Path & Cache Reuse Logic
# ==============================================================================
echo "--- [Check 3] scripts/reverse-url.sh input-details/ integration & skip logic ---"

REV_SCRIPT="${REPO_ROOT}/scripts/reverse-url.sh"
[[ -x "${REV_SCRIPT}" ]] || { echo "FAIL: scripts/reverse-url.sh is not executable"; exit 1; }

# 3-1. Static check for required keywords
grep -q 'SRC_DIR="${ROOT}/input-details"' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh does not use input-details/ directory"; exit 1;
}
grep -q 'cached_video' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh missing cached_video function"; exit 1;
}
grep -q 'series-watch' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh missing series-watch execution"; exit 1;
}
grep -q -- '--episodes' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh missing --episodes parameter support"; exit 1;
}

# 3-2. Dynamic execution test with a mock yt-dlp
MOCK_DIR="${TMP_DIR}/mock_bin"
mkdir -p "${MOCK_DIR}"
MOCK_CALL_LOG="${TMP_DIR}/mock_ytdlp_calls.txt"

cat <<'EOF' > "${MOCK_DIR}/yt-dlp"
#!/usr/bin/env bash
echo "$*" >> "${MOCK_LOG}"

# 1. Playlist flat listing
if [[ "$*" == *"--flat-playlist"* ]]; then
  echo -e "1\tvid00000001"
  echo -e "2\tvid00000002"
  exit 0
fi

# 2. Print video id
if [[ "$*" == *"--print %(id)s"* ]]; then
  echo "vid00000001"
  exit 0
fi

# 3. Download simulation
out_pattern=""
while [[ $# -gt 0 ]]; do
  if [[ "$1" == "-o" ]]; then
    out_pattern="$2"
    shift 2
  else
    shift
  fi
done

if [[ -n "${out_pattern}" ]]; then
  # Replace format template with concrete values
  target="${out_pattern//%\(title\)s/MockEpisode}"
  target="${target//%\(id\)s/vid00000001}"
  target="${target//%\(ext\)s/mp4}"
  mkdir -p "$(dirname "${target}")"
  ffmpeg -nostdin -v error -y -f lavfi -i "color=c=blue:s=320x240:d=1" \
    -f lavfi -i "sine=frequency=440:duration=1" \
    -c:v libx264 -preset ultrafast -c:a aac "${target}"
  # Generate dummy vtt
  vtt="${target%.mp4}.ko.vtt"
  cat <<'VTT' > "${vtt}"
WEBVTT

00:00:00.000 --> 00:00:01.000
WALTER: Mock subtitle
VTT
fi
exit 0
EOF
chmod +x "${MOCK_DIR}/yt-dlp"

REV_ROOT="${TMP_DIR}/rev_root"
mkdir -p "${REV_ROOT}"

# Run 1: initial download and decompile
echo "Executing reverse-url.sh (initial intake)..."
PATH="${MOCK_DIR}:${PATH}" MOCK_LOG="${MOCK_CALL_LOG}" REA_MOVIE_BIN="${BIN}" \
  "${REV_SCRIPT}" "https://www.youtube.com/watch?v=vid00000001" "${REV_ROOT}" --offline > "${TMP_DIR}/rev1.log" 2>&1

[[ -d "${REV_ROOT}/input-details/yt-vid00000001" ]] || {
  echo "FAIL: input-details/yt-vid00000001/ not created by reverse-url.sh"; exit 1;
}
# Verify .vtt was converted to .srt
shopt -s nullglob
SRTS=("${REV_ROOT}/input-details/yt-vid00000001/"*.srt)
shopt -u nullglob
[[ ${#SRTS[@]} -gt 0 ]] || {
  echo "FAIL: .vtt was not converted to .srt in sources cache"; exit 1;
}

# Run 2: second intake of identical video should skip yt-dlp download
echo "Executing reverse-url.sh (second intake - should reuse cache)..."
PREV_CALLS=$(wc -l < "${MOCK_CALL_LOG}" 2>/dev/null || echo 0)
PATH="${MOCK_DIR}:${PATH}" MOCK_LOG="${MOCK_CALL_LOG}" REA_MOVIE_BIN="${BIN}" \
  "${REV_SCRIPT}" "https://www.youtube.com/watch?v=vid00000001" "${REV_ROOT}" --offline > "${TMP_DIR}/rev2.log" 2>&1

grep -q "Reuse cached source (skip yt-dlp download)" "${TMP_DIR}/rev2.log" || {
  echo "FAIL: reverse-url.sh did not report skipping yt-dlp download"; exit 1;
}

# Run 3: playlist with --episodes 1 (should run series-watch then decompile only EP1)
echo "Executing reverse-url.sh playlist with --episodes 1..."
PL_ROOT="${TMP_DIR}/pl_root"
mkdir -p "${PL_ROOT}/input-details/yt-vid00000001" "${PL_ROOT}/input-details/yt-vid00000002"
create_test_video "${PL_ROOT}/input-details/yt-vid00000001/01_Ep1 [vid00000001].mp4"
create_test_video "${PL_ROOT}/input-details/yt-vid00000002/02_Ep2 [vid00000002].mp4"
create_test_srt "${PL_ROOT}/input-details/yt-vid00000001/01_Ep1 [vid00000001].ko.srt"
create_test_srt "${PL_ROOT}/input-details/yt-vid00000002/02_Ep2 [vid00000002].ko.srt"

PATH="${MOCK_DIR}:${PATH}" MOCK_LOG="${MOCK_CALL_LOG}" REA_MOVIE_BIN="${BIN}" \
  "${REV_SCRIPT}" "https://www.youtube.com/playlist?list=PLtestlist123" "${PL_ROOT}" \
  --episodes 1 --offline > "${TMP_DIR}/rev_pl.log" 2>&1

grep -q "Series watch .* -> ${PL_ROOT}/story/" "${TMP_DIR}/rev_pl.log" || {
  echo "FAIL: playlist did not trigger series-watch"; exit 1;
}
grep -q "Skip EP02 (not in --episodes 1)" "${TMP_DIR}/rev_pl.log" || {
  echo "FAIL: EP02 was not skipped when --episodes 1 was passed"; exit 1;
}
[[ -d "${PL_ROOT}/h3-projects/01_Ep1 [vid00000001]" ]] || {
  echo "FAIL: EP01 was not decompiled"; exit 1;
}
[[ ! -d "${PL_ROOT}/h3-projects/02_Ep2 [vid00000002]" ]] || {
  echo "FAIL: EP02 was decompiled despite not being in --episodes 1"; exit 1;
}

echo "PASS: Check 3 (scripts/reverse-url.sh cache reuse & options) passed."

echo "=========================================================================="
echo "ALL ISSUE 8 REGRESSION CHECKS PASSED SUCCESSFULLY!"
echo "=========================================================================="
