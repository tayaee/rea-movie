# Issue 10: 프로 연출 문법 고도화 — A-B-A Shot/Reverse-Shot 씬 분할 방지, 180도 축/샷 사이즈, On-screen vs Off-screen 발화 구분

## 1. 배경 및 문제

대화 장면에서 밝은 배경 A샷과 어두운 배경 B샷이 `A → B → A → B`로 교차할 때,
인접 샷 간 히스토그램 거리(`hist_dists[i]`)가 `HIST_SPLIT(0.55)`을 넘으면
`group_scenes()`가 매 경계를 장소 전환으로 오판해 하나의 대화를 여러 씬으로
쪼갰다. 동일 공간 내 카메라 앵글 교차(Shot/Reverse-Shot)를 장소 전환과
구분하지 못한 것이다.

또한 화면 속 인물이 입을 움직이지 않는 리액션 컷/보이스오버 구간에서도
`build_dialogue_lines()`가 무조건
`speaks clearly with synchronized lip movement`를 지시해, 화면 속 배우가
남의 대사를 립싱크하는 연출 파괴가 발생했다. 대사 큐의 `[off]`, `(VO)`,
`(내레이션)` 표기나 화자-화면 불일치를 판별하는 로직이 없었다.

씬 바이블에도 180도 축(`axis_of_action`)과 카메라 문법(`camera_grammar`)
필드가 없어 OTS/리버스샷 일관성을 M3에 지시할 수 없었다.

## 2. 목표

1. A-B-A 교차 편집을 동일 씬으로 유지하되, 진짜 장소 전환·무음 분할은 그대로 존중한다.
2. On-screen 직접 발화와 Off-screen 보이스/리액션을 구분해 프롬프트의 립싱크 지시를 분기한다.
3. 씬 바이블에 180도 축과 카메라 문법을 추가해 KO/H3 씬 블록과 M3 호출에 주입한다.

## 3. 상세 요구사항

### 3-1. 요구사항 A — A-B-A 인식 씬 그룹핑 (`src/scenes.rs`, `src/decompile/mod.rs`)

a) 대화 장면에서 밝은 배경 A와 어두운 배경 B가 교차(`A → B → A → B`)할 때
   인접 샷 간 히스토그램 거리(`hist_dists[i]`)가 `HIST_SPLIT(0.55)` 이상이어도,
   현재 샷(i)이 2컷 전(i-2) 또는 3컷 전(i-3) 샷과 히스토그램 거리가 가까우면
   (예: `REVERSE_SHOT_MAX_HIST = 0.28` 이하) 동일 공간 내 카메라 앵글
   교차(Shot/Reverse-Shot)로 판정하여 씬을 분할하지 않도록
   `group_scenes()`(또는 확장 함수 `group_scenes_with_lookback()`)를 고도화해.
b) 단, 무음 길이(`silence >= SILENCE_SPLIT_SECS`)나 최대 씬 길이 캡
   (`scene_len > MAX_SCENE_SECS && silence >= CAP_SILENCE_SECS`) 조건은 그대로
   존중하고, 시각적 컷(`hist >= HIST_SPLIT`)에 의한 분할만 A-B-A lookback
   유사도로 억제해.
c) `src/decompile/mod.rs`에서 샷별 contact_sheet(또는 first_frame) 간 1-hop
   거리뿐만 아니라 2-hop/3-hop 히스토그램 거리(`lookback_min_hist`)를 계산하여
   씬 그룹핑에 전달해.

### 3-2. 요구사항 B — On-screen vs Off-screen 구분 (`src/timeline.rs`, `src/decompile/prompt.rs`, `src/decompile/mod.rs`)

a) `ShotMeta`(및 `TimelineShotItem`)에
   `#[serde(default)] pub delivery_mode: String`
   (값: `"on_screen" | "off_screen" | "reaction" | "silent"`, 기본값 빈 문자열
   또는 자동 추론) 및 `#[serde(default)] pub shot_size: String`,
   `#[serde(default)] pub camera_angle: String` 필드를 하위 호환성 있게 추가해.
b) 대사 큐에 `[off]`, `(off-screen)`, `(VO)`, `(V.O.)`, `(내레이션)`,
   `(마음의 소리)` 표기가 있거나 `ShotMeta.delivery_mode`가
   `off_screen`/`reaction`인 경우, 혹은 화자 태그(예: `[S2] ...`)가 현재 샷의
   화면 등장 캐릭터(`shot_meta.characters`)에 포함되지 않는 경우 자동으로
   Off-screen 발화로 판별하는 함수(`infer_delivery_mode`)를 구현해.
c) Off-screen 발화일 때 `build_dialogue_lines()`가 화면 속 인물에게 입을
   움직여 말하라고 지시(`speaks clearly with synchronized lip movement`)하는 대신:
   - 한국어: `- [화면 밖 음성 / 리액션 샷] 화면 속 인물은 입을 다문 채 자연스러운 표정·시선 리액션을 유지하고, 화면 밖(Off-screen)에서 {}의 목소리가 들린다: <d>[{}] {}</d>`
   - H3 영문: `The on-screen subject remains silent with natural listening reaction and closed lips (no lip movement), while {} speaks off-screen (voice-over): <d>[{}] {}</d>.`
   형태로 출력하여 리액션 배우가 남의 대사를 립싱크하는 연출 파괴를 완벽히 차단해.
d) 온라인 M3 프롬프트 생성(`try_minimax_m3_api`) 시스템 프롬프트에도 화면 속
   인물이 직접 입을 움직여 말하는지(on-screen lip sync) 아니면 상대방의 말을
   듣는 리액션/보이스오버인지 구분하여 묘사하도록 지시해.

### 3-3. 요구사항 C — 씬 바이블 180도 축 및 카메라 문법 (`src/scenes.rs`, `src/decompile/prompt.rs`)

a) `SceneBible`에
   `#[serde(default, deserialize_with = "de_flex_string")] pub axis_of_action: String`
   (180도 축 및 좌/우 스크린 방향 규칙, 예:
   `S1 on screen-left looking right; S2 on screen-right looking left`)과
   `#[serde(default, deserialize_with = "de_flex_string")] pub camera_grammar: String`
   (주 샷 사이즈 및 OTS/리버스샷 구성) 필드를 하위 호환성 있게 추가하고,
   `format_scene_block_ko` / `format_scene_block_h3` 및
   `generate_scene_bible_m3`에 반영해.

## 4. 검수 기준

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. `regression-tests/verify-issue-10.sh` 통과:
   - A-B-A-B 교차 편집(1-hop 0.70, 2-hop 0.10)이 단일 씬으로 유지되고,
     진짜 장소 전환(1/2/3-hop 모두 0.70)은 씬 분리.
   - Off-screen 대사(`(VO)`, 화면 `['S1']`인데 `[S2]` 대사,
     `delivery_mode='off_screen'`)의 `prompt.ko.md`/`prompt.h3.txt`에
     `no lip movement`/`off-screen` 포함 + `synchronized lip movement` 제외.
   - `axis_of_action`/`camera_grammar` 직렬화·씬 블록 주입.
3. 기존 `verify-issue-4/5/6/8/9.sh` 모두 통과 (커밋 없음).

## 5. 구현 결과

### 5-1. `src/scenes.rs` — lookback 그룹핑 + 바이블 확장

- `pub const REVERSE_SHOT_MAX_HIST: f64 = 0.28` (신규).
- `group_scenes()`는 `vec![INFINITY]` lookback으로
  `group_scenes_with_lookback()`에 위임 (기존 동작·테스트 호환 유지).
- `group_scenes_with_lookback(spans, cues, hist_dists, lookback_min_hist)` (신규, `pub`):
  `hist >= HIST_SPLIT`일 때 이웃 증거
  `min(lookback[i], lookback[i+1], lookback[i+2]) <= REVERSE_SHOT_MAX_HIST`이면
  시각 분할 억제. `i+1/i+2` 전망(前望)은 A-B-A 온셋(첫 A→B컷은 히스토리가
  없어도 직후 A 복귀가 역샷임을 증명)을 처리. 무음·캡 분할은 항상 존중.
- `SceneBible`에 `axis_of_action`, `camera_grammar`
  (`#[serde(default, deserialize_with = "de_flex_string")]`) 추가.
- `offline_bible()`는 신필드 `""` 초기화.
- `format_scene_block_ko()`: `- 180도 축: {}`, `- 카메라 문법: {}` (비어있으면 생략).
- `format_scene_block_h3()`: `Axis of action: {}.`, `Camera grammar: {}.`.
- 단위 테스트: `test_group_scenes_reverse_shot_no_split` (1-hop 0.70/2-hop 0.10 → 1씬),
  `test_group_scenes_true_location_change_splits` (전부 0.70 → 분할),
  `test_group_scenes_silence_still_splits_despite_lookback` (무음은 lookback 무시하고 분할),
  `test_bible_axis_fields_roundtrip_and_blocks` (신필드 serde·KO/H3 블록 주입·레거시 `{}` 호환).

### 5-2. `src/timeline.rs` — 전달 모드·카메라 필드

- `ShotMeta` + `TimelineShotItem`에 `delivery_mode`/`shot_size`/`camera_angle`
  (`#[serde(default)] String`) 추가. 기존 TOML/JSON에 키가 없어도 파싱 성공.
- `test_shot_meta_serialization`에 신필드 라운드트립 + 레거시 TOML 기본값 `""` 검증 추가.

### 5-3. `src/decompile/prompt.rs` — 발화 모드 분기

- `PromptGenContext`에 `delivery_mode: &str`, `on_screen_characters: &[String]`,
  `shot_size: &str`, `camera_angle: &str` 추가.
- `pub fn infer_delivery_mode(hint, lines, speaker, on_screen) -> String` (신규):
  1) 명시 hint (`off_screen`/`reaction`/`silent`/`on_screen`, 대소문자·하이픈 변형) 우선.
  2) 공백만 → `silent`.
  3) 소문자 조인 텍스트에 `[off]`, `(off-screen)`, `(vo)`, `(v.o.)`, `내레이션`,
     `마음의 소리`, `[vo]`, `voice-over`, `(o.s.)` 등 → `off_screen`.
  4) 큐 내 `[Sx]` 태그가 화면 캐릭터에 없거나 `speaker`가 화면에 없으면 → `off_screen`.
  5) 그 외 → `on_screen`.
- `build_dialogue_lines()`:
  `off_screen`/`reaction`이면 요구사항 문구 그대로 출력
  (KO `화면 밖(Off-screen)...입을 다문 채...`,
   H3 `remains silent with natural listening reaction and closed lips (no lip movement), while ... speaks off-screen (voice-over)`);
  `synchronized lip movement` 미포함. `silent`는 기존 무음 문구, `on_screen`은 기존 립싱크 문구.
- `generate_deterministic_template()`: `shot_size`/`camera_angle`이 비어있지 않으면
  KO `- 샷 사이즈/앵글: ...`, H3 `Shot size/angle: ....` 행 추가.
- `format_scene_bible_for_m3()`: `Axis of action`/`Camera grammar` 행 추가.
- `try_minimax_m3_api()` 시스템 프롬프트에 전달 모드 지시 추가:
  `Delivery mode: describe whether the on-screen subject speaks directly with synchronized lip movement (on-screen) or remains silent with a natural listening reaction and closed lips while another voice speaks off-screen as voice-over (off-screen/reaction); never describe a silent on-screen listener as speaking with synchronized lip movement.`
- `generate_scene_bible_m3()` 시스템 프롬프트 키에 `axis_of_action, camera_grammar` 추가 및
  180도 축 준수·OTS/리버스샷 규칙 명시.
- 단위 테스트: `test_infer_delivery_mode_markers_and_hints`,
  `test_build_dialogue_off_screen_has_no_lipsync`
  (VO는 `화면 밖`+`no lip movement`+`off-screen` 포함·`synchronized` 제외,
   일반 대사는 `synchronized` 포함·`no lip movement` 제외).

### 5-4. `src/decompile/mod.rs` — lookback 계산 + 컨텍스트 배선

- 5b: 1-hop `hist_dists` 외에 `lookback_min_hist[i] = min(dist(i,i-2), dist(i,i-3))`
  (`i<2`는 `INFINITY`, `contact_sheet.jpg` 간 `hist_distance_jpeg`) 계산 후
  `group_scenes_with_lookback()` 호출.
- Loop B: 신규 `ShotMeta` 필드 `""` 초기화, 기존 파일 로드 시 보존
  (`characters`/`scene_id`와 동일 패턴). `TimelineShotItem`에
  `delivery_mode`/`shot_size`/`camera_angle` 복사.
- `PromptGenContext`에 `delivery_mode`/`on_screen_characters`(=`shot_meta.characters`)/
  `shot_size`/`camera_angle` 주입.

### 5-5. `src/sync_prompt.rs` — 테스트 헬퍼 갱신

- `meta()`에 신필드 `String::new()` 3종 추가 (컴파일 오류 방지, 동작 불변).

### 5-6. 회귀 검증 (`regression-tests/verify-issue-10.sh`, 실행 권한 부여)

- Check 1: `cargo test`로 역샷 단일씬·진짜전환 분할·무음 존중 3종 검증.
- Check 2: 오프라인 종단 4종 —
  2a `(VO)` SRT decompile → KO `화면 밖`+`입을 다문`, H3 `no lip movement`+`off-screen`,
  `synchronized` 없음. 2b `[S2]`+화면 `[S1]` → off-screen.
  2c 일반 대사 → `synchronized` 포함·`no lip movement` 없음.
  2d `shot.toml delivery_mode="off_screen"` 후 프롬프트 삭제·무`--force` 재실행 →
  일반 대사도 off-screen으로 재생성 (hint 우선순위 입증).
  + `infer_delivery_mode`/`build_dialogue` 단위 테스트.
- Check 3: `test_bible_axis...` 단위 테스트 +
  `sync-prompt --force` 종단 — `scene_0001.json`에 축/문법 기입 후
  KO 블록 `180도 축`/`카메라 문법`, H3 블록 `Axis of action`/`Camera grammar` 확인.

### 5-7. 품질 검사 통과

- `cargo fmt --check`: 통과.
- `cargo clippy --all-targets -- -D warnings`: 경고 0건
  (`group_scenes` 구호환 래퍼에 `#[allow(dead_code)]`).
- `cargo test`: 전체 79개 통과 (신규 6개 포함).
- `regression-tests/verify-issue-10.sh`: 전체 3개 체크 패스.
- 기존 `verify-issue-4.sh` / `5.sh` / `6.sh` / `8.sh` / `9.sh`: 모두 통과.
