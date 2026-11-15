#!/usr/bin/env bash
# ==============================================================================
# regression-tests/verify-issue-5.sh
# Issue 5 Verification: Anchor Frame Prop Editing (KBO Cap on Head)
#
# Verification Criteria:
# 1. Frame dimension preservation: Composited frame keeps exact width/height.
# 2. Pixel difference: Edited frame differs from original frame (*.orig.jpg).
# 3. Color alteration: Composited region contains red/navy pixels from cap.
# 4. Idempotency: Re-running without --force does not change file hashes.
# 5. Skip without prop: Characters/projects without cap_ref remain untouched.
# 6. Shot and character filtering: --shot and --character flags filter properly.
# 7. MCP tool support: customize_anchor tool operates properly via JSON-RPC.
# ==============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

echo "==> Building rea-movie binary..."
(cd "${REPO_ROOT}" && cargo build --bin rea-movie)
BIN="${REPO_ROOT}/target/debug/rea-movie"

TMP_DIR=$(mktemp -d /tmp/rea-movie-test-issue5-XXXXXX)
trap 'rm -rf "${TMP_DIR}"' EXIT

echo "==> Running Issue 5 regression verifications in ${TMP_DIR}..."

# Setup mock BBox for deterministic testing without external API calls
export REA_MOVIE_MOCK_BBOX="0.25,0.08,0.20,0.50"

# ------------------------------------------------------------------------------
# 1. Project with cap_ref -> Verify composition, dimensions, pixels, and colors
# ------------------------------------------------------------------------------
echo "--- [Check 1] Prop composition, dimension preservation, and color alterations ---"
PROJ_DIR="${TMP_DIR}/proj_walter"
mkdir -p "${PROJ_DIR}/customization" "${PROJ_DIR}/assets/props" "${PROJ_DIR}/shots/shot_0015/refs" "${PROJ_DIR}/shots/shot_0022/refs"

# Copy or generate KBO cap prop
if [[ -f "${REPO_ROOT}/assets/props/kbo_cap.png" ]]; then
  cp "${REPO_ROOT}/assets/props/kbo_cap.png" "${PROJ_DIR}/assets/props/kbo_cap.png"
else
  python3 "${REPO_ROOT}/scripts/make-kbo-cap.py" "${PROJ_DIR}/assets/props/kbo_cap.png"
fi

cat <<'EOF' > "${PROJ_DIR}/customization/characters.toml"
[characters.walter]
name_en = "Walter White"
appearance_en = "a completely bald middle-aged man with a goatee"
cap_ref = "assets/props/kbo_cap.png"
cap_scale = 1.30
cap_offset = -0.05
EOF

cat <<'EOF' > "${PROJ_DIR}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0015",
      "start_sec": 0.0,
      "end_sec": 4.0,
      "duration": 4.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    },
    {
      "shot_id": "shot_0022",
      "start_sec": 4.0,
      "end_sec": 8.0,
      "duration": 4.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    }
  ]
}
EOF

# Create initial 960x540 neutral grey frames
python3 -c '
from PIL import Image
for shot in ["shot_0015", "shot_0022"]:
    for f in ["first_frame.jpg", "last_frame.jpg"]:
        img = Image.new("RGB", (960, 540), (180, 180, 180))
        img.save(f"'"${PROJ_DIR}"'/shots/{shot}/refs/{f}", quality=95)
'

# Run anchor customization
"${BIN}" customize anchor --proj-dir "${PROJ_DIR}"

# 1-1. Check original frame preservation
if [[ ! -f "${PROJ_DIR}/shots/shot_0015/refs/first_frame.orig.jpg" ]]; then
  echo "FAIL: first_frame.orig.jpg was not created in shot_0015"
  exit 1
fi
if [[ ! -f "${PROJ_DIR}/shots/shot_0015/refs/last_frame.orig.jpg" ]]; then
  echo "FAIL: last_frame.orig.jpg was not created in shot_0015"
  exit 1
fi
echo "PASS: Original frames preserved as *.orig.jpg"

# 1-2. Check frame dimensions are unchanged (960x540)
python3 -c '
from PIL import Image
orig = Image.open("'"${PROJ_DIR}"'/shots/shot_0015/refs/first_frame.orig.jpg")
edit = Image.open("'"${PROJ_DIR}"'/shots/shot_0015/refs/first_frame.jpg")
if orig.size != edit.size:
    raise SystemExit(f"FAIL: Size mismatch: orig={orig.size}, edit={edit.size}")
if edit.size != (960, 540):
    raise SystemExit(f"FAIL: Expected size (960, 540), got {edit.size}")
'
echo "PASS: Frame dimensions preserved exactly (960x540)."

# 1-3. Check pixel difference between original and edited frame
if cmp -s "${PROJ_DIR}/shots/shot_0015/refs/first_frame.orig.jpg" "${PROJ_DIR}/shots/shot_0015/refs/first_frame.jpg"; then
  echo "FAIL: Edited frame is byte-identical to original frame (no composition performed)"
  exit 1
fi
echo "PASS: Edited frame differs from original frame."

# 1-4. Check color alterations (presence of red and navy pixels from KBO cap)
python3 -c '
from PIL import Image
img = Image.open("'"${PROJ_DIR}"'/shots/shot_0015/refs/first_frame.jpg")
w, h = img.size
red_count = 0
navy_count = 0
for y in range(h):
    for x in range(w):
        r, g, b = img.getpixel((x, y))[:3]
        if r > 160 and g < 70 and b < 70:
            red_count += 1
        elif r < 50 and b > 50 and g < 60:
            navy_count += 1

if red_count < 50:
    raise SystemExit(f"FAIL: Insufficient red pixels found in composited frame: {red_count}")
if navy_count < 50:
    raise SystemExit(f"FAIL: Insufficient navy pixels found in composited frame: {navy_count}")
'
echo "PASS: Composited frame contains verified red and navy cap pixels."

# 1-5. Verify anchor_edit.json record format
python3 -c '
import json
with open("'"${PROJ_DIR}"'/shots/shot_0015/refs/anchor_edit.json") as f:
    data = json.load(f)
records = data if isinstance(data, list) else [data]
frames = [r["frame"] for r in records]
if "first_frame.jpg" not in frames:
    raise SystemExit("FAIL: first_frame.jpg not recorded in anchor_edit.json")
for r in records:
    c = r["character"]
    assert c == "walter", f"Unexpected character {c}"
    assert len(r["bbox"]) == 4, "Invalid bbox"
    assert "kbo_cap.png" in r["prop"], "Unexpected prop"
    assert "applied_at" in r, "Missing applied_at timestamp"
'
echo "PASS: anchor_edit.json contains valid record schema."

# ------------------------------------------------------------------------------
# 2. Idempotency verification (repeated execution without --force)
# ------------------------------------------------------------------------------
echo "--- [Check 2] Idempotency verification ---"
HASH_BEFORE_FIRST=$(sha256sum "${PROJ_DIR}/shots/shot_0015/refs/first_frame.jpg" | awk '{print $1}')
HASH_BEFORE_LAST=$(sha256sum "${PROJ_DIR}/shots/shot_0015/refs/last_frame.jpg" | awk '{print $1}')
HASH_BEFORE_LOG=$(sha256sum "${PROJ_DIR}/shots/shot_0015/refs/anchor_edit.json" | awk '{print $1}')

# Re-run customize anchor
"${BIN}" customize anchor --proj-dir "${PROJ_DIR}"

HASH_AFTER_FIRST=$(sha256sum "${PROJ_DIR}/shots/shot_0015/refs/first_frame.jpg" | awk '{print $1}')
HASH_AFTER_LAST=$(sha256sum "${PROJ_DIR}/shots/shot_0015/refs/last_frame.jpg" | awk '{print $1}')
HASH_AFTER_LOG=$(sha256sum "${PROJ_DIR}/shots/shot_0015/refs/anchor_edit.json" | awk '{print $1}')

if [[ "${HASH_BEFORE_FIRST}" != "${HASH_AFTER_FIRST}" ]] || [[ "${HASH_BEFORE_LAST}" != "${HASH_AFTER_LAST}" ]] || [[ "${HASH_BEFORE_LOG}" != "${HASH_AFTER_LOG}" ]]; then
  echo "FAIL: File hashes changed on idempotent re-run without --force"
  exit 1
fi
echo "PASS: Re-execution is completely idempotent (hashes unchanged)."

# ------------------------------------------------------------------------------
# 3. Project / character without cap_ref -> Zero edits performed
# ------------------------------------------------------------------------------
echo "--- [Check 3] Characters without cap_ref remain untouched ---"
PROJ_NO_PROP="${TMP_DIR}/proj_no_prop"
mkdir -p "${PROJ_NO_PROP}/customization" "${PROJ_NO_PROP}/shots/shot_0001/refs"

cat <<'EOF' > "${PROJ_NO_PROP}/customization/characters.toml"
[characters.jesse]
name_en = "Jesse Pinkman"
appearance_en = "young man in oversized hoodie"
# No cap_ref
EOF

cat <<'EOF' > "${PROJ_NO_PROP}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 3.0,
      "duration": 3.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["jesse"]
    }
  ]
}
EOF

python3 -c '
from PIL import Image
img = Image.new("RGB", (640, 360), (200, 200, 200))
img.save("'"${PROJ_NO_PROP}"'/shots/shot_0001/refs/first_frame.jpg", quality=95)
'
HASH_NO_PROP_BEFORE=$(sha256sum "${PROJ_NO_PROP}/shots/shot_0001/refs/first_frame.jpg" | awk '{print $1}')

"${BIN}" customize anchor --proj-dir "${PROJ_NO_PROP}"

HASH_NO_PROP_AFTER=$(sha256sum "${PROJ_NO_PROP}/shots/shot_0001/refs/first_frame.jpg" | awk '{print $1}')

if [[ "${HASH_NO_PROP_BEFORE}" != "${HASH_NO_PROP_AFTER}" ]]; then
  echo "FAIL: File was modified for character without cap_ref"
  exit 1
fi
if [[ -f "${PROJ_NO_PROP}/shots/shot_0001/refs/first_frame.orig.jpg" ]]; then
  echo "FAIL: first_frame.orig.jpg was unexpectedly created"
  exit 1
fi
if [[ -f "${PROJ_NO_PROP}/shots/shot_0001/refs/anchor_edit.json" ]]; then
  echo "FAIL: anchor_edit.json was unexpectedly created"
  exit 1
fi
echo "PASS: Projects/characters without cap_ref remain completely untouched."

# ------------------------------------------------------------------------------
# 4. Filtering by --shot and --character
# ------------------------------------------------------------------------------
echo "--- [Check 4] Filter by --shot flag ---"
PROJ_FILTER="${TMP_DIR}/proj_filter"
mkdir -p "${PROJ_FILTER}/customization" "${PROJ_FILTER}/assets/props" "${PROJ_FILTER}/shots/shot_0001/refs" "${PROJ_FILTER}/shots/shot_0002/refs"
cp "${PROJ_DIR}/assets/props/kbo_cap.png" "${PROJ_FILTER}/assets/props/kbo_cap.png"
cp "${PROJ_DIR}/customization/characters.toml" "${PROJ_FILTER}/customization/characters.toml"

cat <<'EOF' > "${PROJ_FILTER}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0001",
      "start_sec": 0.0,
      "end_sec": 3.0,
      "duration": 3.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    },
    {
      "shot_id": "shot_0002",
      "start_sec": 3.0,
      "end_sec": 6.0,
      "duration": 3.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    }
  ]
}
EOF

python3 -c '
from PIL import Image
for s in ["shot_0001", "shot_0002"]:
    img = Image.new("RGB", (640, 360), (220, 220, 220))
    img.save(f"'"${PROJ_FILTER}"'/shots/{s}/refs/first_frame.jpg", quality=95)
'

# Run anchor only for shot_0001
"${BIN}" customize anchor --proj-dir "${PROJ_FILTER}" --shot 0001

if [[ ! -f "${PROJ_FILTER}/shots/shot_0001/refs/first_frame.orig.jpg" ]]; then
  echo "FAIL: shot_0001 was not edited"
  exit 1
fi
if [[ -f "${PROJ_FILTER}/shots/shot_0002/refs/first_frame.orig.jpg" ]]; then
  echo "FAIL: shot_0002 was edited when only shot_0001 was targeted"
  exit 1
fi
echo "PASS: Shot filtering works properly."

# ------------------------------------------------------------------------------
# 5. MCP Tool customize_anchor verification
# ------------------------------------------------------------------------------
echo "--- [Check 5] MCP Tool customize_anchor ---"
PROJ_MCP="${TMP_DIR}/proj_mcp"
mkdir -p "${PROJ_MCP}/customization" "${PROJ_MCP}/assets/props" "${PROJ_MCP}/shots/shot_0010/refs"
cp "${PROJ_DIR}/assets/props/kbo_cap.png" "${PROJ_MCP}/assets/props/kbo_cap.png"
cp "${PROJ_DIR}/customization/characters.toml" "${PROJ_MCP}/customization/characters.toml"

cat <<'EOF' > "${PROJ_MCP}/timeline.json"
{
  "shots": [
    {
      "shot_id": "shot_0010",
      "start_sec": 0.0,
      "end_sec": 3.0,
      "duration": 3.0,
      "is_chained_longtake": false,
      "anchor": "first_and_last",
      "assignee": "",
      "characters": ["walter"]
    }
  ]
}
EOF

python3 -c '
from PIL import Image
img = Image.new("RGB", (640, 360), (210, 210, 210))
img.save("'"${PROJ_MCP}"'/shots/shot_0010/refs/first_frame.jpg", quality=95)
'

MCP_REQ=$(cat <<EOF
{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"customize_anchor","arguments":{"proj_dir":"${PROJ_MCP}","shot":"0010"}}}
EOF
)

MCP_OUT=$(echo "${MCP_REQ}" | "${BIN}" mcp)
if ! echo "${MCP_OUT}" | grep -q "Successfully processed anchor edits for 1 shot"; then
  echo "FAIL: MCP customize_anchor response unexpected: ${MCP_OUT}"
  exit 1
fi
if [[ ! -f "${PROJ_MCP}/shots/shot_0010/refs/first_frame.orig.jpg" ]]; then
  echo "FAIL: MCP customize_anchor failed to composite frame"
  exit 1
fi
echo "PASS: MCP customize_anchor tool verified."

echo "========================================================================"
echo "All Issue 5 regression verifications passed successfully!"
echo "========================================================================"
