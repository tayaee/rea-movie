#!/usr/bin/env bash
# reverse-url.sh — "<youtube url>을 역공학하여 <dir>에 저장" intake (issue 8: input-details/ 캐시 재사용)
#
# 사용법:
#   ./reverse-url.sh "<youtube-url-또는-playlist>" <output-root> \
#       [--range 0-120] [--episodes 1,2|1-3] [--offline] [--force] [-- <decompile 추가 인자...>]
#
# 동작:
#   1. 영상 ID별 캐시 폴더 <output-root>/input-details/yt-<id>/ 에 720p mp4 + 자막(ko,en)을 받는다.
#      - 같은 영상 ID가 이미 input-details/ 에 있으면 yt-dlp 다운로드를 건너뛰고 재사용한다.
#      - 구 레이아웃 <output-root>/input/*[<id>].mp4, <output-root>/source/*[<id>].mp4,
#        <output-root>/sources/yt-<id>/ 가 있으면 input-details/yt-<id>/ 로 링크해 재사용한다 (레거시 폴백).
#      - .vtt -> .srt 변환도 결과 .srt 가 이미 있으면 건너뛴다.
#      - 플레이리스트 목록은 input-details/playlists/<list-id>.tsv 에 캐시되어 재실행 시 네트워크 없이 재사용된다.
#   2. 플레이리스트면 먼저 `rea-movie series-watch` 로 <output-root>/story/{world.json,characters.json,timeline-days.md}
#      를 확보한다 (이미 series-watch 가 스킵).
#   3. --episodes 로 지정한 에피소드만(미지정 시 전체) rea-movie decompile (-o <output-root>/h3-projects/<stem>).
#      decompile 은 <output-root>/input-details/<source_id>/ 의 probe/scene_cuts/자막/키프레임/샷 캐시를 재사용한다.
#
# 환경변수:
#   REA_MOVIE_BIN  rea-movie 바이너리 경로 (기본: PATH 의 rea-movie)
#   YTDLP_BIN      yt-dlp 경로 (기본: PATH 의 yt-dlp)
#
# 예:
#   ./reverse-url.sh "https://www.youtube.com/watch?v=XXXX" ~/rea-movie-artifacts --range 0-120
#   ./reverse-url.sh "https://www.youtube.com/playlist?list=PLxx" ~/rea-movie-artifacts --episodes 1,2 -- --dub-lang ko
set -euo pipefail

usage() {
  echo "Usage: $0 <youtube-url> <output-root> [--range START-END] [--episodes 1,2|1-3] [--offline] [--force] [-- <decompile args...>]"
}

if [[ $# -lt 2 ]]; then
  usage
  exit 2
fi

URL="$1"
ROOT="$2"
shift 2

RANGE=""
EPISODES=""
OFFLINE=0
FORCE=0
EXTRA=()
while [[ $# -gt 0 ]]; do
  case "$1" in
    --range)
      RANGE="$2"
      shift 2
      ;;
    --episodes)
      EPISODES="$2"
      shift 2
      ;;
    --offline)
      OFFLINE=1
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    --)
      shift
      EXTRA+=("$@")
      break
      ;;
    *)
      EXTRA+=("$1")
      shift
      ;;
  esac
done

YTDLP="${YTDLP_BIN:-yt-dlp}"
BIN="${REA_MOVIE_BIN:-$(command -v rea-movie || true)}"
if [[ -z "${BIN}" ]]; then
  echo "Error: rea-movie 바이너리를 찾을 수 없습니다. REA_MOVIE_BIN 을 설정하세요." >&2
  exit 1
fi

mkdir -p "${ROOT}"
ROOT="$(cd "${ROOT}" && pwd)"
SRC_DIR="${ROOT}/input-details"
LEGACY_SRC_DIR="${ROOT}/input"
OLD_SRC_DIR="${ROOT}/sources"
OLD_LEGACY_SRC_DIR="${ROOT}/source"
PROJ_ROOT="${ROOT}/h3-projects"
mkdir -p "${SRC_DIR}" "${PROJ_ROOT}"

require_ytdlp() {
  if ! command -v "${YTDLP}" >/dev/null 2>&1; then
    echo "Error: yt-dlp 가 설치되어 있지 않습니다 (캐시에 없는 영상을 받으려면 필요)." >&2
    exit 1
  fi
}

# --episodes "1,2,5-7" -> 포함 여부
episode_selected() {
  local n="$1"
  [[ -z "${EPISODES}" ]] && return 0
  local part a b
  IFS=',' read -r -a parts <<< "${EPISODES}"
  for part in "${parts[@]}"; do
    part="${part// /}"
    if [[ "${part}" == *-* ]]; then
      a="${part%-*}"
      b="${part#*-}"
      if (( 10#${n} >= 10#${a} && 10#${n} <= 10#${b} )); then return 0; fi
    elif [[ -n "${part}" ]] && (( 10#${n} == 10#${part} )); then
      return 0
    fi
  done
  return 1
}

# 캐시된 영상 파일 (video.* 심볼릭 링크 제외, 실제 다운로드본 우선)
cached_video() {
  local dir="$1" f
  [[ -d "${dir}" ]] || return 1
  for f in "${dir}"/*.mp4 "${dir}"/*.mkv "${dir}"/*.webm; do
    [[ -e "${f}" ]] || continue
    case "$(basename "${f}")" in video.*) continue ;; esac
    echo "${f}"
    return 0
  done
  for f in "${dir}"/video.*; do
    [[ -e "${f}" ]] || continue
    echo "${f}"
    return 0
  done
  return 1
}

# ------------------------------------------------------------------------------
# 1. 영상 목록 (index|id) 확보
# ------------------------------------------------------------------------------
IS_PLAYLIST=0
ENTRIES=()
if [[ "${URL}" == *"list="* ]]; then
  IS_PLAYLIST=1
  LIST_ID="$(sed -n 's/.*[?&]list=\([A-Za-z0-9_-]*\).*/\1/p' <<< "${URL}")"
  PL_CACHE="${SRC_DIR}/playlists/${LIST_ID:-unknown}.tsv"
  if [[ -s "${PL_CACHE}" && "${FORCE}" -eq 0 ]]; then
    echo "==> Reuse cached playlist listing: ${PL_CACHE}"
  else
    require_ytdlp
    echo "==> Listing playlist: ${URL}"
    mkdir -p "$(dirname "${PL_CACHE}")"
    "${YTDLP}" --flat-playlist --yes-playlist --print "%(playlist_index)s	%(id)s" "${URL}" \
      | awk -F'\t' 'NF>=2 && $2!="" {print}' > "${PL_CACHE}.tmp"
    mv "${PL_CACHE}.tmp" "${PL_CACHE}"
  fi
  while IFS=$'\t' read -r idx vid; do
    [[ -n "${vid}" ]] || continue
    [[ "${idx}" =~ ^[0-9]+$ ]] || idx=$(( ${#ENTRIES[@]} + 1 ))
    ENTRIES+=("${idx}|${vid}")
  done < "${PL_CACHE}"
else
  VID="$(sed -n -E 's#.*(v=|youtu\.be/|shorts/|embed/)([A-Za-z0-9_-]{11}).*#\2#p' <<< "${URL}")"
  if [[ -z "${VID}" ]]; then
    require_ytdlp
    VID="$("${YTDLP}" --no-playlist --print "%(id)s" "${URL}" | head -n1)"
  fi
  ENTRIES+=("1|${VID}")
fi

if [[ ${#ENTRIES[@]} -eq 0 ]]; then
  echo "Error: 영상 목록이 비어 있습니다: ${URL}" >&2
  exit 1
fi

# ------------------------------------------------------------------------------
# 2. 영상별 input-details/yt-<id>/ 캐시 확보 (다운로드 / 레거시 재사용 / vtt->srt)
# ------------------------------------------------------------------------------
VIDEOS=()
VIDEO_EPS=()
for entry in "${ENTRIES[@]}"; do
  idx="${entry%%|*}"
  vid="${entry#*|}"
  dir="${SRC_DIR}/yt-${vid}"
  ep="$(printf '%02d' "$((10#${idx}))")"

  video="$(cached_video "${dir}" || true)"
  # 구버전 폴백: 기존 sources/yt-<id>/ 캐시에 파일이 있으면 재사용
  if [[ -z "${video}" && "${FORCE}" -eq 0 ]]; then
    video="$(cached_video "${OLD_SRC_DIR}/yt-${vid}" || true)"
    if [[ -n "${video}" ]]; then
      echo "==> [EP${ep}] Reuse legacy sources/ cache: ${video}"
      mkdir -p "${dir}"
      lstem="$(basename "${video}")"
      lstem="${lstem%.*}"
      for f in "${OLD_SRC_DIR}/yt-${vid}/${lstem}".*; do
        [[ -e "${f}" ]] || continue
        ln -sfn "${f}" "${dir}/$(basename "${f}")"
      done
      video="$(cached_video "${dir}" || true)"
      [[ -n "${video}" ]] || video="$(cached_video "${OLD_SRC_DIR}/yt-${vid}" || true)"
    fi
  fi
  if [[ -n "${video}" && "${FORCE}" -eq 0 ]]; then
    echo "==> [EP${ep}] Reuse cached source (skip yt-dlp download): ${video}"
  else
    legacy=""
    legacy_base=""
    if [[ "${FORCE}" -eq 0 ]]; then
      for cand_base in "${LEGACY_SRC_DIR}" "${OLD_LEGACY_SRC_DIR}"; do
        if [[ -d "${cand_base}" ]]; then
          for f in "${cand_base}"/*"[${vid}]".mp4; do
            [[ -e "${f}" ]] && legacy="${f}" && legacy_base="${cand_base}" && break
          done
        fi
        [[ -n "${legacy}" ]] && break
      done
    fi
    mkdir -p "${dir}"
    if [[ -n "${legacy}" ]]; then
      echo "==> [EP${ep}] Reuse legacy input/ download (skip yt-dlp download): ${legacy}"
      lstem="$(basename "${legacy}" .mp4)"
      for f in "${legacy_base}/${lstem}".*; do
        [[ -e "${f}" ]] || continue
        ln -sfn "${f}" "${dir}/$(basename "${f}")"
      done
    else
      require_ytdlp
      echo "==> [EP${ep}] Downloading ${vid} -> ${dir}/"
      "${YTDLP}" --no-playlist \
        -f "bv*[height<=720][ext=mp4]+ba[ext=m4a]/b[height<=720][ext=mp4]/b[ext=mp4]/bv*+ba/b" \
        --merge-output-format mp4 \
        --write-subs --sub-langs "ko,en" --embed-subs \
        -o "${dir}/${ep}_%(title)s [%(id)s].%(ext)s" \
        "https://www.youtube.com/watch?v=${vid}"
    fi
    video="$(cached_video "${dir}" || true)"
  fi
  if [[ -z "${video}" ]]; then
    echo "Warning: [EP${ep}] 영상 파일을 찾지 못했습니다 (${dir}); 건너뜀" >&2
    continue
  fi

  # 자막 sidecar (.vtt -> .srt): sibling srt 우선순위(ko > srt > en)에 맞춤. 이미 있으면 재사용.
  stem="$(basename "${video}")"
  stem="${stem%.*}"
  for vtt in "${dir}/${stem}".*.vtt; do
    [[ -e "${vtt}" ]] || continue
    lang="$(basename "${vtt}" | sed -n "s/.*\.\([a-zA-Z-]*\)\.vtt/\1/p")"
    [[ -n "${lang}" ]] || lang="en"
    srt="${dir}/${stem}.${lang}.srt"
    if [[ -s "${srt}" && "${FORCE}" -eq 0 ]]; then
      echo "    Reuse cached subtitle: ${srt}"
      continue
    fi
    ffmpeg -nostdin -v error -y -i "${vtt}" "${srt}" || true
  done

  VIDEOS+=("${video}")
  VIDEO_EPS+=("${idx}")
done

if [[ ${#VIDEOS[@]} -eq 0 ]]; then
  echo "Error: 처리할 영상이 없습니다." >&2
  exit 1
fi

COMMON_FLAGS=()
[[ "${OFFLINE}" -eq 1 ]] && COMMON_FLAGS+=(--offline)

# ------------------------------------------------------------------------------
# 3. 플레이리스트: series-watch 로 story/{world,characters,timeline-days} 선행 확보
# ------------------------------------------------------------------------------
if [[ "${IS_PLAYLIST}" -eq 1 ]]; then
  echo "==> Series watch (all ${#VIDEOS[@]} episode(s)) -> ${ROOT}/story/"
  SW_FLAGS=("${COMMON_FLAGS[@]+"${COMMON_FLAGS[@]}"}")
  [[ "${FORCE}" -eq 1 ]] && SW_FLAGS+=(--force)
  "${BIN}" series-watch --out-root "${ROOT}" "${SW_FLAGS[@]+"${SW_FLAGS[@]}"}" "${VIDEOS[@]}"
fi

# ------------------------------------------------------------------------------
# 4. decompile (선택된 에피소드만, 캐시 재사용)
# ------------------------------------------------------------------------------
echo "==> Decompiling into ${PROJ_ROOT}/${EPISODES:+ (episodes: ${EPISODES})}"
FAILED=0
for i in "${!VIDEOS[@]}"; do
  video="${VIDEOS[$i]}"
  ep="${VIDEO_EPS[$i]}"
  if ! episode_selected "${ep}"; then
    echo "---- Skip EP$(printf '%02d' "$((10#${ep}))") (not in --episodes ${EPISODES})"
    continue
  fi
  stem="$(basename "${video}")"
  stem="${stem%.*}"
  dest="${PROJ_ROOT}/${stem}"
  if [[ -d "${dest}" && "${FORCE}" -eq 0 ]]; then
    echo "---- Skip (exists): ${dest} (--force 로 재생성하거나 -o 로 다른 이름 지정)"
    continue
  fi
  DC_FLAGS=("${COMMON_FLAGS[@]+"${COMMON_FLAGS[@]}"}")
  [[ "${FORCE}" -eq 1 ]] && DC_FLAGS+=(--force)
  if ! "${BIN}" decompile "${video}" -o "${dest}" ${RANGE:+--range "${RANGE}"} \
      "${DC_FLAGS[@]+"${DC_FLAGS[@]}"}" "${EXTRA[@]+"${EXTRA[@]}"}"; then
    echo "Warning: decompile failed for ${video}" >&2
    FAILED=$((FAILED + 1))
  fi
done

echo "==> Done. Input details: ${SRC_DIR}/ | Input: ${LEGACY_SRC_DIR}/ | Story: ${ROOT}/story/ | Projects: ${PROJ_ROOT}/ | Results: ${ROOT}/h3-results/"
[[ "${FAILED}" -eq 0 ]]
