---
name: rea-movie
description: Reverse-engineer any movie into reproducible MiniMax-H3 multi-shot video project with DGX Spark pool orchestration
---

# `rea-movie` 스킬 — 영화 역공학 및 MiniMax-H3 다중 샷 렌더링 오케스트레이션

이 문서는 AI 에이전트(Orchestrator, Coder, Reviewer)가 `rea-movie` CLI, 단일 진입점 `./run.sh`, DGX Spark 서버 풀, 그리고 MCP 도구를 효과적으로 활용하여 영화를 역공학하고 고품질 비디오를 재생성 및 검증할 수 있도록 돕는 실전 가이드입니다.

---

## 1. 프로젝트 핵심 아키텍처 및 철학

- **목표**: 임의의 영화/영상(`mp4`, `mkv`)을 입력받아 샷 분할, 앵커 프레임(`first_frame.jpg`, `last_frame.jpg`), 타임코드 번인 스토리보드(`contact_sheet.jpg`), 한국어 연출서(`prompt.ko.md`), MiniMax-H3 영문 프롬프트(`prompt.h3.txt`)를 생성하고, DGX Spark 서버 풀에서 각 샷을 렌더링한 후 최종 영화 및 좌우 1:1 동시 비교 영상(`_compare.mp4`)으로 조립한다.
- **핵심 엔지니어링 원칙**:
  1. **최대 샷 길이 8.0초 이하**: DGX Spark 128GB 통합 메모리 OOM 방지 및 생성 일관성 유지.
  2. **모핑 방지**: 2개의 다른 카메라 앵글/컷을 하나의 `fl2va` 샷으로 묶지 않음.
  3. **Prod vs Smoke 완전 격리**:
     - Prod 기본: `960x576`, `steps=50`, `fps=24` → `shots/shot_XXXX/renders/prod/`, `outputs/prod/`
     - Smoke 모드: `608x352`, `steps=10`, `fps=24` → `shots/shot_XXXX/renders/smoke/`, `outputs/smoke/`
  4. **Skip-by-default & Take History 백업**:
     - 타겟 파일이 이미 존재하면 기본적으로 건너뜀(Skip).
     - `--force` 옵션으로 재생성 시 이전 결과물은 `history/take_NNN.mp4`로 안전하게 백업.
  5. **듀얼 오디오 트랙 및 1:1 비교 영상**:
     - 최종 본편(`outputs/<profile>/<movie_name>_<profile>.mp4`): Track 1(H3 오디오) + Track 2(원본 영화 오디오).
     - 비교 영상(`outputs/<profile>/<movie_name>_compare.mp4`): [좌: 원본 | 우: H3 렌더본 + `Shot #XXXX` 라벨].

---

## 2. CLI 기본 명령어 및 실행 흐름

### A. 영화 역공학 (Decompile)
입력 영상을 분석하여 샷별 에셋 및 프롬프트 프로젝트를 생성합니다:
```bash
# 기본 역공학 (MiniMax-M3 비전 API로 한국어 연출서 자동 작성)
rea-movie robocop.mp4 -o ./robocop_proj

# 특정 구간만 역공학 (예: 1분 10초 ~ 2분 30초)
rea-movie robocop.mp4 -o ./robocop_proj --range 00:01:10-00:02:30

# 오프라인/테스트 모드 (비전 API 호출 없이 플레이스홀더 프롬프트 생성)
rea-movie robocop.mp4 -o ./robocop_proj --offline

# 역공학 완료 후 즉시 렌더 및 조립까지 자동 실행
rea-movie robocop.mp4 -o ./robocop_proj --run
```

### B. 단일 진입점 스크립트 (`./run.sh`)
생성된 프로젝트 루트 디렉토리에서 `./run.sh`를 통해 렌더링과 조립을 원스톱으로 수행합니다:
```bash
cd ./robocop_proj

# 1. Prod 기본 렌더링 및 조립 (960x576, 50 steps)
./run.sh

# 2. Smoke 고속 렌더링 및 조립 (608x352, 10 steps)
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
```

---

## 3. 부분 샷 연출 수정 루프 (Director's Editing Loop)

에이전트나 인간 감독이 특정 샷의 연출을 다듬고자 할 때의 표준 작업 루프입니다:

1. **스토리보드 및 샷 검토**:
   - `contact_sheet.jpg` (타임스탬프 번인 4x4 시트)
   - `shots/shot_0005/refs/first_frame.jpg`, `last_frame.jpg`
   - `shots/shot_0005/shot.toml` (앵커 모드: `first_and_last`, `first_frame`, `ref_only`, `text_only` 등)
2. **한국어 연출 지시서 편집**:
   `shots/shot_0005/prompt.ko.md` 파일을 열고 카메라 워크, 피사체 감정선, 조명, 사운드 연출을 수정합니다.
3. **프롬프트 재컴파일 및 렌더링**:
   ```bash
   # 한국어 연출서로부터 prompt.h3.txt를 재컴파일하고 즉시 샷 0005 재렌더링
   ./run.sh 0005 --force
   ```
4. **결과 비교 및 확인**:
   - `outputs/prod/<movie_name>_compare.mp4`를 재생하여 원본과의 싱크 및 연출 뉘앙스를 확인합니다.

---

## 4. DGX Spark 서버 풀 관리 (`rea-movie pool`)

MiniMax-H3 모델 서빙 파티션(`fl2va` 기본, `ref2va`)을 제어합니다:
```bash
# 서버 풀 노드 상태 및 엔드포인트 포트 조회
rea-movie pool status

# 서버 풀 배포용 systemd 런처 스크립트 생성
rea-movie pool generate-scripts

# 엔진 모드 전환 요청 (fl2va <-> ref2va)
rea-movie pool switch --engine ref2va
```

---

## 5. MCP (Model Context Protocol) 도구 레퍼런스

`rea-movie mcp` 표준 입출력 stdio 서버가 다음 7가지 도구를 제공합니다:

| 도구명 | 파라미터 | 설명 |
| :--- | :--- | :--- |
| `decompile_movie` | `input` (필수), `output`, `range`, `max_shot_duration`, `force`, `run`, `smoke`, `offline` | 원본 영상을 역공학하여 샷 프로젝트 생성 |
| `list_shots` | `proj_dir` | 프로젝트 내 모든 샷의 타임코드, 앵커, Prod/Smoke 렌더 완료 상태 조회 |
| `inspect_shot` | `shot_id` (필수), `proj_dir` | 특정 샷의 `shot.toml`, `prompt.ko.md`, `prompt.h3.txt` 전체 내용 조회 |
| `update_shot_prompt` | `shot_id` (필수), `proj_dir`, `prompt_ko`, `prompt_h3`, `anchor`, `recompile` | 샷의 연출서, 프롬프트, 앵커 설정 업데이트 및 자동 재컴파일 |
| `render_shot` | `proj_dir`, `shot`, `range`, `smoke`, `force`, `force_video_only`, `mock_h3` | 특정 샷/범위 렌더링 실행 |
| `assemble_movie` | `proj_dir`, `smoke` | 본편 영화(듀얼 오디오 트랙) 및 좌우 비교 영상 조립 |
| `pool_status` | `proj_dir` | DGX Spark 풀 노드, 베이스 포트, 활성 엔드포인트 상태 조회 |

---

## 6. 에이전트 문제 해결 및 검증 체크리스트

- **ffprobe/ffmpeg 경로 문제**: `assemble` 시 절대 경로(`canonicalize`)를 사용하여 상대 경로 concat 오류를 방지합니다.
- **폰트 미설치 환경 폴백**: `Shot #XXXX` 라벨 오버레이 시 시스템 폰트가 없거나 `drawtext` 필터 에러가 나면 자동으로 라벨 없는 `hstack` 필터로 안전하게 폴백합니다.
- **오디오 트랙 전환**: VLC, IINA, QuickTime 등 미디어 플레이어에서 오디오 트랙 1(H3)과 트랙 2(Original)를 전환하며 사운드 싱크를 검증할 수 있습니다.
