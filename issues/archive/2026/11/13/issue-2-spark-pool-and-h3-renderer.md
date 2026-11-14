# Issue 2: DGX Spark 동적 서버 풀 매니저 및 Prod/Smoke 격리 렌더링 엔진 구현

## 1. 배경 및 목표
DGX Spark (GB10, 128GB 통합 메모리) 노드 풀(`spark1.local`, `spark2.local`)의 공유 NAS 워크스페이스(`/rosenas/data/rea-movie-workspace`)와 동적 포트(`8000`+)를 관리하고, `fl2va`/`t2va` 및 `ref2va` 모드별 배칭 스케줄링으로 `MiniMax-H3` 렌더링을 수행하는 `pool` 및 `render` 모듈을 구현한다. 또한 `prod`(기본)와 `smoke` 출력 디렉토리 완전 격리, 타겟 파일 존재 시 Skip-by-default, `--force` 시 히스토리 백업 및 프롬프트 재컴파일 기능을 구현한다.

## 2. 상세 요구사항

1. **DGX Spark 동적 서버 풀 매니저 (`src/pool.rs`)**:
   - `rea-movie.toml`에 정의된 노드 풀(`spark1.local`, `spark2.local`) 관리.
   - 각 호스트(및 필요 시 제어 노드)의 `~/rea-movie-workspace/` 아래에 `node-state.json` 및 `serve-h3.sh` (H3 시작 스크립트) 생성/배포 지원 (리포지토리 폴더명 `rea-movie`와의 충돌 방지).
   - 동적 포트 탐색: 기본 포트 `8000`이 점유되어 있으나 통합 메모리 여유(`MemAvailable`)가 충분할 경우 `8001`, `8002`... 등 빈 포트를 찾아 컨테이너 바인딩.
   - GB10 공식 레시피 준수:
     - 이미지: `vllm/vllm-omni:minimax-h3`
     - 마운트: `-v ~/.cache/huggingface:/root/.cache/huggingface -v $HOME/models/MiniMax-H3:/model:ro -v $HOME/tools/vllm-omni:/opt/vllm-omni:ro`
     - 필수 옵션: `--omni --trust-remote-code --num-gpus 1 --tensor-parallel-size 1 --text-encoder-tp-size 1 --usp 1 --ring 1 --vae-patch-parallel-size 1 --vae-parallel-mode tile --vae-use-tiling --quantization fp8 --enforce-eager --diffusion-attention-backend CUDNN_ATTN` (CPU offload 절대 사용 금지)
   - 모드별 배칭(Mode-Grouped Batching):
     - `fl2va` 파티션은 `fl2va`와 `t2va`를 모두 처리하므로, 작업 큐에서 `fl2va`/`t2va` 샷들을 먼저 일괄 실행한 뒤 `ref2va` 샷이 있을 때만 마지막에 1회 엔진을 전환한다.
   - `rea-movie pool status` 및 `rea-movie pool generate-scripts` 서브커맨드 지원.

2. **H3 렌더링 클라이언트 및 Prod/Smoke 격리 (`src/render.rs`)**:
   - 기본 실행(`rea-movie render` 또는 `./run.sh`)은 **Prod 프로필**(`960x576`, `steps=50`, `fps=24`)을 사용하며 결과를 `shots/shot_XXXX/renders/prod/shot_XXXX.mp4`에 저장한다.
   - `--smoke` 옵션 실행(`rea-movie render --smoke` 또는 `./run.sh --smoke`)은 **Smoke 프로필**(`608x352`, `steps=10`, `fps=24`)을 사용하며 결과를 `shots/shot_XXXX/renders/smoke/shot_XXXX.mp4`에 완전히 격리하여 저장한다.
   - **Skip-by-default (멱등성)**:
     - 해당 프로필의 타겟 MP4 파일이 이미 존재하고 크기가 0보다 크면 `--force`나 `--force-video-only`가 없는 한 건너뛴다.
     - 즉, Smoke 렌더링 결과가 존재하더라도 Prod 렌더링은 스킵되지 않으며, 반대도 마찬가지다.
   - **`--force` 및 `--force-video-only` 동작**:
     - 기존에 타겟 MP4 파일이 존재하면 삭제하지 않고 `renders/<profile>/history/take_001.mp4` (순차 증가)로 백업한다.
     - `--force` 시: `prompt.ko.md`를 읽어 `prompt.h3.txt`를 먼저 갱신(컴파일)한 후 비디오를 재생성한다.
     - `--force-video-only` 시: `prompt.h3.txt`를 덮어쓰지 않고(사용자가 직접 수정한 영문 프롬프트 보존) 비디오만 재생성한다.
   - **샷 선택 필터링**:
     - `--shot 0005` 또는 `./run.sh 0005 --force`: 단일 샷 렌더링
     - `--shot 0001-0005` 또는 `./run.sh 0001-0005 --force`: 샷 번호 범위 렌더링
     - `--range 00:00:00-00:00:30`: 시간 구간에 걸치는 샷들만 렌더링
   - **API 호출 및 로컬 Mock/테스트 지원**:
     - 실제 서버 호출 시 `POST http://<host>:<port>/v1/videos/sync` 멀티파트 폼 전송:
       - `anchor = "first_and_last"`: `extra_params={"task":"fl2va","duration":...}` + `input_references`에 `first_frame.jpg`, `last_frame.jpg` 2장 전송 (`frame_indices=[0, -1]`)
       - `anchor = "first_only"`: `extra_params={"task":"fl2va","duration":...}` + `input_reference`에 `first_frame.jpg` 1장 전송 (`frame_indices=[0]`)
       - `anchor = "ref_only"`: `extra_params={"task":"ref2va","duration":...}` + 캐릭터 `face.jpg` / `voice.wav` 전송
       - `anchor = "none"`: `extra_params={"task":"t2va","duration":...}` + `aspect_ratio=16:9` 전송
     - 환경변수 `REA_MOVIE_MOCK_H3=1` (또는 `--mock-h3` 플래그) 지정 시 실제 원격 GPU 서버 없이 로컬 `ffmpeg`로 요청된 해상도(`960x576` 또는 `608x352`), 길이(`duration`), 스테레오 오디오(32kHz)를 가진 유효한 MP4를 즉시 합성하여 전체 파이프라인 및 E2E 테스트가 작동하도록 지원한다.

## 3. 검수 기준
- `cargo test` 통과 및 `./regression-tests/verify-issue-2.sh` 통과.

## 구현 결과

**구현 완료 일시**: 2026-11-13T22:18:48-05:00

### 구현 내용
1. **DGX Spark 서버 풀 매니저 (`src/pool.rs`)**
   - `rea-movie.toml`의 노드/기본 포트/원격 워크스페이스 설정을 읽어 `pool status`, `pool generate-scripts`를 구현했습니다.
   - 제어 노드와 각 DGX Spark 노드가 공유하는 `/rosenas/data/rea-movie-workspace/`에 `serve-h3.sh`, `nodes/<node>/node-state.json`을 생성하고 SSH를 통해 검증/제어합니다.
   - `MemAvailable`을 확인하고 기본 포트가 점유되었으면 `8001`, `8002` 순서로 빈 포트를 선택합니다. 선택된 포트를 `node-state.json`에 기록합니다.
   - `vllm/vllm-omni:minimax-h3` 이미지와 요구된 마운트/FP8/VAE tile/CUDNN 옵션을 모두 포함했으며 CPU offload 옵션은 포함하지 않았습니다.
   - `fl2va`(`fl2va`/`t2va`) 작업 그룹을 먼저 처리하고 `ref2va` 그룹을 마지막에 처리하며, 필요한 경우 노드 엔진 전환을 그룹당 1회 요청합니다. 동일 엔진의 여러 노드는 라운드로빈 배정합니다.

2. **H3 렌더링 클라이언트 및 프로필 격리 (`src/render.rs`)**
   - Prod(`960x576`, 50 steps, 24 fps)와 Smoke(`608x352`, 10 steps, 24 fps)를 각각 `renders/prod/`, `renders/smoke/` 아래 저장하도록 구현했습니다.
   - 0바이트보다 큰 기존 타겟은 기본적으로 건너뛰며, 강제 재생성 시 프로필별 `history/take_NNN.mp4`로 순차 백업합니다.
   - `--force`는 `prompt.ko.md`를 `prompt.h3.txt`로 재컴파일하고, `--force-video-only`는 기존 영문 프롬프트를 변경하지 않습니다.
   - 단일 샷/샷 번호 범위/시간 구간 겹침 필터를 구현했습니다.
   - 실제 H3 서버에는 `/v1/videos/sync` 멀티파트 요청과 앵커별 `extra_params`, 프레임/캐릭터 레퍼런스, `aspect_ratio`를 전송합니다.
   - `REA_MOVIE_MOCK_H3=1` 또는 `--mock-h3`에서는 `ffmpeg`로 요청 해상도/길이의 스테레오 32 kHz MP4를 합성합니다.

3. **프롬프트 컴파일러 (`src/sync_prompt.rs`)**
   - 한국어 연출 지시서를 H3의 `integrated_multimodal_description`, `overall_soundscape`, `non_diegetic_music` 구조로 컴파일하도록 구현하고 `sync-prompt` 명령에도 반영했습니다.

### 변경 파일
- `src/pool.rs`
- `src/render.rs`
- `src/sync_prompt.rs`
- `src/cli.rs`
- `src/main.rs`
- `issues/issue-2-spark-pool-and-h3-renderer.md`
- `issues/issue-2__agent-stats.json`

### 검증 결과
- `cargo test --all-targets`: **13 passed, 0 failed**
- `./regression-tests/verify-issue-2.sh`: **ALL CHECKS PASSED**
- 기존 회귀 `./regression-tests/verify-issue-1.sh`: **ALL CHECKS PASSED**
- 추가 확인: 실제 `/v1/videos/sync` 멀티파트 요청 필드, Smoke MP4 메타데이터(608x352/24 fps/32 kHz stereo), `--force-video-only` 프롬프트 보존, 점유 포트에서의 동적 `8000` → `8001` 이동, 원격 `serve-h3.sh` 배포를 확인했습니다.

**계획 대비 편차**: 없음
