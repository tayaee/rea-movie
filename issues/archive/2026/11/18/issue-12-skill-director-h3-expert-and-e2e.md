# Issue 12: 스킬 프로페셔널 연출가 & MiniMax-H3 전문가 플레이북 완성 및 1줄 요청 E2E 통합

## 1. 개요 및 배경

Issue 8 ~ Issue 11에서 `rea-movie`에 새로 구현된 전문가 기능들
(`sources/` 영구 캐시, `series-watch` 선행 분석, `--min-shot-duration` 미세 컷 병합,
4.0초 미만 샷 앵커 보존 리타이밍, `<Picture 1>/<Picture 2>` 델타 주입 M3 파이프라인,
A-B-A 리버스샷 씬 유지·180도 축·On/Off-screen 발화 구분, 원문 대사 verbatim +
`<d>[Language]</d>` 자동 태그 + 6개국어 외부 자막 + 원본 오디오 default +
`assemble --proof` 증명 비디오)이 바이너리에는 존재하지만, 에이전트가 실제로
읽고 행동하는 **스킬 문서(`skills/rea-movie/SKILL.md`)와 `README.md`에는 반영되어 있지 않았다.**
특히 `SKILL.md` §3-0은 "`series-watch` 커맨드는 계획 단계이며 수동 플로우를 따른다"고
서술하여 이미 구현된 명령과 모순되었고, URL 인테이크·MCP 테이블·문제 해결 체크리스트도
구 구현 기준이라 1줄 역공학 요청(`"<url/mp4>를 역공학하여 <dir>에 저장해"`)을 받은
에이전트가 신기능을 활용하지 못하는 상태였다.

본 이슈는 스킬·README를 전문가 플레이북 수준으로 완성하고(요구사항 A),
1줄 요청 전체 흐름을 원스톱으로 검증하는 E2E 회귀 스크립트를 작성한다(요구사항 B).

---

## 2. 요구사항

### 요구사항 A — 스킬·README 전문가 문서화

a) Issue 8 ~ 11의 모든 전문가 기능을 `skills/rea-movie/SKILL.md`와 `README.md`에 반영:
- `sources/` 영구 캐시 (`<root>/sources/<source_id>/{video.mp4,probe.json,scene_cuts.json,keyframes/,shots_cache/,subtitles/}`, `index.json`) 및 중복 다운로드·추출 자동 스킵.
- `rea-movie series-watch` 서브커맨드 및 MCP 도구 `series_watch`
  (`<root>/story/{world.json,characters.json,timeline-days.md}` 자동 생성).
- `scripts/reverse-url.sh`의 `sources/` 캐시 연동, 플레이리스트 `series-watch` 선행 실행, `--episodes` 선별 역공학.
- `--min-shot-duration` 미세 컷 병합 및 4.0초 미만 렌더 클립의 `last_frame.jpg` 보존 리타이밍(`retime_render_clip`).
- M3 프롬프트 파이프라인의 `<Picture 1>`/`<Picture 2>` 델타 주입, 씬/시리즈 바이블 주입,
  워터마크/타임스탬프 노이즈 배제, H3 전송 시 HTML 주석 마커 자동 제거.
- 프로 연출 문법: A-B-A Shot/Reverse-Shot 2/3-hop lookback 씬 유지, 180도 축(`axis_of_action`)
  및 카메라 문법(`camera_grammar`), On-screen 발화 vs Off-screen 리액션/보이스오버(`delivery_mode`) 구분.
- 원문 대사 보존(기본 verbatim, `--translate-dub` 분리), `<d>[Language]</d>` 자동 언어 감지,
  6개국어 외부 자막(`.ko/.zh/.ja/.en/.fr/.de.srt`) + `.proof.srt` 자동 생성,
  원본 오디오 default 트랙 지정, `assemble --proof` 증명용 번인 비디오 생성.
- `/home/user1/.gemini/config/skills/rea-movie/SKILL.md`가 심볼릭 링크가 아니면 동기화.

b) `skills/rea-movie/SKILL.md`에 **`## 8. 프로페셔널 영화/드라마 연출가 & MiniMax-H3 전문가 검수 플레이북`** 신설:
- 1줄 역공학 요청 수신 시 에이전트가 수행할 5단계 자율 프로토콜
  (① `sources/` 캐시 확인 & `series-watch` → ② 씬 바이블 `axis_of_action`/`eyeline`/A-B-A 리버스샷 검수 →
  ③ `<Picture 1>→<Picture 2>` 델타 및 On/Off-screen 발화 검수 → ④ Smoke 렌더 & 조립(6개국어 `.srt` + 원본 오디오 기본) →
  ⑤ `_compare.mp4` 및 4점 세트로 지시→결과 대조 검수).
- 앵커 제거 실험(Ablation: `first_and_last` → `first_only` → `ref_only` → `none`) 수행 방법.

### 요구사항 B — E2E 통합 회귀 스크립트 `regression-tests/verify-issue-12.sh`

a) `skills/rea-movie/SKILL.md`와 `README.md`에 핵심 키워드·섹션이 모두 문서화되어 있는지 검사.
b) 1줄 요청 E2E 시뮬레이션: 다중 컷(A-B-A 대화 장면 + Off-screen 대사 + 4초 미만 샷 포함)
합성 시리즈 영상을 입력으로 ① `series-watch` → ② `decompile`
(`sources/` 캐시 생성, 씬 바이블 `axis_of_action`, 샷 프롬프트 `<Picture 1>/<Picture 2>`,
Off-screen `no lip movement`, 원문 대사 유지) → ③ `render --smoke --mock-h3` →
④ `assemble --smoke --proof`(4초 미만 리타이밍, 6개국어 `.srt` + `.proof.srt` + `_proof.mp4` +
원본 오디오 default disposition) 전 과정을 원스톱 검증.

---

## 3. 구현 결과

### 3-1. `skills/rea-movie/SKILL.md` (요구사항 A)

- **§D (URL 인테이크) 전면 개정**: `sources/yt-<id>/` 캐시, yt-dlp 다운로드 스킵,
  레거시 `source/` 폴백 링크, `.vtt`→`.srt` 스킵, 플레이리스트 목록 캐시
  (`sources/playlists/<list-id>.tsv`), playlist 선행 `series-watch`, `--episodes 1,2|1-3` 문서화.
- **§3-0 (시리즈 선행 분석) 구현 반영**: "계획 단계·수동 플로우" 서술을 삭제하고
  `rea-movie series-watch --out-root <root> [...] [--offline] [--force] [--interval]` 실명령,
  M3 1회 요약/오프라인 폴백, story 3종 스킵 규칙, MCP `series_watch` 언급,
  `sources/<source_id>/` 레이아웃·`source_id` 정규화(yt-11자리 / file-FNV-1a)·
  `scene_cuts.json` 구간 병합·`shots_cache/` 원자적 재사용으로 교체.
- **§A/B decompile·조립 옵션 추가**: 원문 verbatim 기본 + `--translate-dub`,
  `--min-shot-duration`, `assemble --smoke --proof/--offline` (6개국어 `.srt` + `.proof.srt`,
  전 구간 리타이밍, 듀얼 오디오 원본 default, compare 원본 우선 매핑),
  **§E 신설** (`render --smoke --mock-h3` GPU 없는 파이프라인 검증).
- **§3-3 (M3 프롬프트 파이프라인) 신설**: `<Picture 1>/<Picture 2>` 멀티모달 + Delta-trajectory 지시,
  씬/시리즈 바이블 주입, Negative rule(로고·워터마크·번인·하드섭 금지),
  `reference_line` 헤더 정규화, H3 전송 전 `<!-- rea-movie:... -->` 제거(디스크 유지),
  오프라인 폴백 비대사 영역 영어화.
- **§3-4 (프로 연출 문법) 신설**: A-B-A lookback (`HIST_SPLIT 0.55` / `REVERSE_SHOT_MAX_HIST 0.28`,
  무음·캡 분할 존중), `axis_of_action`/`camera_grammar` (KO `- 180도 축:`/H3 `Axis of action.` 블록),
  `delivery_mode` 4값 + Off-screen 판정 규칙(마커·화자-화면 불일치) + KO/H3 출력 문구,
  verbatim + `--translate-dub` + `detect_dialogue_language()` 스크립트 판정표.
- **§5 MCP 테이블**: 7→8 도구(`series_watch` 추가), `decompile_movie`에
  `min_shot_duration`/`translate_dub`, `assemble_movie`에 `proof`/`offline` 반영.
- **§6 체크리스트**: 구버전 산출물 판별 5종 추가
  (`<d>` 불일치→`sync-prompt --force`, 끝 앵커 소실→`--force` 재조립,
  A-B-A 과분할→lookback 재decompile, Off-screen 립싱크→`delivery_mode` 재생성, 무음샷 지시문 일관화).
- **§8 플레이북 신설** (요구사항 A-b): 5단계 자율 프로토콜(§8-0, 각 단계별 실행 명령·검수 포인트·
  산출물 체크리스트·재실행 루프 `./run.sh <shot> --force` → ④–⑤ 반복) + §8-1 앵커 Ablation
  (`first_and_last`→`first_only`→`ref_only`→`none`별 H3 요청 형태·관찰 기록법·§3-2 디버깅 참조).
- **Gemini 미러 동기화**: `/home/user1/.gemini/config/skills/rea-movie/SKILL.md`는
  일반 파일(심볼릭 링크 아님)이므로 `cp`로 동기화 완료 (`diff -q` 일치 확인).

### 3-2. `README.md` (요구사항 A)

- 1줄 요청 → 에이전트 5단계 프로토콜 안내(`SKILL.md` §8 참조) 추가.
- Issue 8~11 기능 섹션 신설: `sources/` 캐시 + `series-watch`(MCP `series_watch` 포함) +
  `reverse-url.sh` 연동 / `--min-shot-duration` + `retime_render_clip` + `reference_line` +
  Negative rule + 마커 제거 / A-B-A lookback + `delivery_mode` + `axis_of_action`/`camera_grammar` /
  verbatim + `--translate-dub` + `detect_dialogue_language()` + 6개국어 `.srt` + `.proof.srt` +
  원본 default + `--proof` 비디오.
- CLI 빠른 참조 + 회귀 테스트 실행법 추가.

### 3-3. `regression-tests/verify-issue-12.sh` (요구사항 B, 실행 권한 부여)

- **Check 1 (문서 커버리지)**: SKILL 35개 + README 12개 고정 문자열 검사(47 checks) —
  `sources/` 캐시 5종, `series-watch` 8종, `reverse-url --episodes` 2종,
  `--min-shot-duration`/`retime_render_clip`, `<Picture 1>/<Picture 2>`/`reference_line`/
  `Negative`/`<!-- rea-movie`, A-B-A/`axis_of_action`/`camera_grammar`/`delivery_mode`/
  `no lip movement`, verbatim/`detect_dialogue_language`/`--translate-dub`/`.proof.srt`/
  `default`/`--proof`, `## 8.`/자율 프로토콜/`Ablation`/`4점 세트`/`_compare.mp4`.
  + `reverse-url.sh` 정적 배선 검사(`sources/yt-`, `series-watch`, `--episodes`, 스킵 로그).
  + `## 8.` 본문 한정 5단계 스테이지 키워드 9종 검사.
  (`grep -qF --`로 `--` 접두 패턴 안전 처리.)
- **Check 2 (1줄 E2E, 합성 시리즈 7.5초)**:
  합성 입력 = red 2s / blue 2s / red 2s (A-B-A 대화) + green 1.5s (4초 미만 샷),
  sibling `.ko.srt` 4큐(온스크린 EN 2 + `(VO)` 한국어 Off-screen 1 + 숏 버튼 1).
  ① `series-watch --offline` → `story/` 3종 존재.
  ② `decompile --offline` → `Warning: no series context` 없음(컨텍스트 인수 확인),
  `sources/` 캐시(`probe.json`/`scene_cuts.json`/`shots_cache/`) 생성, ≥3샷 분할,
  씬 바이블 `axis_of_action`, VO 샷 KO `화면 밖` + H3 `no lip movement`/`off-screen` +
  `synchronized` 없음, EN verbatim + `<d>[English]>` + `<d>[Korean]>` + `<Picture 1>/<Picture 2>`.
  ③ `render --smoke --mock-h3` → 전 샷 렌더 존재
  (미디어 루트 규칙 `<root>/h3-results/<name>/` 준수 — 초판 실패 원인 수정).
  ④ 변조: 4초 미만 샷 렌더를 H3식 4초 red/blue 2단 클립으로 교체 후
  `assemble --smoke --offline --proof --force` →
  6개국어 `.srt` + `.proof.srt` + `_proof.mp4` 존재, 4큐·타임코드 전 언어 일치,
  변조 샷 구간 head 적색 우세 + tail 청색 우세(단순 절단이면 tail 적색 → **리타이밍 입증**),
  최종 길이 ≈ 원본(±0.6s), proof ≈ 최종(±0.5s),
  ffprobe 듀얼 오디오 중 원본(마지막 인덱스)만 `default=1`.

### 3-4. 품질 검사 통과

- `cargo fmt --check`: 통과 (Rust 변경 없음).
- `cargo clippy --all-targets -- -D warnings`: 경고 0건.
- `cargo test`: 84 passed, 0 failed.
- `regression-tests/verify-issue-12.sh`: Check 1~2 전체 PASS
  (E2E 실측: 3샷, head R=252.0 / tail B=253.0, source 7.52s / final 7.55s / proof 7.55s).
- 커밋은 하지 않고 테스트 통과 상태까지만 완료 (지시 준수).
