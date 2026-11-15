#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-4.sh
# Issue 4 Verification: Customization layer & Character traits
#
# Criteria:
# 1. Project without customization/ -> Unmodified output (no change / regression free)
# 2. Project with customization/ -> Traits injected ONLY to shots where target character is cast
# 3. Idempotent re-application -> No duplicated blocks on repeated apply
# 4. prompt.h3.txt contains NO Korean characters
# 5. customization/ files are never overwritten (read-only input preserved)
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# Ensure binary is built
echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue4-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 4 regression verifications in ${TMP_DIR}..."

# ------------------------------------------------------------------------------
# 1. Project without customization/ -> Unchanged output
# ------------------------------------------------------------------------------
echo "--- [Check 1] Project without customization/ remains unmodified ---"
PROJ_NO_CUSTOM="${TMP_DIR}/proj_no_custom"
mkdir -p "${PROJ_NO_CUSTOM}/shots/shot_0001"

cat <<'EOF' > "${PROJ_NO_CUSTOM}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 4.0,
      "duration": 4.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["S1"]
    }
  ]
}
EOF

cat <<'EOF' > "${PROJ_NO_CUSTOM}/shots/shot_0001/shot.toml"
shot_id = "shot_0001"
start_time = 0.0
end_time = 4.0
duration = 4.0
anchor = "first_and_last"
seed = 42
characters = ["S1"]
audio_peaks = []
EOF

KO_ORIGINAL="# 샷 연출 지시서: shot_0001
## 1. 화면
- 원본 연출 유지
## 5. 배경음악
- 앰비언스"

H3_ORIGINAL="For the target video, at 0.00 seconds into the target video, <Picture 1> and <Picture 2> are fully referenced.

integrated_multimodal_description:
From 0.0s to 4.0s, camera pans right smoothly.

overall_soundscape:
Ambient room tone.

non_diegetic_music:
None."

echo "${KO_ORIGINAL}" > "${PROJ_NO_CUSTOM}/shots/shot_0001/prompt.ko.md"
echo "${H3_ORIGINAL}" > "${PROJ_NO_CUSTOM}/shots/shot_0001/prompt.h3.txt"

"${BIN}" customize apply --proj-dir "${PROJ_NO_CUSTOM}"

# Verify no blocks injected
if grep -q "<!-- rea-movie:block:begin" "${PROJ_NO_CUSTOM}/shots/shot_0001/prompt.h3.txt"; then
  echo "FAIL: Unexpected block found in prompt.h3.txt for uncustomized project"
  exit 1
fi
if grep -q "## 6. 자동 주입 커스텀 지시" "${PROJ_NO_CUSTOM}/shots/shot_0001/prompt.ko.md"; then
  echo "FAIL: Unexpected section 6 found in prompt.ko.md for uncustomized project"
  exit 1
fi
echo "PASS: Uncustomized project has zero injected blocks."

# ------------------------------------------------------------------------------
# 2. Project with customization/ -> Trait injected ONLY to casted shots
# ------------------------------------------------------------------------------
echo "--- [Check 2] Character-targeted traits injected only to relevant shots ---"
PROJ_CUSTOM="${TMP_DIR}/proj_custom"
mkdir -p "${PROJ_CUSTOM}/customization/shots"
mkdir -p "${PROJ_CUSTOM}/shots/shot_0001"
mkdir -p "${PROJ_CUSTOM}/shots/shot_0002"

cat <<'EOF' > "${PROJ_CUSTOM}/customization/characters.toml"
[characters.walter]
name_ko = "월터 화이트"
name_en = "Walter White"
appearance_en = "middle-aged man, bald head, goatee"
appearance_ko = "중년 남성, 대머리, 콧수염"
wardrobe_en = "black bomber jacket over a blue hoodie"
wardrobe_ko = "검정 볼트넥 재킷에 파란 후드"

[characters.mike]
name_ko = "마이크"
name_en = "Mike"
appearance_en = "elderly man, clean-shaven"
appearance_ko = "노년 남성, 말끔한 면도"
wardrobe_en = "dark overcoat"
wardrobe_ko = "어두운 외투"
EOF

cat <<'EOF' > "${PROJ_CUSTOM}/customization/traits.toml"
[[traits]]
id = "kbo_cap"
scope = "global"
target = "character:walter"
shots = []
statement_en = "Walter wears a navy-and-red KBO baseball cap in every shot."
statement_ko = "월터는 모든 샷에서 KBO 야구모자를 착용한다."
EOF

# Shot 0001 has Walter
cat <<'EOF' > "${PROJ_CUSTOM}/shots/shot_0001/shot.toml"
shot_id = "shot_0001"
start_time = 0.0
end_time = 4.0
duration = 4.0
anchor = "first_and_last"
seed = 42
characters = ["walter"]
audio_peaks = []
EOF

echo "${KO_ORIGINAL}" > "${PROJ_CUSTOM}/shots/shot_0001/prompt.ko.md"
echo "${H3_ORIGINAL}" > "${PROJ_CUSTOM}/shots/shot_0001/prompt.h3.txt"

# Shot 0002 has Mike only (via customization/shots/shot_0002.toml cast override)
cat <<'EOF' > "${PROJ_CUSTOM}/customization/shots/shot_0002.toml"
cast = ["mike"]
EOF

cat <<'EOF' > "${PROJ_CUSTOM}/shots/shot_0002/shot.toml"
shot_id = "shot_0002"
start_time = 4.0
end_time = 8.0
duration = 4.0
anchor = "first_and_last"
seed = 43
characters = ["S1"]
audio_peaks = []
EOF

echo "${KO_ORIGINAL}" > "${PROJ_CUSTOM}/shots/shot_0002/prompt.ko.md"
echo "${H3_ORIGINAL}" > "${PROJ_CUSTOM}/shots/shot_0002/prompt.h3.txt"

cat <<'EOF' > "${PROJ_CUSTOM}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 4.0,
      "duration": 4.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    },
    {
      "shot_id": "shot_0002",
      "start_sec": 4.0,
      "end_sec": 8.0,
      "duration": 4.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["mike"]
    }
  ]
}
EOF

# Capture customization files checksums
CHARS_MD5=$(md5sum "${PROJ_CUSTOM}/customization/characters.toml" | awk '{print $1}')
TRAITS_MD5=$(md5sum "${PROJ_CUSTOM}/customization/traits.toml" | awk '{print $1}')

"${BIN}" customize apply --proj-dir "${PROJ_CUSTOM}"

# Verify Walter shot (0001) has kbo_cap
if ! grep -q "id=kbo_cap" "${PROJ_CUSTOM}/shots/shot_0001/prompt.h3.txt"; then
  echo "FAIL: Expected kbo_cap trait block in shot_0001 prompt.h3.txt"
  exit 1
fi
if ! grep -q "id=kbo_cap" "${PROJ_CUSTOM}/shots/shot_0001/prompt.ko.md"; then
  echo "FAIL: Expected kbo_cap trait block in shot_0001 prompt.ko.md"
  exit 1
fi

# Verify Mike shot (0002) DOES NOT have kbo_cap
if grep -q "id=kbo_cap" "${PROJ_CUSTOM}/shots/shot_0002/prompt.h3.txt"; then
  echo "FAIL: kbo_cap trait should NOT be injected into shot_0002 (Mike only)"
  exit 1
fi
if grep -q "id=kbo_cap" "${PROJ_CUSTOM}/shots/shot_0002/prompt.ko.md"; then
  echo "FAIL: kbo_cap trait should NOT be injected into shot_0002 prompt.ko.md"
  exit 1
fi
echo "PASS: Walter trait was selectively injected only into shot_0001."

# ------------------------------------------------------------------------------
# 3. Idempotent re-application
# ------------------------------------------------------------------------------
echo "--- [Check 3] Idempotent re-application ---"
"${BIN}" customize apply --proj-dir "${PROJ_CUSTOM}"
"${BIN}" customize apply --proj-dir "${PROJ_CUSTOM}"

COUNT_H3=$(grep -c "<!-- rea-movie:block:begin id=kbo_cap -->" "${PROJ_CUSTOM}/shots/shot_0001/prompt.h3.txt")
COUNT_KO=$(grep -c "<!-- rea-movie:block:begin id=kbo_cap -->" "${PROJ_CUSTOM}/shots/shot_0001/prompt.ko.md")
COUNT_SEC=$(grep -c "## 6. 자동 주입 커스텀 지시" "${PROJ_CUSTOM}/shots/shot_0001/prompt.ko.md")

if [[ "${COUNT_H3}" -ne 1 ]]; then
  echo "FAIL: Expected exactly 1 block in prompt.h3.txt after multiple applies, got ${COUNT_H3}"
  exit 1
fi
if [[ "${COUNT_KO}" -ne 1 ]]; then
  echo "FAIL: Expected exactly 1 block in prompt.ko.md after multiple applies, got ${COUNT_KO}"
  exit 1
fi
if [[ "${COUNT_SEC}" -ne 1 ]]; then
  echo "FAIL: Expected exactly 1 section 6 in prompt.ko.md after multiple applies, got ${COUNT_SEC}"
  exit 1
fi
echo "PASS: Multiple applies produce identical output with zero duplicated blocks."

# ------------------------------------------------------------------------------
# 4. prompt.h3.txt contains NO Korean characters
# ------------------------------------------------------------------------------
echo "--- [Check 4] prompt.h3.txt contains strictly English ---"
for h3_file in "${PROJ_CUSTOM}"/shots/*/prompt.h3.txt; do
  # Python one-liner to check for Hangul unicode range
  if python3 -c "
import sys
content = open(sys.argv[1], encoding='utf-8').read()
hangul = [c for c in content if '\uac00' <= c <= '\ud7a3']
if hangul:
    print(f'Found Korean in {sys.argv[1]}: {\"\".join(hangul[:10])}')
    sys.exit(1)
" "${h3_file}"; then
    :
  else
    echo "FAIL: Hangul detected in ${h3_file}"
    exit 1
  fi
done
echo "PASS: All prompt.h3.txt files contain strictly English."

# ------------------------------------------------------------------------------
# 5. customization/ files are never overwritten
# ------------------------------------------------------------------------------
echo "--- [Check 5] customization/ input files remain untouched ---"
CHARS_AFTER_MD5=$(md5sum "${PROJ_CUSTOM}/customization/characters.toml" | awk '{print $1}')
TRAITS_AFTER_MD5=$(md5sum "${PROJ_CUSTOM}/customization/traits.toml" | awk '{print $1}')

if [[ "${CHARS_MD5}" != "${CHARS_AFTER_MD5}" ]]; then
  echo "FAIL: customization/characters.toml was modified!"
  exit 1
fi
if [[ "${TRAITS_MD5}" != "${TRAITS_AFTER_MD5}" ]]; then
  echo "FAIL: customization/traits.toml was modified!"
  exit 1
fi
echo "PASS: customization/ files were preserved untouched."

echo "==> ALL ISSUE 4 CHECKS PASSED SUCCESSFULLY!"
exit 0
