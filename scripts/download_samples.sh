#!/usr/bin/env bash
# download_samples.sh
# 퍼블릭 도메인(저작권 만료) 고전 명작 영화 5편과 저작권 설명 문서(.md)를
# 공유 NAS 아티팩트(/rosenas/data/rea-movie-artifacts/sample_movies/)에 준비합니다.
# 이미 다운로드된 영상 파일이 존재하면 기본적으로 건너뛰며(Skip), --force 지정 시 재다운로드합니다.
#
# 사용법:
#   ./download_samples.sh                # 5편 전체 다운로드 및 .md 생성 (존재 시 스킵)
#   ./download_samples.sh 2              # 2번(슈퍼맨 1941, 10분 단편)만 다운로드
#   ./download_samples.sh 1 --force      # 1번(샤레이드 1963) 강제 재다운로드
set -euo pipefail

WORKSPACE_DIR="${REA_MOVIE_WORKSPACE:-/rosenas/data/rea-movie-artifacts}"
SAMPLE_DIR="$WORKSPACE_DIR/sample_movies"
mkdir -p "$SAMPLE_DIR" "$WORKSPACE_DIR/h3-projects"

FORCE=0
FILTER_ID=""

for arg in "$@"; do
    case "$arg" in
        --force|-f)
            FORCE=1
            ;;
        [1-5]|0[1-5])
            FILTER_ID="$(printf "%02d" "$((10#$arg))")"
            ;;
        -h|--help)
            echo "Usage: $0 [1..5] [--force]"
            exit 0
            ;;
    esac
done

write_copyright_docs() {
    cat > "$SAMPLE_DIR/01_charade_1963.md" <<'EOF'
# 01. 샤레이드 (*Charade*, 1963)

- **파일 경로**: `/rosenas/data/rea-movie-artifacts/sample_movies/01_charade_1963.mp4`
- **감독**: 스탠리 도넌 (Stanley Donen)
- **주연**: 케리 그랜트 (Cary Grant), 오드리 헵번 (Audrey Hepburn), 월터 매소 (Walter Matthau)
- **개봉 연도**: 1963년 (미국, Universal Pictures 배급)
- **러닝타임**: 약 113분 (컬러)
- **YouTube 출처 URL**: `https://www.youtube.com/watch?v=6ROvPFaUl80`

---

## 📜 저작권 상태 및 법적 근거 (Public Domain Status)

- **저작권 상태**: **퍼블릭 도메인 (Public Domain — 저작권 소멸 / 자유 이용 가능)**
- **법적·역사적 사유**:
  - 1978년 개정 이전의 미국 저작권법(1909 Copyright Act)은 공표되는 모든 저작물에 정확한 형식의 저작권 고지(`Copyright` 또는 `©` 기호 + 연도 + 권리자 성명)를 필수 요건으로 규정하고 있었습니다.
  - 1963년 개봉 당시 배급사인 유니버설 픽처스(Universal Pictures)는 영화 오프닝 타이틀 카드에 `MCMLXIII BY UNIVERSAL PICTURES COMPANY, INC. and STANLEY DONEN FILMS, INC. ALL RIGHTS RESERVED`라고만 표기하고, 정작 법적으로 필수인 **`Copyright` 단어 또는 `©` 기호를 누락**하는 실수를 범했습니다.
  - 이로 인해 본 영화의 영상과 오디오 트랙은 미국 개봉과 동시에 퍼블릭 도메인(공유 저작물)으로 귀속되었으며, 저작권 침해 없이 자유로운 다운로드, 편집, AI 역공학 재현 및 2차 창작에 활용할 수 있습니다.

---

## 🎬 `rea-movie` 실행 예시 (공유 워크스페이스에 프로젝트 자동 생성)

```bash
# -o 생략 시 자동으로 /rosenas/data/rea-movie-artifacts/h3-projects/01_charade_1963 에 생성됩니다.
rea-movie /rosenas/data/rea-movie-artifacts/sample_movies/01_charade_1963.mp4 --range 00:01:00-00:02:00
```
EOF

    cat > "$SAMPLE_DIR/02_superman_mad_scientist_1941.md" <<'EOF'
# 02. 슈퍼맨: 매드 사이언티스트 (*Superman: The Mad Scientist*, 1941)

- **파일 경로**: `/rosenas/data/rea-movie-artifacts/sample_movies/02_superman_mad_scientist_1941.mp4`
- **제작 / 감독**: 플라이셔 스튜디오 (Fleischer Studios) / 데이브 플라이셔 (Dave Fleischer)
- **성우**: 버드 콜리어 (Bud Collyer), 조안 알렉산더 (Joan Alexander)
- **개봉 연도**: 1941년 9월 26일 (미국, Paramount Pictures 배급)
- **러닝타임**: 약 10분 37초 (테크니컬러 애니메이션 단편)
- **YouTube 출처 URL**: `https://www.youtube.com/watch?v=9UJphNPwDfk`

---

## 📜 저작권 상태 및 법적 근거 (Public Domain Status)

- **저작권 상태**: **퍼블릭 도메인 (Public Domain — 저작권 갱신 미이행으로 만료)**
- **법적·역사적 사유**:
  - 미국 1909년 저작권법 체계에서 1941년에 개봉한 본 단편 영화는 최초 28년의 보호 기간이 끝나는 28년 차(1969년)에 저작권 갱신(Copyright Renewal) 등록을 해야만 보호 기간이 연장될 수 있었습니다.
  - 원 권리자(Paramount Pictures / National Comics)가 플라이셔 스튜디오 제작 슈퍼맨 단편 시리즈 17편 전체에 대해 28년 차 저작권 갱신 절차를 이행하지 않아, **1969년부로 본 영상물의 저작권이 공식 만료되어 퍼블릭 도메인**에 진입했습니다.
  - 러닝타임이 약 10분으로 완결성을 갖추고 있어 `rea-movie`로 단편 1편 전체를 역공학 및 재생성하기에 가장 이상적인 샘플입니다.

---

## 🎬 `rea-movie` 실행 예시 (공유 워크스페이스에 프로젝트 자동 생성)

```bash
# -o 생략 시 자동으로 /rosenas/data/rea-movie-artifacts/h3-projects/02_superman_mad_scientist_1941 에 생성됩니다.
rea-movie /rosenas/data/rea-movie-artifacts/sample_movies/02_superman_mad_scientist_1941.mp4 --range 00:00:30-00:02:30
```
EOF

    cat > "$SAMPLE_DIR/03_night_of_the_living_dead_1968.md" <<'EOF'
# 03. 살아있는 시체들의 밤 (*Night of the Living Dead*, 1968)

- **파일 경로**: `/rosenas/data/rea-movie-artifacts/sample_movies/03_night_of_the_living_dead_1968.mp4`
- **감독**: 조지 A. 로메로 (George A. Romero)
- **주연**: 듀안 존스 (Duane Jones), 주디스 오디어 (Judith O'Dea)
- **개봉 연도**: 1968년 10월 1일 (미국, Image Ten 제작 / Walter Reade Organization 배급)
- **러닝타임**: 약 96분 (흑백 리마스터)
- **YouTube 출처 URL**: `https://www.youtube.com/watch?v=SEcsfcDCoVg`

---

## 📜 저작권 상태 및 법적 근거 (Public Domain Status)

- **저작권 상태**: **퍼블릭 도메인 (Public Domain — 저작권 소멸)**
- **법적·역사적 사유**:
  - 제작사 Image Ten은 원래 가제인 *Night of the Flesh Eaters*의 타이틀 프레임 하단에 저작권 고지 문구를 넣어 두었으나, 배급사(Walter Reade Organization)가 개봉 직전 영화 제목을 *Night of the Living Dead*로 교체하면서 새로 제작한 타이틀 카드에 **저작권 고지 문구(`© 1968 Image Ten...`)를 실수로 누락**했습니다.
  - 당시 미국 저작권법은 저작권 고지 없이 배포된 저작물을 즉시 퍼블릭 도메인으로 간주했기 때문에, 본 작품은 개봉과 동시에 공유 저작물이 되었습니다.

---

## 🎬 `rea-movie` 실행 예시 (공유 워크스페이스에 프로젝트 자동 생성)

```bash
# -o 생략 시 자동으로 /rosenas/data/rea-movie-artifacts/h3-projects/03_night_of_the_living_dead_1968 에 생성됩니다.
rea-movie /rosenas/data/rea-movie-artifacts/sample_movies/03_night_of_the_living_dead_1968.mp4 --range 00:03:00-00:04:30
```
EOF

    cat > "$SAMPLE_DIR/04_his_girl_friday_1940.md" <<'EOF'
# 04. 히즈 걸 프라이데이 (*His Girl Friday*, 1940)

- **파일 경로**: `/rosenas/data/rea-movie-artifacts/sample_movies/04_his_girl_friday_1940.mp4`
- **감독**: 하워드 혹스 (Howard Hawks)
- **주연**: 케리 그랜트 (Cary Grant), 로잘린드 러셀 (Rosalind Russell)
- **개봉 연도**: 1940년 1월 18일 (미국, Columbia Pictures)
- **러닝타임**: 약 92분 (흑백 HD 복원본)
- **YouTube 출처 URL**: `https://www.youtube.com/watch?v=MWVZXblHWXI`

---

## 📜 저작권 상태 및 법적 근거 (Public Domain Status)

- **저작권 상태**: **퍼블릭 도메인 (Public Domain — 저작권 갱신 미이행으로 만료)**
- **법적·역사적 사유**:
  - 1940년에 공개된 본 영화는 최초 28년 저작권 보호 기간이 만료되는 1968년에 저작권 갱신(Renewal) 신청이 이루어지지 않아 **1968년부로 미국 저작권이 소멸하고 퍼블릭 도메인**이 되었습니다(원작 희곡 *The Front Page* 역시 1928년 출간되어 현재 미국 저작권 만료).

---

## 🎬 `rea-movie` 실행 예시 (공유 워크스페이스에 프로젝트 자동 생성)

```bash
# -o 생략 시 자동으로 /rosenas/data/rea-movie-artifacts/h3-projects/04_his_girl_friday_1940 에 생성됩니다.
rea-movie /rosenas/data/rea-movie-artifacts/sample_movies/04_his_girl_friday_1940.mp4 --range 00:02:00-00:03:00
```
EOF

    cat > "$SAMPLE_DIR/05_sherlock_jr_1924.md" <<'EOF'
# 05. 셜록 주니어 (*Sherlock Jr.*, 1924)

- **파일 경로**: `/rosenas/data/rea-movie-artifacts/sample_movies/05_sherlock_jr_1924.mp4`
- **감독 / 주연**: 버스터 키튼 (Buster Keaton)
- **개봉 연도**: 1924년 4월 21일 (미국, Metro Pictures)
- **러닝타임**: 약 45분 (흑백 복원본)
- **YouTube 출처 URL**: `https://www.youtube.com/watch?v=gN5OLNaFk0I`

---

## 📜 저작권 상태 및 법적 근거 (Public Domain Status)

- **저작권 상태**: **퍼블릭 도메인 (Public Domain — 저작권 보호 기간 만료)**
- **법적·역사적 사유**:
  - 미국 저작권법상 공표 후 95년이 경과한 모든 1924년작 영화는 **2020년 1월 1일부로 저작권 보호 기간이 완전히 만료되어 전 세계적인 퍼블릭 도메인**에 편입되었습니다.

---

## 🎬 `rea-movie` 실행 예시 (공유 워크스페이스에 프로젝트 자동 생성)

```bash
# -o 생략 시 자동으로 /rosenas/data/rea-movie-artifacts/h3-projects/05_sherlock_jr_1924 에 생성됩니다.
rea-movie /rosenas/data/rea-movie-artifacts/sample_movies/05_sherlock_jr_1924.mp4 --range 00:15:00-00:16:30
```
EOF
}

write_copyright_docs

if ! command -v yt-dlp >/dev/null 2>&1; then
    echo "Error: yt-dlp 가 설치되어 있지 않습니다." >&2
    exit 1
fi

# ID | 파일 베이스명 | YouTube URL | 제목
MOVIES=(
    "01|01_charade_1963|https://www.youtube.com/watch?v=6ROvPFaUl80|Charade (1963, Public Domain)"
    "02|02_superman_mad_scientist_1941|https://www.youtube.com/watch?v=9UJphNPwDfk|Superman: The Mad Scientist (1941, Public Domain)"
    "03|03_night_of_the_living_dead_1968|https://www.youtube.com/watch?v=SEcsfcDCoVg|Night of the Living Dead (1968, Public Domain)"
    "04|04_his_girl_friday_1940|https://www.youtube.com/watch?v=MWVZXblHWXI|His Girl Friday (1940, Public Domain)"
    "05|05_sherlock_jr_1924|https://www.youtube.com/watch?v=gN5OLNaFk0I|Sherlock Jr. (1924, Public Domain)"
)

for entry in "${MOVIES[@]}"; do
    IFS='|' read -r id base url title <<< "$entry"
    if [ -n "$FILTER_ID" ] && [ "$id" != "$FILTER_ID" ]; then
        continue
    fi

    target_mp4="$SAMPLE_DIR/${base}.mp4"
    target_md="$SAMPLE_DIR/${base}.md"

    echo "=================================================================="
    echo " 🎬 [$id/05] $title"
    echo "    Video : $target_mp4"
    echo "    Doc   : $target_md"
    echo "=================================================================="

    if [ -f "$target_mp4" ] && [ -s "$target_mp4" ] && [ "$FORCE" -eq 0 ]; then
        echo "✅ Skip: 이미 존재합니다 ($(du -h "$target_mp4" | cut -f1)). 재다운로드하려면 --force 옵션을 주세요."
        continue
    fi

    echo "⬇️  Downloading from $url ..."
    yt-dlp \
        --no-playlist \
        --js-runtimes node \
        --remote-components ejs:github \
        --ignore-errors \
        -f "bv*[height<=720][ext=mp4]+ba[ext=m4a]/b[height<=720][ext=mp4]/b[ext=mp4]/bv*+ba/b" \
        --merge-output-format mp4 \
        --write-subs --sub-langs "en" --embed-subs \
        -o "$target_mp4" \
        "$url"

    ls -lh "$target_mp4"
done

echo ""
echo "🎉 모든 샘플 영화 및 저작권 문서 준비 완료: $SAMPLE_DIR"
