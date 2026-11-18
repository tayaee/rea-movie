---
name: rea-movie
description: Reverse-engineer any movie into reproducible MiniMax-H3 multi-shot video project with DGX Spark pool orchestration
---

# `rea-movie` 스킬 — 영화 역공학 및 MiniMax-H3 다중 샷 렌더링 오케스트레이션

이 문서는 AI 에이전트(Orchestrator, Coder, Reviewer)가 `rea-movie` CLI, 단일 진입점 `./run.sh`, DGX Spark 서버 풀, 그리고 MCP 도구를 효과적으로 활용하여 영화를 역공학하고 고품질 비디오를 재생성 및 검증할 수 있도록 돕는 실전 가이드입니다.

---

## 1. 프로젝트 핵심 아키텍처 및 철학

- **목표**: 임의의 영화/영상(`mp4`, `mkv`)을 입력받아 샷 분할, 앵커 프레임(`first_frame.jpg`, `last_frame.jpg`), 타임스탬프 번인 스토리보드(`contact_sheet.jpg`), 한국어 연출서(`prompt.ko.md`), MiniMax-H3 영문 프롬프트(`prompt.h3.txt`)를 생성하고, DGX Spark 서버 풀에서 각 샷을 렌더링한 후 최종 영화 및 좌우 1:1 동시 비교 영상(`_compare.mp4`)으로 조립한다.
- **핵심 엔지니어링 원칙**:
  1. **최대 샷 길이 8.0초 이하**: DGX Spark 128GB 통합 메모리 OOM 방지 및 생성 일관성 유지.
  2. **모핑 방지**: 2개의 다른 카메라 앵글/컷을 하나의 `fl2va` 샷으로 묶지 않음.
  3. **Prod vs Smoke 완전 격리**:
     - Prod 기본: `960x576`, `steps=50`, `fps=24` → `shots/shot_XXXX/renders/prod/`, `outputs/prod/`
     - Smoke 모드: `608x352`, `steps=10`, `fps=24` → `shots/shot_XXXX/renders/smoke/`, `outputs/smoke/`
     - 해상도는 32 배수(VAE 타일·패치 패러렐 요구)이어야 함. `640x360` 불가 → `608x352` 사용.
  4. **Skip-by-default & Take History 백업**:
     - 타겟 파일이 이미 존재하면 기본적으로 건너뜀(Skip).
     - `--force` 옵션으로 재생성 시 이전 결과물은 `history/take_NNN.mp4`로 안전하게 백업.
  5. **듀얼 오디오 트랙 및 1:1 비교 영상**:
     - 최종 본편(`outputs/<profile>/<movie_name>_<profile>.mp4`): Track 1(H3 오디오) + Track 2(원본 영화 오디오).
     - 비교 영상(`outputs/<profile>/<movie_name>_compare.mp4`): [좌: 원본 | 우: H3 렌더본 + `Shot #XXXX` 라벨].

## 1-2. 출력 디렉토리 구조 (필수 숙지)

- 역공학 프로젝트(가벼움, git 추적): `~/rea-movie-artifacts/h3-projects/<name>/`
- 렌더 결과물(무거움, git 제외): `~/rea-movie-artifacts/h3-results/<name>/` (형제 `../h3-results/` 자동 매핑)
- 풀 워크스페이스(`serve-h3.sh`, node-state): `~/rea-movie-artifacts`
- 세 호스트(wsl2, spark1, spark2)의 `~/rea-movie-artifacts`는 모두 `/rosenas/data/rea-movie-artifacts` 심볼릭 링크.
- 구 레이아웃(`rea-movie-projects/` → `rea-movie-workspace/`)은 마이그레이션 폴백으로만 인식됨. 신규 작업에 사용 금지.

---

## 2. CLI 기본 명령어 및 실행 흐름

**역할 분담**: 판단·협상·품질 게이트는 에이전트(스킬)가, 반복 실행은 바이너리가 맡는다.
모호한 입력(URL, "원본처럼"), 바이블 검수, 장애 대응(§7)은 스킬로.
정해진 단일 동작(특정 샷 재렌더, assemble, pool status), 스크립트·CI·회귀는 바이너리 직접 실행.
에이전트가 바이너리 작업을 손으로 대신하지 말 것 (66개 프롬프트 수기 편집 금지 —
`customize apply`, `./run.sh <shot>` 사용).

### A. 영화 역공학 (Decompile)
입력 영상을 분석하여 샷별 에셋 및 프롬프트 프로젝트를 생성합니다:
```bash
# 기본 역공학: -o 생략 시 ~/rea-movie-artifacts/h3-projects/<파일명>/ 에 생성
# + MiniMax-M3 비전 API로 한국어 연출서 자동 작성 (MINIMAX_API_KEY 필요)
rea-movie robocop.mp4 --range 00:01:10-00:02:30

# 자막 파일은 입력 영상과 동명 sibling .srt여야 함 (예: robocop.en.srt, .ko.srt, .diarized.srt)
# 자막 큐는 시작점이 속한 샷 하나에만 정확히-한-번 할당됨 (마이크로샷 반복 방지)

# 대사 원문 그대로 재현이 기본 (verbatim). 번역·의역·창작 금지. <d> 태그 언어는 본문과 자동 일치.
rea-movie robocop.mp4 -o ./robocop_proj --offline   # 원문 유지 (기본)
rea-movie robocop.mp4 -o ./robocop_proj --translate-dub  # 레거시 KO 더빙 번역 (명시 시에만)

# 미세 컷 병합: 너무 짧은 샷을 인접 샷과 병합 (기본값 0.0 = 병합 없음)
rea-movie robocop.mp4 -o ./robocop_proj --offline --min-shot-duration 1.0

# 더빙 언어 (기본값 ko; H3가 말하는 언어. 원본 유지가 목적이면 ko)
rea-movie robocop.mp4 -o ./robocop_proj --dub-lang ko   # 기본값이라 생략 가능
rea-movie robocop.mp4 -o ./robocop_proj --dub-lang en   # 영어 더빙이 필요할 때만

# 오프라인/테스트 모드 (비전 API 호출 없이 템플릿 프롬프트 생성; 번역도 테이블 폴백)
rea-movie robocop.mp4 -o ./robocop_proj --offline

# 역공학 완료 후 즉시 렌더 및 조립까지 자동 실행
rea-movie robocop.mp4 -o ./robocop_proj --run
```

### B. 단일 진입점 스크립트 (`./run.sh`)
생성된 프로젝트 루트 디렉토리에서 `./run.sh`를 통해 렌더링과 조립을 원스톱으로 수행합니다:
```bash
cd ~/rea-movie-artifacts/h3-projects/robocop_proj
REA_MOVIE_BIN=/path/to/rea-movie ./run.sh --smoke   # 바이너리 위치 명시 권장

# 1. Prod 기본 렌더링 및 조립 (960x576, 50 steps)
./run.sh

# 2. Smoke 고속 렌더링 및 조립 (608x352, 10 steps) — 먼저 여기로 검증
./run.sh --smoke

# 3. 특정 단일 샷 재생성 (--force로 이전 파일 백업 후 재렌더링)
./run.sh 0005 --force

# 4. 프롬프트 재컴파일 없이 비디오만 재렌더링 (시드 변경 또는 H3 재시도)
./run.sh 0005 --force-video-only

# 5. 특정 범위 샷 재생성
./run.sh 0001-0005 --force
```

### C. 수동 조립 (`rea-movie assemble`)
렌더링된 샷들을 본편 영화 및 좌우 비교 영상으로 조립합니다:
```bash
# Prod 프로필 조립 (outputs/prod/robocop_prod.mp4, outputs/prod/robocop_compare.mp4)
rea-movie assemble

# Smoke 프로필 조립 (outputs/smoke/robocop_smoke.mp4, outputs/smoke/robocop_compare.mp4)
rea-movie assemble --smoke

# 증명용 비디오도 함께 생성 (화자 태그 자막 번인: [화자] 대사 1줄 통합)
rea-movie assemble --smoke --proof
# 오프라인 조립 (M3 자막 번역 없이 결정적 폴백만 사용)
rea-movie assemble --smoke --offline --proof
```
- 조립 시 6개국어 외부 자막 자동 생성: `<movie>_<profile>.{ko,zh,ja,en,fr,de}.srt` + 항상 `.proof.srt`
  (원본: `<proj>/story/transcript.srt`, 번역 캐시: `<proj>/story/subtitles_cache/<lang>.srt`).
- 원본 비디오는 건드리지 않고, 4.0초 미만 샷의 H3 렌더 클립은 끝 프레임까지 살려
  `target_dur`에 리타이밍 (`setpts` 비디오 + `atempo` 오디오) — 단순 절단 금지 (끝 앵커 소실 버그 방지).
- 최종 본편 듀얼 오디오: Track 1(H3) + Track 2(원본). **기본(default) 트랙은 원본 오디오.**
- 비교 영상 원본 클립 오디오는 원본 우선 매핑.

### E. 로컬 목업 렌더 (`--mock-h3`)
GPU 풀 없이 파이프라인 검증:
```bash
rea-movie render --proj-dir <proj> --smoke --mock-h3   # 전 샷 목업 렌더
rea-movie render --proj-dir <proj> --smoke --mock-h3 --shot 0005  # 단일 샷
```

### D. URL 인테이크 ("<youtube url>을 역공학하여 <dir>에 저장해")
사용자가 URL로 지시하면 에이전트 표준 플로우 (`scripts/reverse-url.sh`):
```bash
# 단일 영상 또는 플레이리스트를 <output-root>에 역공학 (범위 옵션 전달 가능)
./scripts/reverse-url.sh "<youtube-url>" ~/rea-movie-artifacts --range 0-120
./scripts/reverse-url.sh "<playlist-url>" ~/rea-movie-artifacts -- --dub-lang ko
# 플레이리스트에서 원하는 에피소드만 선별 역공학
./scripts/reverse-url.sh "<playlist-url>" ~/rea-movie-artifacts --episodes 1,2 --offline
./scripts/reverse-url.sh "<playlist-url>" ~/rea-movie-artifacts --episodes 1-3 --force
```
1. yt-dlp로 720p mp4 + 자막(ko,en)을 `<output-root>/sources/yt-<id>/`에 다운로드 (플레이리스트면 자동 전체).
   - 동일 영상 ID가 이미 `sources/`에 있으면 yt-dlp 다운로드를 건너뛰고 재사용 (중복 다운로드 방지).
   - 구 레이아웃 `<output-root>/source/*[<id>].mp4`가 있으면 `sources/yt-<id>/`로 링크해 재사용 (레거시 폴백).
   - 자막 vtt → sibling `.srt` 변환도 `.srt`가 이미 있으면 건너뜀.
   - 플레이리스트 목록은 `sources/playlists/<list-id>.tsv`에 캐시되어 재실행 시 네트워크 없이 재사용.
2. decompile은 `<output-root>/sources/<source_id>/`의 probe/scene_cuts/자막/키프레임/샷 캐시를 재사용
   (동일 소스 재실행 시 느린 `select='gt(scene,0.14)'` 씬 감지 0초 스킵, 샷 에셋 원자적 복사).
3. 플레이리스트면 decompile 전에 먼저 `rea-movie series-watch --out-root <root>`를 실행하여
   `<root>/story/{world.json,characters.json,timeline-days.md}`를 선행 확보 (이미 있으면 스킵).
4. `--episodes 1,2` 또는 `1-3`으로 지정한 에피소드만 풀 decompile (미지정 시 전체).
이후는 에이전트가 최고의 연출 스킬 전문가로 행동: 시리즈 선행 분석(§3-0) →
씬 바이블 검토 → 샷별 연출서 다듬기 (§3-2) → smoke 검증 → 본편 조립.

### 3-0. 시리즈 전체 선행 분석 (Series Watch) — 기본 동작
**부분 역공학 요청이 와도 전체 시리즈를 먼저 요구하는 것이 기본값이다.**
smoke든 prod든, 1분만 뽑든 전편을 뽑든, 연출 지시는 시리즈 전체를 본 뒤에
작성한다. 세계관/인물/공간/연대/지역/연출문법의 일관성은 전편 없이는 보장
불가능하기 때문이다.

에이전트 표준 멘트: "일관된 연출을 위해 시리즈 전체 영상이 필요합니다.
전편을 주세요. 일부만으로 진행할 수도 있지만, 그 경우 인물·장소·시간대
일관성은 해당 범위 안에서만 보장됩니다."

- 사용자가 일부만 제공하겠다고 하면 그때만 그것으로 진행한다 (명시적 확인 후).
  이 경우 인물 ID는 샷/씬 단위 지역 스코프이며 에피소드간 일관성은 보장하지
  않는다고 고지한다.
- "전체 시청"은 전편 풀 디컴파일이 아니다. 가벼운 프로브(ffprobe + 5초 간격
  샘플 프레임 + 자막 전체 + 1분 미리보기)로 충분하며, 풀 디컴파일은 출력
  범위에만 수행한다. 비용 때문에 기본값을 흔들지 않는다.

1. **월드 바이블** (`story/world.json`): 장소 목록, 시대·계절·날씨, 장소별 팔레트·조명,
   촬영 문법(평균 샷 길이·핸드헬드 비율 — 로컬 계산 가능), 색보정 톤.
2. **인물 레지스트리** (`story/characters.json`): 전역 ID, 외모 고정점,
   에피소드별 의상 변형, 관계 그래프(누가 누구에게 반말/존댓말), 말버릇.
3. **날짜·시간 지도** (`story/timeline-days.md`): "7일물"처럼 날짜가 뼈대면 EP별 day 매핑.
   밤 끝→아침 시작 같은 에피소드간 연속성을 여기서 확정한다.
4. **에피소드 아크표**: 콜드오픈, 서브플롯 출현 여부, 클리프행어 위치.
5. **축·스크린 방향**: 씬별 180도 축, 인물의 입장/퇴장 방향 (M3 + 육안 검수).
6. **소품·의상 연속성**: 씬 경계 컷의 손 소품·컵 위치 등 (비교 영상에서 대조).

이후 씬 바이블의 인물 ID는 전역 레지스트리를 참조하고(S1@씬 = P3),
씬 바이블의 장소·시간은 월드 바이블과 모순되지 않아야 한다.
M3 1회/편 요약(샷틀 + 자막 전체)으로 초안을 만들고 사람이 확정한다.
전용 커맨드가 구현되어 있으므로 아래 명령으로 실행한다:

```bash
# 시리즈 전체 선행 분석 (가벼운 프로브: ffprobe + 5초 간격 키프레임 + 자막 파싱)
rea-movie series-watch --out-root <root> <episode1.mp4> <episode2.mp4> ...
rea-movie series-watch --out-root <root> --source-dir <root>/sources --offline
rea-movie series-watch --out-root <root> --interval 5.0 --force  # story/ 강제 재생성
```

- `series-watch`는 풀 decompile 없이 에피소드 스캔 → 자막 화자 태그(`WALTER:`, `[S1]`, `(지수)`)
  + 색상 팔레트/루마 분석으로 결정적 오프라인 기본 컨텍스트를 구축한다.
- `MINIMAX_API_KEY`가 있으면 M3 1회 호출로 세계관/캐릭터/타임라인 초안 요약 (실패 시 오프라인 폴백).
- `story/{world.json,characters.json,timeline-days.md}`가 이미 존재하면 `--force`가 아닐 때 스킵.
- MCP 도구 `series_watch`로도 호출 가능 (§5 참조).
- `sources/` 영구 캐시: `<root>/sources/<source_id>/{video.mp4,probe.json,scene_cuts.json,keyframes/,shots_cache/,subtitles/}`
  - `source_id`: YouTube는 `yt-<11자리ID>`, 로컬 파일은 크기+앞/뒤 1MiB FNV-1a 해시 `file-<fp>`.
  - `index.json`에 `(path,size,mtime)` 매핑, `scene_cuts.json`은 구간 병합(`insert`)으로 `--range` 재실행 시 재감지 스킵.
  - `shots_cache/<start>_<end>/`에 `original_clip.mp4`/`original_audio.wav`/`contact_sheet.jpg` 보관, `--force` 아니면 원자적 복사 재사용.

---

## 3. 부분 샷 연출 수정 루프 (Director's Editing Loop)

에이전트나 인간 감독이 특정 샷의 연출을 다듬고자 할 때의 표준 작업 루프입니다:

### 3-1. 씬 먼저 보기 (Scene-First)
수십 초 이어지는 대사를 여러 각도로 나눈 장면은 샷 단위가 아니라 **씬 단위**로 다룬다:
1. `story/scenes/scene_XXXX.json` (씬 바이블) 열기: 장소, 시간대, 인물(S1, S2...) 외모·의상 고정점,
   공간 배치도, 시선 문법(누가 어느 쪽을 보는가), 대사 흐름.
   - 씬은 무음 3초+ / 화면 전환(히스토그램 0.55+) / 45초 캡 기준으로 자동 그룹핑됨.
   - 동일 씬 내 인물 ID는 M3가 전 샷을 통째로 보고 부여하므로 앵글이 바뀌어도 동일 인물로 유지됨.
2. 바이블이 틀렸으면(인물 바뀜, 장소 오판) JSON을 직접 고친 뒤 해당 샷들에
   `rea-movie customize apply --proj-dir <proj>` 또는 `./run.sh <shot> --force`로 프롬프트에 재반영.
   씬 블록(`<!-- rea-movie:scene -->`)은 멱등 교체라 중복 걱정 없음.
3. 샷 프롬프트(`prompt.ko.md` 선두 `## 0. 씬 컨텍스트`)에 씬 블록이 들어있는지 확인.
   없으면 `story/scenes/` JSON 유무와 `shot.toml`의 `scene_id`를 점검.

### 3-2. 샷 단위 수정 루프
1. **스토리보드 및 샷 검토**:
   - `contact_sheet.jpg` (1초 간격 타임스탬프 번인 1xN 스트립 — 사람 리뷰·M3 비전 입력용. H3 렌더 입력이 아님)
   - `shots/shot_0005/refs/first_frame.jpg`, `last_frame.jpg` (H3 `fl2va`의 실제 입력)
   - `shots/shot_0005/shot.toml` (앵커 모드: `first_and_last`, `first_only`, `ref_only`, `none`)
2. **한국어 연출 지시서 편집**:
   `shots/shot_0005/prompt.ko.md` 파일을 열고 카메라 워크, 피사체 감정선, 조명, 사운드 연출을 수정합니다.
3. **프롬프트 재컴파일 및 렌더링**:
   ```bash
   # 한국어 연출서로부터 prompt.h3.txt를 재컴파일하고 즉시 샷 0005 재렌더링
   ./run.sh 0005 --force
   ```
4. **결과 비교 및 확인**:
   - `outputs/smoke/<movie_name>_compare.mp4`를 재생하여 원본과의 싱크 및 연출 뉘앙스를 확인합니다.
   - 지시→결과 학습용 4점 세트: `prompt.ko.md` + `prompt.h3.txt` + `renders/smoke/*.mp4` + `refs/first_frame.jpg`.

### 앵커별 H3 요청 형태 (디버깅용)
최저 레벨 호출은 `POST {endpoint}/v1/videos/sync` (multipart)이며 앵커에 따라 달라집니다:
- `first_and_last` → `task=fl2va`, `frame_indices=[0,-1]`, 파트 `input_references` × 2 (first/last)
- `first_only` → `task=fl2va`, `frame_indices=[0]`, 파트 `input_reference` × 1 (단수 주의)
- `ref_only` → `task=ref2va`, `frame_indices` 없음, `face.jpg` + `voice.wav` (현재 1인분만 전송, 다중인물 미지원)
- `none` → `task=t2va`, 이미지 없음, `aspect_ratio=16:9` 추가 (일관성 가장 낮음)

### 3-3. M3 프롬프트 파이프라인 (바이블·앵커 주입)
- M3 샷 프롬프트 생성 시 `contact_sheet.jpg`뿐 아니라 `first_frame.jpg`(`<Picture 1>`)와
  `last_frame.jpg`(`<Picture 2>`)를 멀티모달로 함께 전달하고, `<Picture 1>` 초기 구도 → 중간 동작 →
  `<Picture 2>` 최종 구도의 델타 궤적(Delta trajectory) 묘사를 지시한다.
- 씬 바이블(장소·시간대·S1/S2 외모·의상·공간 배치·시선 문법·대사 흐름)과
  시리즈 컨텍스트(`<root>/story/world.json` 요약 ≤1200자)를 user/system 메시지에 포함시켜
  샷 본문이 씬 바이블 인물 ID·시선 방향과 100% 일관되게 작성되도록 한다.
- 온라인 M3의 `prompt_h3` 선두에는 앵커 종류에 맞는 `reference_line` 헤더를 정규화한다.
- 방송국 로고·워터마크·번인 타임스탬프·하드섭 자막 같은 비주얼 노이즈 묘사는 Negative rule로 명시 금지한다.
- H3 전송 직전 `<!-- rea-movie:... -->` 마커 라인을 제거한 깨끗한 프롬프트만 전송한다
  (디스크의 `prompt.h3.txt`는 멱등성 유지를 위해 마커 유지).
- 오프라인 폴백 컴파일러(`compile_ko_to_h3`) 출력의 비대사 영역에서 한글을 제거한다
  (`<d>[Language] ...</d>` 태그 내부 대사는 보존, 일반 연출 서술문은 영문 구조화 디렉션으로 대체).

### 3-4. 프로 연출 문법 (A-B-A / 180도 축 / On-Off 스크린)
- **A-B-A Shot/Reverse-Shot 씬 유지**: 대화 장면의 `A → B → A → B` 교차 편집은 동일 씬으로 유지한다.
  1-hop 거리가 `HIST_SPLIT(0.55)` 이상이어도 2-hop/3-hop lookback 최소 거리가
  `REVERSE_SHOT_MAX_HIST(0.28)` 이하면 동일 공간 내 카메라 앵글 교차로 판정해 분할을 억제한다.
  단, 무음 3초+ 분할과 45초 캡 분할은 항상 존중한다.
- **180도 축 및 카메라 문법**: 씬 바이블의 `axis_of_action`(예: `S1 on screen-left looking right;
  S2 on screen-right looking left`)과 `camera_grammar`(주 샷 사이즈·OTS/리버스샷 구성)를 준수한다.
  KO 씬 블록(`- 180도 축:`, `- 카메라 문법:`)과 H3 씬 블록(`Axis of action.`, `Camera grammar.`)에 주입된다.
- **On-screen vs Off-screen 발화 구분** (`delivery_mode`: `on_screen` | `off_screen` | `reaction` | `silent`):
  - 대사 큐에 `[off]`, `(off-screen)`, `(VO)`, `(V.O.)`, `(내레이션)`, `(마음의 소리)` 표기가 있거나,
    화자 태그(예: `[S2] ...`)가 현재 샷의 화면 등장 캐릭터에 포함되지 않으면 자동으로 Off-screen 판정.
  - Off-screen일 때 화면 속 인물에게 립싱크를 지시하지 말 것:
    KO `- [화면 밖 음성 / 리액션 샷] 화면 속 인물은 입을 다문 채 ...`,
    H3 `remains silent with natural listening reaction and closed lips (no lip movement), while ... speaks off-screen (voice-over)`.
- **원문 대사 보존**: 대사는 기본 verbatim 유지. `--translate-dub` 명시 시에만 레거시 KO 더빙 번역 경로 동작.
  `<d>[Language]</d>` 태그는 `detect_dialogue_language()`로 본문 언어와 항상 일치
  (Hangul→Korean, 히라가나/가타카나→Japanese, 한자→Chinese, ß/äöü→German, é/è/ê→French, 라틴→English).

---

## 4. DGX Spark 서버 풀 관리 (`rea-movie pool`)

첫 사용자는 노드부터 등록한다 (기본 spark1/2는 개발자 환경이므로 그대로 쓰지 말 것):
```bash
# 최초 1회: 1~4대 H3 노드 등록 (대화형; 비대화형은 --nodes 플래그)
rea-movie pool --proj-dir <proj> setup
rea-movie pool --proj-dir <proj> setup --nodes h3node1.local,h3node2.local --no-check

# 서버 풀 노드 상태 및 엔드포인트 포트 조회 (전 노드 unreachable이면 setup 힌트 출력)
rea-movie pool status

# 서버 풀 배포용 런처 스크립트 생성 (~/rea-movie-artifacts 기준)
rea-movie pool generate-scripts
```
- 노드당 1작업으로 고정할 것: `REA_MOVIE_H3_ENDPOINT=http://spark1.local:8000 ./run.sh --smoke`
- 3작업을 2노드에 올리면 경합 → 504/행 발생 실측. 노드당 1개가 안정적.
- 주의: `pool switch` 서브커맨드는 존재하지 않음. 엔진 재시작이 필요하면 §7 참조.

---

## 5. MCP (Model Context Protocol) 도구 레퍼런스

`rea-movie mcp` 표준 입출력 stdio 서버가 다음 8가지 도구를 제공합니다:

| 도구명 | 파라미터 | 설명 |
| :--- | :--- | :--- |
| `decompile_movie` | `input` (필수), `output`, `range`, `max_shot_duration`, `min_shot_duration`, `translate_dub`, `force`, `run`, `smoke`, `offline` | 원본 영상을 역공학하여 샷 프로젝트 생성 (대사 원문 verbatim 기본) |
| `list_shots` | `proj_dir` | 프로젝트 내 모든 샷의 타임코드, 앵커, Prod/Smoke 렌더 완료 상태 조회 |
| `inspect_shot` | `shot_id` (필수), `proj_dir` | 특정 샷의 `shot.toml`, `prompt.ko.md`, `prompt.h3.txt` 전체 내용 조회 |
| `update_shot_prompt` | `shot_id` (필수), `proj_dir`, `prompt_ko`, `prompt_h3`, `anchor`, `recompile` | 샷의 연출서, 프롬프트, 앵커 설정 업데이트 및 자동 재컴파일 |
| `render_shot` | `proj_dir`, `shot`, `range`, `smoke`, `force`, `force_video_only`, `mock_h3` | 특정 샷/범위 렌더링 실행 |
| `assemble_movie` | `proj_dir`, `smoke`, `proof`, `offline` | 본편 영화(듀얼 오디오, 원본 default) + 좌우 비교 + 6개국어 .srt + 증명 비디오 조립 |
| `pool_status` | `proj_dir` | DGX Spark 풀 노드, 베이스 포트, 활성 엔드포인트 상태 조회 |
| `series_watch` | `inputs`, `source_dir`, `out_root`, `offline`, `force`, `interval` | 시리즈 선행 분석 → `<root>/story/{world.json,characters.json,timeline-days.md}` 생성 |

---

## 6. 에이전트 문제 해결 및 검증 체크리스트

- **ffprobe/ffmpeg 경로 문제**: `assemble` 시 절대 경로(`canonicalize`)를 사용하여 상대 경로 concat 오류를 방지합니다.
- **폰트 미설치 환경 폴백**: `Shot #XXXX` 라벨 오버레이 시 시스템 폰트가 없거나 `drawtext` 필터 에러가 나면 자동으로 라벨 없는 `hstack` 필터로 안전하게 폴백합니다.
- **오디오 트랙 전환**: VLC, IINA, QuickTime 등 미디어 플레이어에서 오디오 트랙 1(H3)과 트랙 2(Original)를 전환하며 사운드 싱크를 검증할 수 있습니다.
- **대사 반복 증상**: 조립본에서 같은 대사가 반복되면 decompile이 overlap 매칭 버전인지 확인. 현행은 정확히-한-번 할당(`assign_cues_to_spans`) + 회귀테스트 존재.
- **무음샷 `<d>[English]` 태그**: 무음샷 h3 대사가 영어 하드코딩인 기지가 있음. 한국어 dub 프로젝트에서 발견 시 수정 대상.
  현행(Issue 11~) 무음샷은 `dub_lang` 무관 동일 무음 지시문을 반환하므로, 구 프로젝트 잔재인지 확인.
- **M3 환각**: 연출서에 원본에 없는 요소(자막 문구 등)가 묘사될 수 있음. compare 영상으로 대조 검증.
- **`<d>[Language]` 불일치**: `<d>` 태그 언어가 본문과 다르면 구버전 산출물. `sync-prompt --force`로 재생성.
- **4초 미만 샷 끝 앵커 소실**: 조립 시 렌더 클립을 단순 `-t` 절단하면 `last_frame` 수렴 구간이 잘려나감.
  현행은 전 구간 리타이밍(`retime_render_clip`)이 기본이므로, 구 조립본이면 `--force`로 재조립.
- **A-B-A 과분할**: 대화 장면이 샷마다 별도 씬으로 쪼개지면 구버전 산출물. 2-hop/3-hop lookback 적용 버전으로 재decompile.
- **Off-screen 립싱크 파괴**: 리액션 컷의 화면 속 인물이 남의 대사를 립싱크하면 구버전 산출물. `delivery_mode` 추론 버전으로 재생성.

---

## 7. H3/M3 장애 운영 런북 (실측 기반)

### 소요 시간 기준 (smoke, 608x352, 10 steps)
- decompile 온라인(M3 연출서+번역): 샷당 약 2분
- H3 렌더: 샷당 약 2~7분 (2분짜리 60샷 ≈ 3~4시간/노드)
- 계획 수립 시 위 기준으로 ETA 산정할 것.

### H3 서버 행(hang) 판별과 복구
- 증상: `/health` 200이지만 렌더 요청이 15분+ 무응답. `nvidia-smi` GPU 사용률 0%면 워커 wedged 확정.
- 진행 중 작업이 없을 때만: `ssh sparkX.local 'docker restart h3-fl2va'` (동일 설정 재시작, 안전).
- 재시작 후 모델 리로드에 10~15분 소요. `/health` 200 복귀 후 단발샷 프로브(`render --shot 0019 --smoke`)로 확인하고 본 작업 재개.
- 주의: 행 요청의 클라이언트를 kill해도 서버 측 작업이 남아 있을 수 있음. 동일 노드 재투입 전 프로브 필수.
- 서버 측 7200초 타임아웃(504 Gateway Timeout)도 관측됨. 3중첩 경합이 유발하므로 노드당 1작업 원칙 준수.

### M3 API 과부하 (529) 대응
- 코드에 재시도 내장: per-shot 연출 3회, per-shot 번역 3회(600s 타임아웃, 30줄 청크).
- 클러스터 전체 529가 1시간+ 지속된 실측 있음. 그 경우 온라인 decompile 강행 시 템플릿 폴백 양산 → 대기 후 재개가 정답.
- 회복 판별: `POST /v1/chat/completions`에 `"Reply with exactly: OK"` probe.

### 중지/재개 절차
- 중지: `pkill -f '[r]un\.sh --smoke'` 후 `pkill -f '[r]ea-movie render'` (대괄호 트릭: pkill 자기매칭 방지). 잔여 프로세스 `ps`로 확인.
- 재개: 완료 샷은 Skip-by-default로 자동 스킵되므로 `./run.sh --smoke` 재실행만으로 이어짐. 손실은 진행 중이던 1샷뿐.
- 부분 실패 프로젝트(v2 등) 재생성 시: 중단된 decompile의 잔재 디렉토리는 직접 삭제 후 재실행 (bases 보존 로직 때문에 `--force`로도 프롬프트가 재생성되지 않음).

---

## 8. 프로페셔널 영화/드라마 연출가 & MiniMax-H3 전문가 검수 플레이북

`"<url/mp4>를 역공학하여 <dir>에 저장해"` 같은 1줄 요청을 받으면 에이전트가 자율 수행하는 5단계 프로토콜이다.
판단·협상·품질 게이트는 에이전트가, 반복 실행은 바이너리가 맡는다 (§2 역할 분담 준수).

### 8-0. 1줄 요청 5단계 자율 프로토콜

```
① sources/ 캐시 확인 & series-watch → ② 씬 바이블 검수 → ③ 델타·발화 검수 →
④ Smoke 렌더 & 조립 → ⑤ _compare.mp4 + 4점 세트 대조 검수
```

**① `sources/` 캐시 확인 & `series-watch` 선행 분석.**
URL이면 `./scripts/reverse-url.sh "<url>" <root> [--episodes ...]`로 `sources/yt-<id>/` 캐시를 확보한다
(이미 있으면 다운로드 스킵). 로컬 파일이면 decompile이 `sources/file-<fp>/`에 자동 캐시한다.
플레이리스트·시리즈물은 decompile 전에 반드시
`rea-movie series-watch --out-root <root>`로 `<root>/story/{world.json,characters.json,timeline-days.md}`를 확보한다
(이미 있으면 스킵). 사용자가 일부만 제공하겠다고 명시 확인한 경우에만 그 범위로 진행하며,
그 경우 인물 ID는 샷/씬 단위 지역 스코프이고 에피소드간 일관성을 보장하지 않는다고 고지한다 (§3-0).

**② 씬 바이블 `axis_of_action` / `eyeline` / A-B-A 리버스샷 검수.**
`story/scenes/scene_XXXX.json`을 열어 장소·시간대·인물 외모·의상 고정점, 공간 배치도,
180도 축(`axis_of_action`: S1 좌/우, 시선 방향), 시선 문법(누가 어느 쪽을 보는가), A-B-A 리버스샷 구성이
맞는지 육안+M3로 검수한다. 바이블이 틀렸으면(인물 바뀜, 장소 오판, 축 반전) JSON을 직접 고친 뒤
`rea-movie customize apply --proj-dir <proj>` 또는 `./run.sh <shot> --force`로 프롬프트에 재반영한다
(씬 블록 `<!-- rea-movie:scene -->`은 멱등 교체라 중복 걱정 없음).
대화 장면이 샷마다 별도 씬으로 쪼개졌으면 구버전 산출물이므로 lookback 적용 버전으로 재decompile한다 (§3-4).

**③ `<Picture 1> → <Picture 2>` 델타 및 On/Off-screen 발화 검수.**
각 샷의 `prompt.ko.md`가 `<Picture 1>` 초기 구도 → 중간 동작 → `<Picture 2>` 최종 구도의
델타 궤적을 정확히 묘사하는지, 화면 속 인물이 직접 말하는지(`on_screen`: `synchronized lip movement`) vs
상대방 말을 듣는 리액션/보이스오버인지(`off_screen`/`reaction`: `no lip movement` + `off-screen (voice-over)`)를
구분 묘사하는지 검수한다. 리액션 배우에게 남의 대사를 립싱크시키면 연출 파괴이므로 즉시 수정한다.
`prompt.h3.txt` 선두의 앵커별 `reference_line` 헤더가 앵커 모드와 일치하는지, HTML 주석 마커
(`<!-- rea-movie:... -->`)가 전송 프롬프트에 섞이지 않았는지(디스크 파일은 유지), 비대사 영역에
한글이나 워터마크/타임스탬프/하드섭 묘사가 없는지 확인한다 (§3-3, §3-4).

**④ Smoke 렌더 & 조립 (6개국어 `.srt` + 원본 오디오 기본).**
`./run.sh --smoke`로 고속 검증 후 본편 조립한다. 조립 산출물 체크리스트:
`outputs/smoke/<movie>_smoke.mp4` (Track 1 H3 + Track 2 원본, **default = 원본**),
`outputs/smoke/<movie>_compare.mp4` (좌 원본 | 우 H3 + `Shot #XXXX` 라벨),
`<movie>_smoke.{ko,zh,ja,en,fr,de}.srt` + `<movie>_smoke.proof.srt` (큐 개수·타임코드 일치),
`--proof` 시 `<movie>_smoke_proof.mp4` (화자 태그 자막 번인).
GPU 풀 없이 검증할 때는 `rea-movie render --smoke --mock-h3`를 사용한다.
4.0초 미만 샷은 전 구간 리타이밍으로 조립되는지(끝 앵커 보존) 확인한다 (§2-C).

**⑤ `_compare.mp4` 및 4점 세트로 지시→결과 대조 검수.**
`outputs/smoke/<movie>_compare.mp4`를 재생하여 원본과의 싱크·연출 뉘앙스를 확인한다.
샷별 지시→결과 학습용 4점 세트: `prompt.ko.md` + `prompt.h3.txt` + `renders/smoke/*.mp4` + `refs/first_frame.jpg`.
대사 불일치(원문 변형·`<d>` 태그 언어 불일치), A-B-A 축 반전, Off-screen 립싱크, 끝 앵커 소실 중
하나라도 발견되면 해당 샷만 `./run.sh <shot> --force`로 재생성하고 ④–⑤를 반복한다.

### 8-1. 앵커 제거 실험 (Ablation) 수행 방법

앵커 기여도를 측정하거나 앵커 문제를 격리할 때 `shot.toml`의 `anchor`를 단계적으로 낮추며 비교한다:

```bash
# shot.toml: anchor = "first_and_last" → "first_only" → "ref_only" → "none"
./run.sh 0005 --force   # 앵커 변경 후 샷 재생성 (프롬프트 재컴파일 + 재렌더)
```

- `first_and_last` → `fl2va` + `frame_indices=[0,-1]` + `input_references` × 2 (양끝 구속, 일관성 최상).
- `first_only` → `fl2va` + `frame_indices=[0]` + `input_reference` × 1 (시작 구속만, 끝은 자유 생성).
- `ref_only` → `ref2va` (얼굴·보이스 레퍼런스만, 구도 자유. 현재 1인분만 전송).
- `none` → `t2va` + `aspect_ratio=16:9` (이미지 없음, 일관성 가장 낮음 — 순수 프롬프트 능력 측정).
- 각 단계의 compare 영상을 나란히 보고 앵커 제거 시 깨지는 요소(인물 동일성 → 배경 → 동작)를 기록한다.
- H3 요청 형태 디버깅은 §3-2 하단 "앵커별 H3 요청 형태" 참조.
