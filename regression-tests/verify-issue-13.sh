#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-13.sh
# Issue 13 Verification: `source/` → `input/` and `sources/` → `input-details/`
# directory renaming.
#
# Criteria:
# 1. decompile stores everything under <root>/input-details/ (index.json,
#    <source_id>/{probe.json, scene_cuts.json, keyframes/, shots_cache/}) and
#    a second decompile reuses 100% of it; no <root>/sources/ is created.
# 2. Videos under <root>/input/ are recognised for series root detection
#    (series-watch + decompile) and caches land in <root>/input-details/;
#    legacy <root>/sources/ and <root>/source/ inputs are still recognised
#    via fallback while new caches still land in <root>/input-details/.
# 3. scripts/reverse-url.sh, skills/rea-movie/SKILL.md and README.md reflect
#    `input-details` / `input`, and verify-issue-8.sh + verify-issue-12.sh pass.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue13-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 13 regression verifications in ${TMP_DIR}..."

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
# Check 1: decompile writes <root>/input-details/ and reuses 100% on rerun
# ==============================================================================
echo "--- [Check 1] decompile input-details/ cache creation + 100% reuse ---"

ROOT1="${TMP_DIR}/series1"
mkdir -p "${ROOT1}/input" "${ROOT1}/input-details" "${ROOT1}/h3-projects"
VIDEO1="${ROOT1}/input/sample_episode [dQw4w9WgXcQ].mp4"
create_test_video "${VIDEO1}"
create_test_srt "${ROOT1}/input/sample_episode [dQw4w9WgXcQ].ko.srt"

PROJ_V1="${ROOT1}/h3-projects/sample-v1"
PROJ_V2="${ROOT1}/h3-projects/sample-v2"

echo "Executing first decompile (v1)..."
"${BIN}" decompile "${VIDEO1}" -o "${PROJ_V1}" --offline > "${TMP_DIR}/dc1.log" 2>&1

[[ -f "${ROOT1}/input-details/index.json" ]] || { echo "FAIL: index.json not in input-details/"; exit 1; }
grep -q '"source_id": "yt-dQw4w9WgXcQ"' "${ROOT1}/input-details/index.json" || {
  echo "FAIL: yt-dQw4w9WgXcQ not indexed"; exit 1;
}
SOURCE_DIR="${ROOT1}/input-details/yt-dQw4w9WgXcQ"
[[ -d "${SOURCE_DIR}" ]] || { echo "FAIL: cache dir ${SOURCE_DIR} missing"; exit 1; }
[[ -f "${SOURCE_DIR}/probe.json" ]] || { echo "FAIL: probe.json not cached"; exit 1; }
[[ -f "${SOURCE_DIR}/scene_cuts.json" ]] || { echo "FAIL: scene_cuts.json not cached"; exit 1; }
[[ -d "${SOURCE_DIR}/keyframes" ]] || { echo "FAIL: keyframes/ not created"; exit 1; }
[[ -d "${SOURCE_DIR}/shots_cache" ]] || { echo "FAIL: shots_cache/ not created"; exit 1; }
# New caches must not fall back to the old directory name.
[[ ! -e "${ROOT1}/sources" ]] || { echo "FAIL: legacy ${ROOT1}/sources/ was created"; exit 1; }
[[ -f "${PROJ_V1}/shots/shot_0001/refs/first_frame.jpg" ]] || { echo "FAIL: shot_0001 first_frame missing in v1"; exit 1; }
grep -q "Source cache assets:" "${TMP_DIR}/dc1.log" || { echo "FAIL: cache stats not logged in v1"; exit 1; }

echo "Executing second decompile (v2) of the same video..."
"${BIN}" decompile "${VIDEO1}" -o "${PROJ_V2}" --offline > "${TMP_DIR}/dc2.log" 2>&1

grep -q "reuse probe.json (yt-dQw4w9WgXcQ)" "${TMP_DIR}/dc2.log" || {
  echo "FAIL: probe.json was not reused in v2"; exit 1;
}
grep -q "reuse scene_cuts.json (yt-dQw4w9WgXcQ" "${TMP_DIR}/dc2.log" || {
  echo "FAIL: scene_cuts.json was not reused in v2"; exit 1;
}
grep -q "Source cache assets: .* reused, 0 extracted (yt-dQw4w9WgXcQ)" "${TMP_DIR}/dc2.log" || {
  echo "FAIL: shot assets were not 100% reused in v2"; exit 1;
}
[[ -f "${PROJ_V2}/shots/shot_0001/refs/contact_sheet.jpg" ]] || { echo "FAIL: contact_sheet missing in v2"; exit 1; }
cmp -s "${PROJ_V1}/shots/shot_0001/refs/first_frame.jpg" "${PROJ_V2}/shots/shot_0001/refs/first_frame.jpg" || {
  echo "FAIL: v1 and v2 anchor frames differ"; exit 1;
}

echo "PASS: Check 1 (input-details/ cache + 100% reuse) passed."

# ==============================================================================
# Check 2: <root>/input/ recognition + legacy <root>/sources|source/ fallback
# ==============================================================================
echo "--- [Check 2] input/ recognition and legacy fallback ---"

ROOT2="${TMP_DIR}/series2"
mkdir -p "${ROOT2}/input" "${ROOT2}/input-details" "${ROOT2}/h3-projects"
EP1_VID="${ROOT2}/input/EP1 [vid00000001].mp4"
EP2_VID="${ROOT2}/input/EP2 [vid00000002].mp4"
create_test_video "${EP1_VID}"
create_test_video "${EP2_VID}"
create_test_srt "${ROOT2}/input/EP1 [vid00000001].ko.srt"
create_test_srt "${ROOT2}/input/EP2 [vid00000002].ko.srt"

# 2-1. series-watch over <root>/input/ must resolve <root> and fill story/ + cache.
echo "Running series-watch for input/ layout..."
"${BIN}" series-watch --out-root "${ROOT2}" --source-dir "${ROOT2}/input" --offline > "${TMP_DIR}/sw1.log" 2>&1
for f in world.json characters.json timeline-days.md; do
  [[ -f "${ROOT2}/story/${f}" ]] || { echo "FAIL: story/${f} missing (input/ layout)"; exit 1; }
done
grep -q '"episode_count": 2' "${ROOT2}/story/world.json" || {
  echo "FAIL: world.json does not report 2 episodes"; exit 1;
}
[[ -d "${ROOT2}/input-details/yt-vid00000001" ]] || { echo "FAIL: input-details/ cache missing for EP1"; exit 1; }
[[ ! -e "${ROOT2}/sources" ]] || { echo "FAIL: legacy ${ROOT2}/sources/ was created by series-watch"; exit 1; }

# 2-2. decompile of an input/ video must reuse the series root (no warning).
"${BIN}" decompile "${EP1_VID}" -o "${ROOT2}/h3-projects/EP1" --offline > "${TMP_DIR}/dc_ep1.log" 2>&1
if grep -q "Warning: no series context" "${TMP_DIR}/dc_ep1.log"; then
  echo "FAIL: series context not picked up for input/ video"; exit 1;
fi

# 2-3. Legacy fallback: pre-existing sources/ + source/ inputs still resolve.
ROOT3="${TMP_DIR}/legacy"
mkdir -p "${ROOT3}/sources/yt-vid00000001" "${ROOT3}/source" "${ROOT3}/h3-projects"
create_test_video "${ROOT3}/sources/yt-vid00000001/legacy_ep [vid00000001].mp4"
create_test_srt "${ROOT3}/sources/yt-vid00000001/legacy_ep [vid00000001].ko.srt"
create_test_video "${ROOT3}/source/legacy_single [vid00000002].mp4"
create_test_srt "${ROOT3}/source/legacy_single [vid00000002].ko.srt"

echo "Running series-watch over legacy sources/ layout (fallback scan)..."
"${BIN}" series-watch --out-root "${ROOT3}" --offline \
  "${ROOT3}/sources/yt-vid00000001/legacy_ep [vid00000001].mp4" \
  "${ROOT3}/source/legacy_single [vid00000002].mp4" > "${TMP_DIR}/sw_legacy.log" 2>&1
[[ -f "${ROOT3}/story/world.json" ]] || { echo "FAIL: legacy series-watch did not write story/"; exit 1; }
grep -q '"episode_count": 2' "${ROOT3}/story/world.json" || {
  echo "FAIL: legacy world.json does not report 2 episodes"; exit 1;
}
echo "Running decompile of a legacy source/ video (new cache must land in input-details/)..."
"${BIN}" decompile "${ROOT3}/source/legacy_single [vid00000002].mp4" \
  -o "${ROOT3}/h3-projects/legacy-ep" --offline > "${TMP_DIR}/dc_legacy.log" 2>&1
[[ -f "${ROOT3}/input-details/index.json" ]] || {
  echo "FAIL: legacy decompile did not create input-details/index.json"; exit 1;
}

echo "PASS: Check 2 (input/ + legacy fallback) passed."

# ==============================================================================
# Check 3: script/docs references + issue 8/12 suites green
# ==============================================================================
echo "--- [Check 3] reverse-url.sh / docs references + issue 8/12 suites ---"

REV_SCRIPT="${REPO_ROOT}/scripts/reverse-url.sh"
SKILL="${REPO_ROOT}/skills/rea-movie/SKILL.md"
README="${REPO_ROOT}/README.md"

grep -q 'SRC_DIR="${ROOT}/input-details"' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh SRC_DIR is not <root>/input-details"; exit 1;
}
grep -q 'LEGACY_SRC_DIR="${ROOT}/input"' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh LEGACY_SRC_DIR is not <root>/input"; exit 1;
}
grep -q 'sources/yt-' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh lost legacy sources/yt- fallback"; exit 1;
}
grep -q 'input-details/yt-' "${REV_SCRIPT}" || {
  echo "FAIL: reverse-url.sh missing input-details/yt- cache dir"; exit 1;
}

grep -q 'input-details' "${SKILL}" || { echo "FAIL: SKILL.md missing input-details"; exit 1; }
grep -q '<root>/input-details/<source_id>' "${SKILL}" || {
  echo "FAIL: SKILL.md missing <root>/input-details/<source_id> layout"; exit 1;
}
grep -q 'input' "${README}" || { echo "FAIL: README.md missing input"; exit 1; }
grep -q '<root>/input-details/<source_id>' "${README}" || {
  echo "FAIL: README.md missing <root>/input-details/<source_id> layout"; exit 1;
}
grep -q 'legacy' "${README}" || { echo "FAIL: README.md missing legacy note"; exit 1; }

echo "Running regression-tests/verify-issue-8.sh..."
"${REPO_ROOT}/regression-tests/verify-issue-8.sh" > "${TMP_DIR}/v8.log" 2>&1 || {
  echo "FAIL: verify-issue-8.sh failed (see ${TMP_DIR}/v8.log)"; tail -20 "${TMP_DIR}/v8.log"; exit 1;
}
echo "Running regression-tests/verify-issue-12.sh..."
"${REPO_ROOT}/regression-tests/verify-issue-12.sh" > "${TMP_DIR}/v12.log" 2>&1 || {
  echo "FAIL: verify-issue-12.sh failed (see ${TMP_DIR}/v12.log)"; tail -20 "${TMP_DIR}/v12.log"; exit 1;
}

echo "PASS: Check 3 (script/docs + issue 8/12 suites) passed."

echo "=========================================================================="
echo "ALL ISSUE 13 REGRESSION CHECKS PASSED SUCCESSFULLY!"
echo "=========================================================================="
