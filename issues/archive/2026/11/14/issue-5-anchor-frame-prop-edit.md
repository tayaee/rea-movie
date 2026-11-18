# Issue 5: 앵커 프레임 소품 편집 — 인물의 머리 위에 KBO 모자 합성

## 1. 배경 및 문제

Issue 4에서 프롬프트 레벨 트레이트 주입(KBO 모자 착용)을 구현했으나, **실제로는 렌더에 반영되지 않는다.**

실증(v1 프로젝트 `breakingbad-s5e7`, `--mod`로 KBO 문구를 넣고 smoke 렌더 완료 상태):

| 샷 | 원본 | 렌더 결과 |
| --- | --- | --- |
| shot_0022 | 월터 대머리 클로즈업 | **모자 없음** (얼굴만 원본과 유사) |
| shot_0025 | 월터 대머리 | **모자 없음** (머리카락이 뾰족하게 변하는 모핑 artifact) |

원인: `fl2va`는 **레퍼런스 프레임(first/last frame)을 최우선으로 고정**한다.
프롬프트에 "wear a KBO cap"을 넣어도 앵커 프레임에 대머리가 있으면 모델은 앵커를 따른다.

따라서 "walter에게만 KBO 야구모자를 항상"을 실현하려면 **앵커 프레임 자체를 편집**해야 한다.

## 2. 목표

`customization/characters.toml`의 인물 정의에 소품(`cap_ref`)을 지정하면,
그 인물이 등장하는 샷의 앵커 프레임(`first_frame.jpg`, `last_frame.jpg`)에
소품 이미지를 머리 위치에 합성하고, 원본 프레임은 `*.orig.jpg`로 보존한다.
이후 H3 렌더는 "모자를 쓴 월터"를 앵커로부터 자연스럽게 이어받는다.

## 3. 상세 요구사항

### 3.1 설정 (`customization/characters.toml`)

```toml
[characters.walter]
name_en = "Walter White"
appearance_en = "a completely bald middle-aged man with a goatee"
cap_ref = "assets/props/kbo_cap.png"   # 프로젝트 기준 상대 경로(투명 PNG)
cap_scale = 1.30                        # 캡 폭 = 머리 폭 * cap_scale (기본 1.25)
cap_offset = -0.05                      # 캵 수직 오프셋(머리 높이 대비, 기본 -0.08)
```

### 3.2 CLI

```
rea-movie customize anchor --proj-dir . [--shot 0015] [--character walter] [--force]
```

- `customization/characters.toml`에 `cap_ref`가 있는 인물을 대상으로만 동작.
- 이미 편집된 프레임은 `--force`가 없으면 건너뛴다(멱등).
  편집 이력을 `shots/shot_XXXX/refs/anchor_edit.json`에 기록한다:
  ```json
  {"frame": "first_frame.jpg", "character": "walter", "bbox": [0.263, 0.079, 0.206, 0.519],
   "prop": "assets/props/kbo_cap.png", "applied_at": "<ISO8601>"}
  ```
- `original_frame`은 `refs/first_frame.orig.jpg`로 보존(원본이 이미 있으면 유지).

### 3.3 머리 위치 검출 (MiniMax-M3 비전)

- 앵커 프레임 이미지를 M3에 전송하고 **정규화 좌표 bbox**를 JSON으로 받는다.
  요청 문구(구현 기준):
  > Locate the head of {appearance_en}. Output ONLY JSON
  > {"head": {"x": <0-1 left>, "y": <0-1 top>, "w": <0-1 width>, "h": <0-1 height>}}
  > describing the bounding box of the HEAD ONLY (skull, not shoulders).
  > If the character is not present, output {"head": null}.
- 응답 파싱은 `<think>...</think>` 블록을 제거한 뒤 JSON을 추출해야 한다(기존 프롬프트 API는
  `<think>` 블록 없이 파싱해 실패한다 — **이 파서를 공용으로 만들어라**).
- 검증: `0<=x<=1`, `0<=y<=1`, `0.02<=w<=0.9`, `0.02<=h<=0.9`, `x+w<=1.02`, `y+h<=1.02`.
  검증 실패 또는 `null` → 해당 프레임은 편집하지 않고 경고 출력 후 계속.
- API 키가 없거나 호출 실패 → 전체 건너뛰고 경고 1회 출력(다른 샷 계속).

### 3.4 합성 규칙 (Rust `image` crate)

1. 프레임 JPEG를 RGBA8로 디코드.
2. `cap_ref` PNG를 RGBA8로 디코드하고 **투명 영역 바운딩박스를 계산**한 뒤 그 부분만 사용.
3. 목표 폭 `cap_w = head_w_px * cap_scale`, 목표 높이는 캡 이미지의 종횡비를 유지.
4. 배치 좌표:
   - 캡 중심 x = 머리 bbox의 중심 x
   - 캡의 **아래쪽(브림)**이 머리 상단이 되도록: 캡 윗끝 y = head_top + head_h * cap_offset
     (`cap_offset` 기본 -0.08 → 캡이 두개골 위쪽에 얹힌다)
   - 좌우가 프레임 밖이면 잘라낸다(음수 좌표 금지).
5. 알파 합성: `out = cap.rgb * a + base.rgb * (1 - a)` (수동 구현, `imageops::overlay`에 의존하지 않는다).
6. 결과는 JPEG(q=2)로 `refs/first_frame.jpg`에 저장.

### 3.5 안전장치

- 원본 프레임은 절대 덮어쓰지 않는다(`*.orig.jpg` 보존).
- `customization/` 파일은 읽기 전용.
- 앵커 편집은 **프롬프트와 별개**이며, `customize apply`와 독립 실행 가능.

## 4. 검수 기준

1. `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test` 통과.
2. `regression-tests/verify-issue-5.sh` 통과:
   - 앵커 편집이 프레임 크기를 바꾸지 않는다.
   - 편집된 프레임과 원본 프레임의 픽셀이 다르다(실제로 합성됨).
   - 합성 영역에 색상 변화(빨강/남색 픽셀)가 생긴다.
   - 재실행 시 멱등(파일 해시 불변).
   - `cap_ref`가 없는 인물/프로젝트에서는 아무것도 하지 않는다.
3. v2 프로젝트(`breakingbad-s5e7-v2`)에서 월터 등장 샷의 앵커에 모자가 합성되고,
   원본 프레임이 `.orig.jpg`로 보존된다.

## 5. 구현 결과

1. **신규 모듈 `src/anchor.rs` 구현**:
   - `BBox` 구조체 및 유효성 검증(`0<=x,y<=1`, `0.02<=w,h<=0.9`, `x+w,y+h<=1.02`).
   - `AnchorEditRecord` 및 `anchor_edit.json` 직렬화/역직렬화 (단일 객체 및 배열 형식 완전 지원).
   - 투명도 바운딩 박스 트리밍(`trim_transparent`) 및 종횡비 유지 리사이징.
   - 배치 좌표 계산(캡 중심 x = 머리 bbox 중심 x, 캡 윗끝 y = head_top + head_h * cap_offset).
   - 수동 알파 블렌딩 합성(`out = cap.rgb * a + base.rgb * (1 - a)`).
   - 원본 프레임 `*.orig.jpg` 백업 및 보존, 최고 품질 JPEG(q=98) 저장.
   - `run_anchor_edit` 메인 엔트리포인트 및 멱등성 보장(`--force` 지원).

2. **MiniMax-M3 공용 JSON 파서 구현 (`src/decompile/prompt.rs`)**:
   - `<think>...</think>` 추론 블록 제거 함수 `strip_think_blocks` 구현.
   - 마크다운 코드 블록 및 최외곽 JSON 슬라이스 추출 함수 `extract_json_slice` 구현.
   - 제네릭 `parse_llm_json<T>` 공용 파서를 신설하여 기존 프롬프트 API(`try_minimax_m3_api`, `detect_characters_with_m3`, `translate_ko_base_to_h3_m3`)와 앵커 머리 검출 API(`detect_head_bbox`)가 모두 공유하도록 리팩터링.

3. **`customization/characters.toml` 및 설정 확장 (`src/customize.rs`)**:
   - `Character` 구조체에 `cap_ref`, `cap_scale`, `cap_offset` 필드 및 기본값 헬퍼 메서드(`cap_scale()` -> 1.25, `cap_offset()` -> -0.08) 추가.

4. **CLI 및 MCP 도구 확장**:
   - CLI: `rea-movie customize anchor --proj-dir . [--shot <id>] [--character <id>] [--force]` 추가.
   - MCP: `customize_anchor` 도구 정의 및 실행 핸들러 구현.

5. **단위 테스트 및 회귀 테스트 통과**:
   - `cargo test`: 33개 단위 테스트 모두 통과.
   - `cargo clippy --all-targets -- -D warnings`: 경고 0건 통과.
   - `cargo fmt --check`: 포맷팅 통과.
   - `regression-tests/verify-issue-5.sh`:
     - 프레임 크기 불변 확인
     - 원본 프레임(`*.orig.jpg`) 보존 및 픽셀 상이 확인
     - 합성 영역 빨강/남색 색상 변화 검출 확인
     - 재실행 시 멱등성(파일 해시 불변) 확인
     - `cap_ref` 미지정 인물/프로젝트 무변형 확인
     - `--shot` 필터링 및 MCP 도구 작동 확인
     모두 PASS.
