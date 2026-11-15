# Issue 4: 커스터마이징 레이어(`customization/`) — 인물 캐스팅 + 전역/샷 단위 트레이트 주입

## 1. 배경 및 문제

현재 `rea-movie`의 프롬프트 커스터마이징은 `--mod` 단일 문자열 + 하드코딩된 `"kbo"`/`"야구모자"`
매칭(`src/decompile/prompt.rs:368`, `src/sync_prompt.rs:85`)으로만 가능하다. 다음 결함이 있다.

1. `--mod`는 **MiniMax-M3 API 성공 경로에서 완전히 무시**된다(오프라인 템플릿 경로에서만 적용).
2. 인물 단위 지정이 불가능하다. `characters`는 항상 `["S1"]` 하드코딩(`src/decompile/mod.rs:201,212`).
   → "walter에게만" 같은 지시를 표현할 수 없다.
3. `sync_prompt::compile_ko_to_h3`가 **한국어 본문을 영어 H3 프롬프트에 그대로 삽입**한다
   (`src/sync_prompt.rs:48-95`). 문서화된 편집 루프(`./run.sh 0015 --force`)가 영문 프롬프트를
   망가뜨리는 것이 확인되었다.
4. `decompile --force`가 `prompt.ko.md`를 덮어써 사람의 편집을 잃는다(`src/decompile/prompt.rs:27`).
5. M3 API 1회 실패 시 전역 static 플래그로 **프로세스 전체가 영구히 템플릿 모드**가 된다
   (`src/decompile/prompt.rs:5,39,52`).

목표: **"walter 에게만 KBO 야구모자를 항상"** 같은 지시를 구조화·재현 가능하게 표현하고,
프롬프트는 사람이 쓰는 언어(ko)와 모델이 먹는 언어(en)를 분리하며, 모든 산출물을
단일 진실 공급원에서 생성한다.

## 2. 설계 요약

```
<project>/
  customization/
    characters.toml        # 인물 Bible (id, 외형, 복장, face/voice 레퍼런스 경로)
    traits.toml            # 전역/샷 단위 트레이트 (statement_en + statement_ko)
    shots/shot_0015.toml   # 샷 단위 override (cast, action_swap, prop 제거)
  shots/shot_XXXX/
    prompt.ko.md           # 사람이 쓰는 한국어 연출서 (base 영역 + 자동 주입 블록)
    prompt.h3.txt          # 모델이 먹는 영문 프롬프트   (base 영역 + 자동 주입 블록)
```

- `customization/`은 **읽기 전용 입력**(덮어쓰지 않는다). 사람이 지시를 바꾼 뒤 재적용할 수 있다.
- `prompt.ko.md` / `prompt.h3.txt`는 **파생물**. base 영역은 보존하고, 주입 블록만 갱신한다(멱등).
- H3 프롬프트에는 **영어만** 들어간다.

## 3. 상세 요구사항

### 3.1 `src/customize.rs` 신규 모듈

```rust
// customization/characters.toml
// [characters.walter]
// name_ko = "월터 화이트"
// name_en = "Walter White"
// appearance_en = "middle-aged man, bald head, goatee, hollow cheeks, dark sunken eyes"
// appearance_ko = "중년 남성, 대머리, 콧수염, 함몰된 눈"
// wardrobe_en = "black bomber jacket over a blue hoodie"
// wardrobe_ko = "검정 볼트넥 재킷에 파란 후드"
// face_ref = "assets/characters/walter/face.jpg"
// voice_ref = "assets/characters/walter/voice.wav"

// customization/traits.toml
// [[traits]]
// id = "kbo_cap"
// scope = "global"                 # "global" | "shot"
// target = "character:walter"      # "character:<id>" | "subject:lead" | "scene" | "prop:<id>"
// shots = []                       # scope == "shot" 일 때 대상 샷 ID 목록
// statement_en = "Walter wears a navy-and-red KBO baseball cap ... in every shot."
// statement_ko = "월터는 모든 샷에서 ... KBO 야구모자를 착용한다."
// negative_en = "no bare head, no cap removal"   # optional
// negative_ko = ""                                # optional

// customization/shots/shot_0015.toml
// cast = ["walter"]
// [[overrides]]
// kind = "action_swap"   # "action_swap" | "remove" | "preserve"
// from = "smoking a cigarette"
// to   = "drinking water from a glass bottle"
// target = "prop:cigarette, smoke"   # remove/preserve 사용
```

핵심 API:

```rust
pub struct Block { pub id: String, pub en: String, pub ko: String }

pub struct Customization { /* characters, traits, shots */ }

impl Customization {
    pub fn load(proj_dir: &Path) -> Customization;              // 파일 부재 시 빈 구조(오류 아님)
    pub fn has_any(&self) -> bool;
    pub fn cast_for(&self, shot_id: &str, fallback: &[String]) -> Vec<String>;
    pub fn blocks_for(&self, shot_id: &str, cast: &[String]) -> Vec<Block>;
}
pub fn apply_to_h3(base: &str, blocks: &[Block]) -> String;
pub fn apply_to_ko(base: &str, blocks: &[Block]) -> String;
```

### 3.2 블록 주입 규칙 (멱등)

- 마커 형식(ko/h3 공통):

  ```
  <!-- rea-movie:block:begin id=<id> -->
  ...본문...
  <!-- rea-movie:block:end id=<id> -->
  ```

- `apply_to_h3`:
  1. 기존 `block:begin/end` 구간 **전부 제거**(id 무관).
  2. `integrated_multimodal_description:` 줄 **바로 다음**에 새 블록을 삽입.
     해당 줄이 없으면 파일 끝에 추가(그리고 `integrated_multimodal_description:` 헤더를 생성).
  3. base 영역(아래 3.3)은 그대로 보존.
- `apply_to_ko`:
  1. 기존 블록 구간 전부 제거.
  2. 파일 끝에 `## 6. 자동 주입 커스텀 지시` 섹션으로 블록을 추가(섹션이 이미 있으면 그 자리 갱신).
- 같은 함수를 두 번 호출해도 결과가 동일(중복 블록 없음)해야 한다 → **단위 테스트 필수**.

### 3.3 base 영역 보존

`prompt.ko.md` / `prompt.h3.txt` 첫머리에 base 마커를 둔다.

```
<!-- rea-movie:base:begin -->
...사람이 편집할 수 있는 본문...
<!-- rea-movie:base:end -->
```

- `decompile`는 base가 이미 있으면 **base를 재생성하지 않는다**(사람의 편집 보존).
  파일이 없을 때만 M3 API(또는 결정론적 템플릿)로 base를 생성한다.
- `--force`도 **base를 보존**한다. 강제 재생성은 `customization/`을 바꿔서 반영한다.
  (레거시 파일에 base 마커가 없으면 전체를 base로 간주.)

### 3.4 인물 캐스팅 감지 (MiniMax-M3 비전)

- `customization/characters.toml`이 존재할 때, decompile이 각 샷의 contact sheet를 M3에 보내
  "이 프레임에 등장하는 인물 id만 JSON으로 반환"하도록 요청한다.
  요청 본문에 인물 Bible(id + `appearance_en`)를 포함한다.
- 응답 JSON: `{"characters": ["walter", "mike"]}` → Bible에 있는 id만 채택.
- 실패/파싱 오류 → 빈 목록 → 기존 동작(`["S1"]`)으로 폴백(경고 1회 출력).
- 감지 결과는 `shots/shot_XXXX/shot.toml`의 `characters`와 `timeline.json`의 `characters`에 저장.
  (`TimelineShotItem`에 `#[serde(default)] characters: Vec<String>` 추가.)
- `customization/shots/<shot_id>.toml`에 `cast`가 명시되어 있으면 **자동 감지보다 우선**한다(사람의 지정 우선).

### 3.5 트레이트 적용 판정

- `scope == "global"`:
  - `target`이 `character:<id>` → **그 인물이 해당 샷 cast에 있을 때만** 적용.
  - `target`이 그 외(`subject:lead`, `scene`, `prop:*`) → 모든 샷에 적용.
- `scope == "shot"` → `shots` 목록에 포함된 샷에만 적용(또 `character:` 대상이면 cast 조건도 확인).
- 블록 순서: **캐릭터 Bible(캐스트 순서) → 트레이트 → 샷 override**.

### 3.6 `sync-prompt` 재정의 (한국어 → 영어 H3 프롬프트)

- 현재 동작(ko 본문을 영어 프롬프트에 삽입)은 **금지**. 영어 H3 프롬프트에 한글이 들어가면 안 된다.
- 새 동작:
  1. `prompt.ko.md`의 base 영역을 읽고(자동 주입 블록은 제거) M3 API로 **영어 H3 프롬프트로 변환**한다.
     기존 `try_minimax_m3_api`의 JSON 출력(`prompt_h3`) 방식을 재사용한다.
     `<d>[Language] ...</d>` 대사 태그는 원문 그대로 유지하도록 지시한다.
  2. `prompt.h3.txt` = 새 영어 base + 커스텀 블록(영문).
  3. API를 쓸 수 없으면(키 없음/호출 실패) **기존 `prompt.h3.txt`의 base를 유지**하고
     커스텀 블록만 갱신한다. 한글이 영어 프롬프리에 섞이지 않는다. 경고 출력.
- `prompt.ko.md`는 base를 보존한 채 커스텀 블록만 갱신한다.

### 3.7 `--mod` 호환

- `--mod "<한국어 텍스트>"`는 **전역 트레이트 1건**으로 변환되어 모든 샷에 적용된다.
  - `statement_ko` = 입력 텍스트.
  - `statement_en` = M3 API로 번역 가능하면 번역 결과, 아니면 원문(경고).
- `src/decompile/prompt.rs`와 `src/sync_prompt.rs`의 **하드코딩된 `"kbo"`/`"야구모자"` 분기 삭제**.
  (`--mod`가 온라인 경로에서도 동작해야 한다.)

### 3.8 CLI / MCP

CLI:
```
rea-movie customize apply --proj-dir . [--shot 0015]   # 블록 재적용(멱등)
rea-movie customize show  --proj-dir .                # 인물/트레이트/샷 cast 요약
```

MCP: `apply_customization`(proj_dir, shot), `show_customization`(proj_dir) 2개 추가.
`tools/list` 테스트의 개수 기대값(7)을 9로 갱신.

### 3.9 예외 처리 규칙

- `customization/` 파일이 하나도 없으면: 기존 동작과 **100% 동일**하게 동작(회귀 없음).
- M3 API 실패: 해당 샷만 base 재생성 실패 → 결정론적 템플릿으로 폴백(기존 동작 유지).
  전역 static 비활성화 플래그는 **샷 단위로만** 유지하고, 실패 로그를 남긴다.

## 4. 검수 기준

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. `regression-tests/verify-issue-4.sh` 통과:
   - `customization/`이 없는 프로젝트 → 기존 산출물과 동일(무변경).
   - `customization/`이 있는 프로젝트 → 대상 인물이 등장하는 샷에만 트레이트 주입.
   - 재적용 시 멱등(블록 중복 없음).
   - `prompt.h3.txt`에 한글이 섞이지 않음.
   - `customization/` 파일이 덮어써지지 않음.
3. `scripts/breakingbad-s5e7-v2.sh` 존재 및 실행 가능(shellcheck 통과).

## 5. 구현 결과

1. **`src/customize.rs` 신규 모듈 구현**:
   - 데이터 구조체: `Character`, `CharactersFile`, `Trait`, `TraitsFile`, `ShotOverride`, `ShotCustomization`, `Block`, `Customization` (Section 3.1)
   - 핵심 API: `Customization::load`, `has_any`, `cast_for`, `blocks_for`, `apply_to_h3`, `apply_to_ko` 구현 (Section 3.1)
   - 블록 주입 및 멱등성: 마커(`<!-- rea-movie:block:begin/end -->`) 기반 교체, `apply_to_h3`는 `integrated_multimodal_description:` 바로 다음 삽입, `apply_to_ko`는 `## 6. 자동 주입 커스텀 지시` 섹션으로 주입하며 중복 없이 멱등성 보장 (Section 3.2)
   - Base 영역 보존: `<!-- rea-movie:base:begin/end -->` 마커 파싱 및 `extract_base`/`wrap_base` 구현으로 사람의 수동 편집 영역 보존 (Section 3.3)
   - 트레이트 판정: `scope`("global"/"shot") 및 `target`("character:<id>", "scene", 등) 조건 평가, 블록 순서(인물 Bible -> 트레이트 -> 샷 override) 준수 (Section 3.5)
   - 실행 지원: `run_apply` 및 `generate_show_summary` 구현 (Section 3.8)

2. **`src/timeline.rs` 수정**:
   - `TimelineShotItem`에 `#[serde(default)] pub characters: Vec<String>` 추가 (Section 3.4)

3. **`src/cli.rs` & `src/main.rs` 수정**:
   - CLI 하위 명령어 `rea-movie customize apply [--shot <id>]` 및 `rea-movie customize show` 추가 (Section 3.8)
   - CLI 인자 정규화 `known_subcommands`에 `"customize"` 등록

4. **`src/decompile/prompt.rs` 수정**:
   - 프로세스 전체 영구 템플릿 모드 유발하던 static 플래그 제거, 샷 단위 경고 출력 및 템플릿 폴백 처리 (Section 3.9)
   - `decompile` 시 기존 파일이 존재하면 `--force` 여부와 무관하게 base 영역 보존 (Section 3.3)
   - 하드코딩된 `"kbo"` / `"야구모자"` 분기 완전 삭제 (Section 3.7)
   - MiniMax-M3 API 기반 인물 캐스팅 감지(`detect_characters_with_m3`), `--mod` 영문 번역(`translate_mod_to_english_m3`), 한국어 base의 영문 H3 프롬프트 재컴파일(`translate_ko_base_to_h3_m3`) 함수 구현 (Section 3.4, 3.6, 3.7)

5. **`src/decompile/mod.rs` 수정**:
   - `customization/` 디렉토리 자동 로드 및 `--mod` 옵션을 전역 트레이트로 변환하여 통합 (Section 3.7)
   - MiniMax-M3 비전 API 기반 contact sheet 인물 캐스팅 자동 감지 및 `customization/shots/<shot_id>.toml` 우선 지정 적용 (Section 3.4)
   - `shot.toml` 및 `timeline.json`에 최종 결정된 `characters` 목록 영구 저장 (Section 3.4)
   - 각 샷 프롬프트 생성 시 커스텀 블록 주입 (Section 3.2, 3.5)

6. **`src/sync_prompt.rs` 수정**:
   - 한국어 본문을 영어 H3 프롬프트에 그대로 삽입하던 결함 수정 (Section 3.6)
   - M3 API 가용 시 영어 H3 프롬프트로 변환, API 불가 시 기존 `prompt.h3.txt`의 영어 base를 보존한 채 커스텀 블록만 갱신 (한글 미혼입 보장) (Section 3.6)
   - 하드코딩된 `"kbo"` / `"야구모자"` 분기 완전 삭제 (Section 3.7)

7. **`src/mcp.rs` 수정**:
   - MCP 도구 `apply_customization`, `show_customization` 2개 추가 (Section 3.8)
   - `tools/list` 테스트 개수 기대값 7 -> 9로 갱신 및 검증 통과 (Section 3.8)

8. **스크립트 및 회귀 테스트**:
   - `scripts/breakingbad-s5e7-v2.sh`: `customization/` 레이어(Walter White + KBO 모자 트레이트)를 활용한 2분 역공학 스크립트 작성 완료 (검수 기준 3)
   - `regression-tests/verify-issue-4.sh`: 5대 검수 항목(비커스텀 무변경, 타겟팅 주입, 멱등성, 영문 H3 보장, 읽기전용 입력 보존) 자동 검증 스크립트 작성 및 100% 통과 (검수 기준 2)

9. **코드 품질 검증**:
   - `cargo fmt --check`: 통과
   - `cargo clippy --all-targets -- -D warnings`: 경고 0건 통과
   - `cargo test`: 전체 23개 단위 테스트 통과 (검수 기준 1)
