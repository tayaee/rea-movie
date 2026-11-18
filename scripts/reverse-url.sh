#!/usr/bin/env bash
# reverse-url.sh — "<youtube url>을 역공학하여 <dir>에 저장" intake
#
# 사용법:
#   ./reverse-url.sh "<youtube-url-또는-playlist>" <output-root> [--range 0-120] [-- <decompile 추가 인자...>]
#
# 동작:
#   1. yt-dlp로 720p mp4 + 자막(ko,en)을 <output-root>/source/ 에 다운로드
#   2. 각 영상에 대해 rea-movie decompile (-o <output-root>/h3-projects/<stem>)
#   3. decompile이 프로젝트/미디어 분리를 자동 처리 (h3-projects + h3-results)
#
# 예:
#   ./reverse-url.sh "https://www.youtube.com/watch?v=XXXX" ~/rea-movie-artifacts --range 0-120
#   ./reverse-url.sh "https://www.youtube.com/playlist?list=PLxx" ~/rea-movie-artifacts -- --dub-lang ko
set -euo pipefail

if [[ $# -lt 2 ]]; then
  echo "Usage: $0 <youtube-url> <output-root> [--range START-END] [-- <decompile args...>]"
  exit 2
fi

URL="$1"
ROOT="$2"
shift 2

RANGE=""
EXTRA=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --range)
      RANGE="$2"
      shift 2
      ;;
    --)
      shift
      EXTRA=("$@")
      break
      ;;
    *)
      EXTRA+=("$1")
      shift
      ;;
  esac
done

if ! command -v yt-dlp >/dev/null 2>&1; then
  echo "Error: yt-dlp 가 설치되어 있지 않습니다." >&2
  exit 1
fi

BIN="${REA_MOVIE_BIN:-$(command -v rea-movie || true)}"
if [[ -z "${BIN}" ]]; then
  echo "Error: rea-movie 바이너리를 찾을 수 없습니다. REA_MOVIE_BIN 을 설정하세요." >&2
  exit 1
fi

SRC_DIR="${ROOT}/source"
PROJ_ROOT="${ROOT}/h3-projects"
mkdir -p "${SRC_DIR}" "${PROJ_ROOT}"

echo "==> Downloading: ${URL}"
PLAYLIST_FLAG="--no-playlist"
if [[ "${URL}" == *"list="* ]]; then
  PLAYLIST_FLAG="--yes-playlist"
fi
# shellcheck disable=SC2086
yt-dlp ${PLAYLIST_FLAG} \
  -f "bv*[height<=720][ext=mp4]+ba[ext=m4a]/b[height<=720][ext=mp4]/b[ext=mp4]/bv*+ba/b" \
  --merge-output-format mp4 \
  --write-subs --sub-langs "ko,en" --embed-subs \
  -o "${SRC_DIR}/%(playlist_index)02d_%(title)s [%(id)s].%(ext)s" \
  "${URL}"

echo "==> Decompiling into ${PROJ_ROOT}/"
for video in "${SRC_DIR}"/*.mp4; do
  [[ -e "${video}" ]] || continue
  stem="$(basename "${video}" .mp4)"
  # 자막 sidecar (.vtt -> .srt) : sibling srt 우선순위(ko > srt > en)에 맞춤
  for vtt in "${SRC_DIR}/${stem}".*.vtt; do
    [[ -e "${vtt}" ]] || continue
    lang="$(basename "${vtt}" | sed -n "s/.*\.\([a-z-]*\)\.vtt/\1/p")"
    [[ -n "${lang}" ]] || lang="en"
    ffmpeg -nostdin -v error -y -i "${vtt}" "${SRC_DIR}/${stem}.${lang}.srt" || true
  done
  dest="${PROJ_ROOT}/${stem}"
  if [[ -d "${dest}" ]]; then
    echo "---- Skip (exists): ${dest} (삭제 후 재실행하거나 -o 로 다른 이름 지정)"
    continue
  fi
  # shellcheck disable=SC2086
  "${BIN}" "${video}" -o "${dest}" ${RANGE:+--range "${RANGE}"} "${EXTRA[@]+${EXTRA[@]}}" || true
done

echo "==> Done. Projects: ${PROJ_ROOT}/ | Results: ${ROOT}/h3-results/"
