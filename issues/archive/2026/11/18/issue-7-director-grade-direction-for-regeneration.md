# Issue 7: 최고의 연출가 지식으로 원본급 재생성을 가능케 하는 연출서

## 1. 배경 및 최종 목표

`rea-movie` 스킬과 프로젝트의 최종 목표는 하나다. **최고의 드라마/영화 감독의
연출 지식을 이용하여 연출서를 작성하고, 그 연출서로 MiniMax-H3가 원본과 거의
같은 품질의 비디오를 생성할 수 있도록 하는 것.**

역공학(`h3-projects` 생성)이 목표가 아니라 수단이다. 역공학 과정에서
"이렇게 지시하니 이렇게 나온다"는 지시→결과 매핑을 학습하고, 그 매핑이 곧
신규 창작(원본 앵커 없음) 시 최소 필요 입력의 정답이 된다.

## 2. 범위

### 2-1. 분석 범위 vs 출력 범위 (one-fine-week 기준)
- 분석: EP01~EP10 전체 (인물·대사 분석, 에피소드간 일관성 유지용).
- 출력: EP01~EP02의 앞부분 1분씩, 재생성 비디오 2개 (`-v2` 프로젝트).
- 소스: `https://www.youtube.com/playlist?list=PLqqQvcAR1H0lvElJjtKv5x6qFq8NPP-fw`

### 2-2. 디렉토리 규약
- 역공학 프로젝트(가벼움, git 추적): `~/rea-movie-artifacts/h3-projects/<name>/`
- 재생성 결과물(무거움): `~/rea-movie-artifacts/h3-results/<name>/` (형제 `../h3-results/` 자동 매핑)
- wsl2, spark1, spark2의 `~/rea-movie-artifacts`는 모두 `/rosenas/data/rea-movie-artifacts` 심볼릭 링크.
- 구 레이아웃(`rea-movie-projects/`, `rea-movie-workspace/`)은 마이그레이션 폴백 전용. 신규 작업 금지.

### 2-3. 당면 과제 (최우선)
**수십초 이어지는 대사 동안 여러 각도의 다른 카메라 웍 연출을 rea-movie가
제대로 다루지 못한다.** 현재 파이프라인은 컷 단위로 샷을 쪼개 각 샷을 독립
프롬프트로 렌더하므로, 샷간 연결(인물 동일성, 시선 문법, 공간 배치)이 끊긴다.
출력 비디오의 품질이 조악하고 샷간 연결이 안 되는 근본 원인이다.
smoke 해상도(608x352)는 변명이 안 된다. 연출서 품질의 문제다.

## 3. 상세 요구사항

### 3-0. 시리즈 선행 분석 (스킬 §3-0, 기본 동작)
부분 역공학 요청이라도 전체 시리즈를 먼저 요구하는 것이 기본값이다.
smoke든 prod든 연출 지시는 시리즈 전체를 본 뒤에 작성한다.
사용자가 일부만 제공하겠다고 명시 확인한 경우에만 그것으로 진행하며,
이 경우 인물 ID는 지역 스코프이고 에피소드간 일관성은 보장하지 않는다고 고지한다.
"전체 시청"은 풀 디컴파일이 아니라 가벼운 프로브다 (풀 디컴파일은 출력 범위에만).
출력 범위 decompile 전에 EP01~EP10 전편을 ingest하여 아래를 `story/`에 기록한다:
1. `world.json` (장소·시대·팔레트·촬영문법), 2. `characters.json` (전역 ID·EP별 의상·관계·말투),
3. `timeline-days.md` (EP별 day 매핑), 4. 에피소드 아크표, 5. 축·스크린 방향, 6. 소품·의상 연속성.
이후 씬 바이블의 인물 ID는 전역 레지스트리를 참조한다. M3 1회/편 요약으로 초안 작성 후 사람이 확정.

### 3-1. 씬 레이어 연출 (엔진 구현됨, 검증·고도화 필요)
1. 연속 샷을 **씬**으로 그룹핑한다. 현재 구현은 무음 길이·화면 변화·최대 길이
   기준으로 나누며, EP01 2분 샘플에서 장소 전환과 일치함을 확인했다.
   단, 이 수치는 단일 샘플에서 맞춘 v1이며 장르·조명·액션 장면에 그대로
   통한다는 보장은 없다. EP01~EP02 전체 적용 후 육안 검수로 확정한다.
2. 씬마다 **씬 바이블**(`story/scenes/scene_XXXX.json`)을 M3 비전으로 생성한다 (씬당 1회):
   장소, 시간대, 인물(S1, S2...) 외모·의상 고정점, 공간 배치도, 시선 문법,
   대사 흐름. 동일 씬 내 인물 ID는 앵글이 바뀌어도 동일 인물로 유지한다.
3. M3 실패 시 결정적 offline 바이블로 폴백한다. M3 JSON 파싱은 관대하게
   (문자열|배열|맵 모두 수용).
4. 각 샷의 `prompt.ko.md`/`prompt.h3.txt` 선두에 씬 블록(`## 0. 씬 컨텍스트`)이
   정확히 1회 들어간다. `shot.toml`과 `timeline.json`은 `scene_id`를 가진다.
5. 고도화 방향 (우선순위순): ① EP 전체 검증 후 임계값 재조정 (고정값→영상별 적응형),
   ② 화자 교대 패턴(shot/reverse-shot 교대=동일 씬 증거) 활용 — 화자분리 도입 후,
   ③ M3 의미론적 장면분할을 정답지로 삼은 캘리브레이션.

### 3-2. 대사 원문 그대로 재현 (수정됨)
1. 자막 큐는 시작점이 속한 샷 하나에만 할당한다 (`assign_cues_to_spans`).
   overlap 매칭으로 인한 조립본 대사 반복이 0건이어야 한다.
2. 대사는 **원본 그대로 재현**한다. 번역·의역·창작 금지. `<d>` 태그의 언어 표기는
   본문 언어와 일치해야 한다. (종전 한국어 더빙 기본값·per-shot EN→KO 번역 방침은 폐기.
   `dub-lang` 기본값과 번역 파이프라인 위치는 재검토 대상.)
3. 무음샷 h3 태그 영어 하드코딩을 `dub_lang` 추종으로 수정한다 (미해결).

### 3-3. 증명용 출력 vs 최종 출력
1. 증명용(현재): 생성된 비디오의 자막 앞에 인물명을 한국어로 (`[이름] 영어대사` 1줄 통합).
   인물 감지 능력의 증거다.
2. 증명용(현재): 자막을 영어로 변환하여 넣는다. 대사 감지 능력의 증거다.
3. 증명용(현재): 최다대사 남배우 1인을 박보검으로, 여배우 1인을 카리나로 교체한다.
   인물 수정 능력의 증거다. (`ref2va` face 교체; 현재 1인분만 전송 — 다중인물 미지원)
4. 최종본: 화자 태그를 넣지 않는다. 원본 언어(한국어 등)를 유지해도 된다.

### 3-4. 다국어 자막 외부 파일 + 원본 언어 음성 (수정됨)
1. 자막은 mux가 아니라 **외부 자막 파일 6개**로 제공한다:
   한국어, 중국어, 일본어, 영어, 프랑스어, 독일어 (`.srt`, 비디오별).
2. 번역 원문은 3-2의 원문 대사. 번역 품질은 M3 배치 번역 + 캐시를 사용한다.
   (종전 per-shot 더빙 번역은 자막 파일 생성 파이프라인으로 이동.)
3. 음성(오디오 트랙)은 **원본 비디오의 언어를 따른다** (본 프로젝트: 한국어).
   최종 본편의 기본 오디오는 원본 오디오이며, H3 생성 오디오와의 관계(듀얼트랙
   유지 vs 원본 단일)는 구현 시 확정한다.
4. 증명용(화자태그 `[이름] 대사` 자막, §3-3)은 별도 번인 빌드로 리뷰한다.

### 3-5. 앵커 제거 실험 (ablation, 예정)
신규 창작(원본 앵커 없음) 시 최소 필요 입력을 정량화한다. 동일 샷에 대해
`first_and_last → first_only → ref_only → none` 순서로 앵커를 걷어내며 재생성하고
인물 일관성 붕괴 지점을 측정한다. `contact_sheet.jpg`는 H3 입력이 아니므로
제거 대상이 아니다 (M3 연출서·인물감지용일 뿐).

### 3-6. 사용 UX
1. 실제 사용자는 `"<youtube playlist or video url>을 역공학하여 <dir>에 저장해"` 또는
   `"<mp4>파일을 역공학 하여 <output-root>에 저장해"` 형태로 지시한다
   (`scripts/reverse-url.sh` 표준 플로우).
2. 그 다음부터 rea-movie는 최고의 연출 스킬 전문가로 행동한다:
   씬 바이블 검토 → 샷별 연출서 다듬기 → smoke 검증 → 본편 조립.

## 4. 검수 기준 (`regression-tests/verify-issue-7.sh` 자동화 기준)

> **구현 지침 (Coder Agent 필독)**:
> - 본 이슈의 하위 엔진 기능들은 Issue 8~12에서 구현되었으며, 현재 워킹 트리에 있는 안정성 수정사항(`src/assemble.rs`의 `detect_speech_window` 1.0x speech trim guard, `src/decompile/extract.rs`의 경계 프레임 타임스탬프 폴백, `src/decompile/contact_sheet.rs`의 `-pix_fmt yuvj420p`)을 절대 되돌리지 말고 보존·포맷팅(`cargo fmt`)하십시오.
> - **추가 필수 구현 (`src/series_watch.rs`)**: `run_series_watch()`에서 이번 실행 결과 `mode == "offline"`일 때(예: `--offline` 또는 M3 API 호출 실패 폴백), 설령 `opts.force == true`라 하더라도 디스크의 기존 `world.json` 또는 `characters.json`이 이미 `"mode": "m3"`(온라인 리치 분석본)를 가지고 있다면 오프라인 빈 초안으로 덮어쓰지 않고 보존(skip)하도록 보호 로직과 단위 테스트를 추가하십시오 (`timeline-days.md`도 기존 `world.json`이 `"m3"`이고 이번 `mode == "offline"`이면 보존).
> - **회귀 테스트 스크립트 작성 (`regression-tests/verify-issue-7.sh`, `chmod +x`)**: 외부 네트워크/GPU 없이 로컬 합성 영상으로 아래 항목을 빠르게 자동 검증하도록 작성하십시오:
>   1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
>   2. **씬 레이어 및 연출서 검증**: 다중 컷 합성 영상 + SRT decompile 시 하나의 대화 장면(A-B-A 교차 포함)이 단일 씬으로 묶이고 장소 전환 시 분리되며, 각 샷 `prompt.ko.md`/`prompt.h3.txt`에 씬 블록(`<!-- rea-movie:scene:begin`)이 정확히 1회 주입되고 `timeline.json`의 `scene_id`가 매핑됨을 확인.
>   3. **원문 대사·6개국어 자막·원본 오디오·Speech 1.0x Trim 가드 검증**:
>      - 대사 큐 중복 할당 0건 및 원문 그대로 재현.
>      - 조립(`assemble --smoke --offline --proof`) 시 외부 자막 6종(`ko/zh/ja/en/fr/de.srt`) + `.proof.srt` 생성 및 기본 오디오 트랙이 원본(`default=1`)인지 확인.
>      - 4초 미만 샷에 진폭이 변하는(varying RMS) 실제 발화형 오디오가 들어있을 때 `detect_speech_window`가 1.0x 윈도우 트림 경로로 정상 조립하는지 확인.
>   4. **`series-watch` M3 리치 컨텍스트 덮어쓰기 방지 검증**: `story/world.json` 및 `story/characters.json`에 `"mode": "m3"` 데이터가 있을 때 `series-watch --offline --force`를 실행해도 `"mode": "m3"` 파일이 덮어써지지 않고 보존되는지 확인.
> - 구현 및 검증 통과 후 본 문서 하단에 `## 7. 구현 결과` 섹션을 작성하십시오 (`git commit` / `git mv`는 오케스트레이터가 수행하므로 직접 실행 금지).

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. 씬 리그레션 (결과 기준 — 수치 임계값은 구현이 책임진다):
   - 하나의 이어지는 대화 장면(shot/reverse-shot 포함)이 여러 씬으로 쪼개지지 않는다.
   - 장소·시간이 바뀌면 씬이 나뉜다.
   - 씬 블록이 각 샷 프롬프트에 정확히 1회 들어간다 (중복·누락 없음).
3. 산출물 검수:
   - 실제 대사 중복 0건, 대사는 원문 그대로 (번역·의역 없음).
   - 비디오별 외부 자막 6종 (ko/zh/ja/en/fr/de `.srt`) 존재, 타임코드 정합.
   - 기본 오디오는 원본 언어.
   - 각 샷 프롬프트 선두에 해당 씬 블록 존재, `timeline.json`의 `scene_id` 매핑 완전.
   - 본편+비교 영상 조립 완료.
4. `series-watch` 오프라인 폴백 시 기존 `"mode": "m3"` 리치 컨텍스트 보존.

## 5. 비범위 (이번 이슈에서 하지 않는 것)

1. prod(960x576, 50 steps) 렌더 — smoke 검증이 먼저다.
2. EP03 출력 — EP01~EP02 1분 2개 완성 후.
3. EP04~EP10 다운로드 — 단, §3-0 시리즈 선행 분석에 필요하므로 출력 2편 연출서
   1차 완성 후 다운로드한다 (분석용, 전체 decompile 아님).
4. 에피소드간 인물 동일성 자동화 (크로스에피소드 바이블, 성문/얼굴 임베딩) —
   수동 레지스트리(`story/characters.json`)가 먼저다.
5. 수십 개 언어 확장 — 6개 자막 파일 구조가 잡힌 후.
6. 전용 `series-watch` 커맨드 — 스킬 수동 플로우(§3-0)로 시작하고, 반복되면 자동화한다.

## 6. 참고 (실측 운영 지식)

- smoke 샷당 2~7분 (decompile 온라인 샷당 ~2분 포함). 2분 60샷 ≈ 노드당 3~4시간.
- 노드당 1작업 고정 (`REA_MOVIE_H3_ENDPOINT=... ./run.sh --smoke`). 3중첩 경합 시 504/행 발생.
- H3 행 판별: `/health` 200 + GPU 0% + 15분 무응답 → `docker restart h3-fl2va` → 단발샷 프로브 후 재개.
- M3 529 과부하 시 온라인 decompile 강행 금지 (템플릿 폴백 양산). 회복 프로브 후 재개.
- 중지: `pkill -f '[r]un\.sh --smoke'` 후 `pkill -f '[r]ea-movie render'` (자기매칭 주의).

## 7. 구현 결과

1. **`series-watch` M3 리치 컨텍스트 보존 로직 추가 (`src/series_watch.rs`)**:
   - `has_m3_mode(&Path) -> bool` 도우미 함수를 구현하여 JSON 내 `"mode": "m3"` 존재 여부를 판별.
   - `run_series_watch()` 실행 결과가 `mode == "offline"`인 경우, `--force` 플래그가 주어지더라도 기존 `story/world.json` 또는 `story/characters.json`이 `"mode": "m3"`이면 덮어쓰지 않고 보존(`report.skipped` 추가).
   - `story/timeline-days.md` 역시 기존 `world.json`이 `"mode": "m3"`이고 파일이 존재하면 보존.
   - 관련 단위 테스트(`test_has_m3_mode_behavior`, `test_preserve_existing_m3_context_on_offline_force`)를 추가하여 회귀 방지.

2. **기존 엔진 안정성 수정사항 보존 및 검증 단위 테스트 강화 (`src/assemble.rs`, `src/decompile/extract.rs`, `src/decompile/contact_sheet.rs`)**:
   - `src/assemble.rs`: `detect_speech_window` 기반 varying-RMS 실제 발화형 오디오 탐지 시 1.0x 윈도우 트림(`-ss -t`) 경로 보존 및 `test_detect_speech_window_varying_vs_constant_sine` 단위 테스트 추가.
   - `src/decompile/extract.rs`: 경계 프레임 타임스탬프 폴백 후보군(`candidates = [t, t - 0.2, t - 0.4, start_sec + 0.05]`) 및 `-pix_fmt yuvj420p` 보존.
   - `src/decompile/contact_sheet.rs`: 타일 프레임 추출 시 `-pix_fmt yuvj420p` 보존.
   - `cargo fmt` 전체 코드 포맷팅 정렬.

3. **자동화 회귀 검증 스크립트 작성 (`regression-tests/verify-issue-7.sh`, `chmod +x`)**:
   - **Gate 1**: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` (87개 테스트 전체 통과).
   - **Check 1 (`series-watch` M3 보존)**: `mode: m3` 데이터가 디스크에 존재하는 상태에서 `series-watch --offline --force` 실행 시 기존 M3 세계관/인물/타임라인 데이터가 손실 없이 100% 보존됨을 검증.
   - **Check 2 (씬 레이어 & 원문 대사)**: 다중 컷 합성 영상(A-B-A 리버스샷 대화 + 화이트 장소 전환) 및 자막 decompile 시:
     - A-B-A 교차 대화(3개 샷)가 단일 씬(`scene_0001`)으로 묶이고, 장소 전환 샷이 신규 씬(`scene_0002`)으로 분리됨을 확인.
     - 각 샷 프롬프트(`prompt.ko.md`, `prompt.h3.txt`) 선두에 씬 블록(`<!-- rea-movie:scene:begin`)이 정확히 1회 주입되고 `timeline.json`의 `scene_id`가 매핑됨을 확인.
     - 대사 큐 중복 할당 0건 및 원문 대사 그대로 재현 확인.
   - **Check 3 (조립 & 6개국어 자막 & 원본 오디오 & Speech 1.0x Trim 가드)**:
     - 4초 미만 샷에 varying RMS 발화형 오디오를 주입했을 때 `detect_speech_window`를 통한 1.0x 트림 정상 동작 확인.
     - 조립(`assemble --smoke --offline --proof`) 결과 외부 자막 6종(`ko/zh/ja/en/fr/de.srt`) 및 `.proof.srt` 생성, 타임코드 정합 확인.
     - 원본 오디오 트랙이 최종 영상의 기본 오디오 트랙(`stream_disposition=default: 1`)으로 올바르게 설정됨을 확인.
   - 모든 항목 자동 검증 완료 확인.
