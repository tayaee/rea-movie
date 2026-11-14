# AGENTS.md — `rea-movie` AI 에이전트 행동 지침 및 엔지니어링 헌장

> **필독**: 이 리포지토리에서 작업하는 모든 AI 에이전트(Orchestrator, Coder, Reviewer)는 본 문서와 [`README.md`](README.md)의 설계 명세를 철저히 준수해야 합니다.

---

## 1. 프로젝트 정체성 및 핵심 목표

- **리포지토리명**: `reverse-engineer-any-movie`
- **바이너리 및 CLI 명칭**: `rea-movie` (Rust 구현)
- **에이전트 스킬 명칭**: `/rea-movie` (`skills/rea-movie/SKILL.md`)
- **핵심 목표**:
  1. `mkv` / `mp4` 원본 영상을 입력받아 샷 분할, 타임스탬프 번인 스토리보드(`contact_sheet.jpg`), 대사/보이스 지문, 한국어 연출서(`prompt.ko.md`), MiniMax-H3 전용 영문 프롬프트(`prompt.h3.txt`)를 포함한 역공학 프로젝트를 생성한다.
  2. DGX Spark 서버 풀(`spark1.local`, `spark2.local`, 공유 NAS 워크스페이스 `/rosenas/data/rea-movie-workspace`, 동적 포트 `8000`+)을 통해 `MiniMax-H3`(`fl2va` 기본, `ref2va`, `t2va`)로 각 샷을 렌더링하고, 본편 영상 및 좌우 1:1 비교 영상(`_compare.mp4`)을 조립한다.

---

## 2. 필수 엔지니어링 규칙

1. **보안 및 API 키 관리**:
   - 소스 코드, 스크립트, 설정 파일, 테스트 코드 어디에도 실제 API 키를 하드코딩하지 마십시오.
   - MiniMax-M3 비전/프롬프트 API 호출 시 반드시 환경변수 `MINIMAX_API_KEY` (및 기본 엔드포인트 `https://api.minimax.io/v1/chat/completions`, 모델 `MiniMax-M3`)를 사용하십시오.
2. **샷 분할 및 프레임 앵커 규칙**:
   - 최대 샷 길이 기본값은 **`8.0`초**입니다 (DGX Spark 128GB OOM 방지).
   - 2개의 서로 다른 카메라 컷을 하나의 `fl2va` 샷으로 묶지 마십시오 (모핑 방지).
   - `first_frame.jpg`는 컷 시작 `+2프레임`(또는 `+0.08s`), `last_frame.jpg`는 컷 끝 `-2프레임`(또는 `-0.08s`)에서 추출하십시오.
3. **Prod vs Smoke 디렉토리 완전 격리 & Skip-by-default**:
   - 기본 실행(`./run.sh`, `rea-movie render`)은 **Prod 모드**(`960x576`, `steps=50`, `fps=24`)이며 `shots/shot_XXXX/renders/prod/`와 `outputs/prod/`를 사용합니다.
   - 스모크 실행(`./run.sh --smoke`, `rea-movie render --smoke`)은 **Smoke 모드**(`608x352`, `steps=10`, `fps=24`)이며 `shots/shot_XXXX/renders/smoke/`와 `outputs/smoke/`를 사용합니다.
   - 타겟 파일이 이미 존재하고 유효하면 기본적으로 건너뛰며(Skip), `--force` 옵션이 주어졌을 때만 기존 파일을 `history/take_NNN.mp4`로 백업하고 재생성합니다.
4. **AutoSDLC 단일 커밋(Single Commit) 규칙**:
   - 오케스트레이터는 `~/.local/bin/ai-coder.sh --mode best` (또는 `~/.local/bin/orchestrator-argon.sh --issue <N> --mode best`)를 통해 코더 에이전트에게 구현을 위임하고, `regression-tests/verify-issue-<N>.sh` 검증 통과 후 이슈 아카이빙(`issues/archive/<YYYY>/<MM>/<DD>/`)과 함께 단일 커밋을 생성합니다.
