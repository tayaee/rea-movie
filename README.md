# rea-movie (`reverse-engineer-any-movie`)

**`rea-movie`**는 기존의 영화나 드라마 영상(`mkv` / `mp4`)을 입력받아 **대본, 등장인물 도감, 정밀 샷 타임라인, 한국어 연출 지시서 및 MiniMax-H3 전용 구조화 영문 프롬프트 프로젝트**로 역공학(Reverse-Engineer)하고, **DGX Spark (GB10) 서버 풀**에서 **MiniMax-H3 (`fl2va` / `ref2va` / `t2va`)**를 구동해 원본 영화를 그대로 재현하거나 다중 작업자가 샷별로 분담 편집·재생성할 수 있게 해주는 **Rust 기반 CLI + MCP 서버 + `/rea-movie` 에이전트 스킬**입니다.

---

## 🎯 핵심 설계 철학 (5대 원칙)

1. **1차 목표는 "보자마자 감탄이 나오는 원본 1:1 재현" (Strict Single-Cut `fl2va` 기본)**
   - 두 개의 카메라 컷을 하나의 클립으로 묶어 모핑(Morphing)이 일어나는 것을 원천 차단합니다.
   - 원본 영화의 실제 카메라 하드 컷(Hard Cut) 단위로 정확히 끊되, 8초를 초과하는 롱테이크만 **최대 8.0초**(DGX Spark GB10 128GB OOM 방지 상한) 단위로 분할하고 앞 조각의 끝 프레임과 뒤 조각의 시작 프레임을 체이닝합니다.
   - 컷 전환 직후/직전 블렌딩 잔상을 피하기 위해 **컷 시작 `+2프레임`(`first_frame.jpg`)과 컷 끝 `-2프레임`(`last_frame.jpg`)**을 추출하여 `fl2va`의 양 끝 앵커(`frame_indices=[0, -1]`)로 고정합니다.
2. **타임스탬프 번인(Burn-in) 스토리보드 기반 초 단위 동작 궤적 프롬프트**
   - 1초 간격 스냅샷 좌상단에 `[0.0s]`, `[1.0s]`, `[2.0s]` 타임스탬프를 시각적으로 번인한 `contact_sheet.jpg`를 생성하여 **MiniMax-M3**(`$MINIMAX_API_KEY`) 비전 모델에 전달합니다.
   - 시작 프레임과 끝 프레임 사이의 **초 단위 동작·카메라·표정 변화 경로(Observable Path)**를 정밀 서술하여 중간 구간에서 동작이 멈추거나 마지막에 급발진하는 현상을 방지합니다.
3. **화자 보이스 지문(Voice Fingerprint) 고정 & 오디오 피크 타임코드 자동 주입**
   - 등장인물별 성별·연령대·음역대·말투 질감을 담은 보이스 지문을 추출해 모든 대사 프롬프트(`<d>[Language] ...</d>`)에 일관되게 주입합니다.
   - 원본 샷 오디오의 데시벨 피크(총성, 폭발음, 문 닫힘 등) 시각을 검출해 프롬프트에 초 단위로 명시합니다.
4. **원본 vs 재현본 1:1 동시 비교 영상 (`_compare.mp4`) 및 듀얼 오디오 트랙 자동 생성**
   - 최종 병합 시 본편 영상뿐만 아니라 **[좌측: 원본 영화 | 우측: H3 재현 영화 + `Shot #XXXX` 오버레이]**가 1프레임 오차 없이 나란히 재생되는 비교 영상을 자동으로 생성합니다.
   - 본편 영상에는 **Track 1 (H3 생성 스테레오 오디오)**과 **Track 2 (원본 영화 오디오)**가 멀티트랙으로 함께 수록됩니다.
5. **Prod 기본 실행 + Smoke 디렉토리 완전 격리 + 멱등성(Skip-by-default) 보장**
   - 아무 옵션 없이 `./run.sh`를 실행하면 **Prod Grade (`960x576`, `50 steps`, `24 fps`)**로 렌더링됩니다.
   - 스토리/연출 체크용 빠른 실행(`./run.sh --smoke`, `608x352`, `10 steps`)은 `renders/smoke/`와 `outputs/smoke/`에 완전 분리 저장되어 Prod 결과물과 절대 섞이지 않습니다.
   - 타겟 파일이 이미 존재하면 기본적으로 건너뛰며(중단 후 재실행 시 즉시 이어하기), 프롬프트 수정 후 재생성할 때만 `--force`를 사용합니다.

---

## 🚀 빠른 시작 및 사용법 (Quick Start)

### 1. 영화 역공학 프로젝트 생성 (`decompile`)

```bash
# [기본] mkv/mp4 파일을 입력하면 ./robocop 폴더에 역공학 프로젝트 생성
rea-movie robocop.mkv

# [구간 지정] 01분 00초 ~ 02분 30초 구간만 역공학 프로젝트 생성
rea-movie robocop.mkv --range 00:01:00-00:02:30 -o ./robocop

# [원스톱 실행] 프로젝트 생성 후 곧바로 Prod 전체 렌더링 + 비교 영상 조립까지 실행
rea-movie robocop.mkv --range 00:01:00-00:02:30 --run
```

### 2. 전체 영화 생성 (Prod 본편 vs Smoke 스토리 체크)

생성된 프로젝트 폴더 안에는 단일 진입점 스크립트 `./run.sh`가 포함되어 있습니다.

```bash
cd ./robocop

# ⭐ [기본: Prod Grade 본편 생성]
# 960x576, 50 steps, 24fps로 미완료 샷들을 DGX Spark 풀(spark1.local, spark2.local)에서 렌더링 후
# outputs/prod/robocop_prod.mp4 및 outputs/prod/robocop_compare.mp4 생성
./run.sh

# ⚡ [스토리 체크: Smoke 저해상도 프리뷰 생성]
# 608x352, 10 steps로 빠르게 전체 샷을 렌더링하여 outputs/smoke/ 에 별도 저장
./run.sh --smoke
```

### 3. 부분 샷 프롬프트 편집 및 재생성 (다중 작업자 협업 예시)

여러 사람이 시간대/샷 번호를 분담하여(예: 작업자 A는 `0001~0005`, 작업자 B는 `0006~0010`) 교정할 때, 각 샷은 `shots/shot_XXXX/` 아래에 완전히 독립된 파일로 존재합니다.

예를 들어 **`0005`번 샷**의 연출이나 대사를 수정하고 다시 생성하는 전체 과정은 다음과 같습니다:

```bash
cd ./robocop

# 1) 0005번 샷의 한국어 연출 지시서 편집 (가장 직관적인 편집 방법)
vi shots/shot_0005/prompt.ko.md

# (선택 A) 만약 MiniMax-H3 영문 구조화 프롬프트를 직접 세밀하게 손보고 싶다면:
vi shots/shot_0005/prompt.h3.txt

# (선택 B) 프레임 구속력(anchor = "first_and_last" | "first_only" | "ref_only" | "none")이나
#          seed, duration 등을 조정하고 싶다면:
vi shots/shot_0005/shot.toml

# 2) 수정한 프롬프트가 잘 먹히는지 2분 만에 Smoke(저해상도, 10 steps)로 먼저 테스트하고 싶을 때
#    (결과물: shots/shot_0005/renders/smoke/shot_0005.mp4)
./run.sh 0005 --smoke --force

# 3) 마음에 들면 0005번 샷을 Prod Grade(960x576, 50 steps)로 강제 재생성 및 전체 영화 재조립!
#    - 기존 renders/prod/shot_0005.mp4 는 renders/prod/history/take_001.mp4 로 자동 백업됨
#    - 수정된 prompt.ko.md 를 바탕으로 prompt.h3.txt 를 갱신한 뒤 렌더링 & outputs/prod/ 재조립
./run.sh 0005 --force

# 4) 영문 프롬프트(shots/shot_0005/prompt.h3.txt)를 직접 수정해서 한->영 재컴파일 없이 영상만 다시 찍을 때
./run.sh 0005 --force-video-only

# 5) 담당 구간(예: 0001번 ~ 0005번 샷) 전체를 강제 재생성할 때
./run.sh 0001-0005 --force
```

---

## 📁 생성되는 프로젝트 디렉토리 레이아웃

```text
robocop/
├── rea-movie.toml                  # 전역 설정 (DGX Spark 서버 풀: spark1.local, spark2.local / 최대 샷 길이 8.0s)
├── timeline.json                   # 전체 샷 타임코드, 길이, 모드, 담당자 매니페스트
├── run.sh                          # 🚀 단일 통합 실행 스크립트 (Prod 기본 / --smoke / 특정 샷 재생성 / 조립)
│
├── story/
│   ├── transcript.srt              # 추출된 전체 대사/타임코드
│   ├── screenplay.md               # 역추출된 전체 대본 및 씬 요약
│   └── characters.toml             # 등장인물 도감 (S1, S2 화자 ID, 외형 묘사, 보이스 지문, 레퍼런스 경로)
│
├── assets/
│   └── characters/
│       ├── S1_murphy/
│       │   ├── face.jpg            # Ref2VA용 캐릭터 대표 얼굴 (<Picture 1>)
│       │   └── voice.wav           # Ref2VA용 캐릭터 대표 음성 (<Audio 1>)
│       └── S2_lewis/ ...
│
├── shots/
│   ├── shot_0001/
│   │   ├── shot.toml               # 샷 메타데이터 (anchor="first_and_last", duration, seed, speaker 등)
│   │   ├── prompt.ko.md            # ✍️ [사람이 편집] 한국어 연출/카메라/초 단위 동선/대사/사운드 설명
│   │   ├── prompt.h3.txt           # 🤖 [H3 입력] integrated_multimodal_description / overall_soundscape / non_diegetic_music
│   │   ├── refs/
│   │   │   ├── original_clip.mp4   # 비교용 원본 샷 영상 클립
│   │   │   ├── original_audio.wav  # 원본 샷 오디오 및 피크 분석 소스
│   │   │   ├── contact_sheet.jpg   # [0.0s], [1.0s]... 타임스탬프 번인 1초 스냅샷 스토리보드
│   │   │   ├── first_frame.jpg     # fl2va 시작 프레임 (컷 시작 +2프레임)
│   │   │   └── last_frame.jpg      # fl2va 끝 프레임 (컷 끝 -2프레임)
│   │   └── renders/
│   │       ├── prod/               # ⭐ 본편(960x576, 50 steps) 결과물 (기본 타겟)
│   │       │   ├── shot_0001.mp4
│   │       │   └── history/
│   │       │       └── take_001.mp4
│   │       └── smoke/              # ⚡ 스토리 체크(608x352, 10 steps) 결과물 (완전 격리)
│   │           └── shot_0001.mp4
│   │
│   ├── shot_0002/ ...
│   └── shot_0005/ ...
│
└── outputs/
    ├── prod/
    │   ├── robocop_prod.mp4        # ⭐ 최종 본편 (Track 1: H3 스테레오 오디오, Track 2: 원본 오디오)
    │   └── robocop_compare.mp4     # 🎬 좌(원본) vs 우(H3 재현본 + Shot 번호 오버레이) 1:1 동시 비교 영상
    └── smoke/
        ├── robocop_smoke.mp4       # ⚡ 스토리 체크용 저해상도 본편
        └── robocop_compare.mp4     # ⚡ 스토리 체크용 좌우 비교 영상
```

---

## 🖥️ DGX Spark (GB10) 서버 풀 및 MiniMax-H3 구동 아키텍처

* **기본 노드 풀**: `spark1.local`, `spark2.local` (`rea-movie.toml`에서 1대 또는 N대로 자유롭게 설정 가능)
* **원격 워크스페이스**: 각 호스트(제어 노드 및 H3 노드)의 `~/rea-movie-workspace/` 디렉토리에 H3 시작 스크립트(`serve-h3.sh`), `node-state.json`, 로그 등을 격리하여 관리합니다 (리포지토리 이름 `rea-movie`와의 경로 충돌 방지).
* **동적 포트 탐색**: 기본 포트 `8000`을 확인하되, 통합 메모리 여유량(`MemAvailable`)이 충분한데 `8000`번 포트가 점유되어 있으면 자동으로 빈 포트(`8001`, `8002`...)를 찾아 컨테이너를 바인딩합니다.
* **모드별 배칭 (Single-Tenant 보호)**:
  * DGX Spark 1대에는 `h3-fl2va`(`fl2va` 및 `t2va` 동시 지원) 또는 `h3-ref2va` 중 하나만 상주합니다.
  * `rea-movie` 스케줄러는 먼저 `fl2va`/`t2va` 샷들을 전부 렌더링한 뒤, `ref2va` 샷이 존재할 때만 마지막에 1회 엔진을 전환합니다.
  * GB10 통합 메모리 규칙에 따라 CPU 오프로드를 비활성화하고 `--quantization fp8 --vae-parallel-mode tile --vae-use-tiling --enforce-eager --diffusion-attention-backend CUDNN_ATTN` 레시피를 사용합니다.
