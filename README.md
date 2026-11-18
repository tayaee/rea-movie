# REA-Movie 

REA-Movie (Reverse Engineer Any Movie) - reverse-engineer any movie into an editable shot-by-shot project and re-render it with MiniMax-H3.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/tayaee/rea-movie/main/install.sh | sh
rea-movie --version
```

## Requirements

- H3 render nodes: 1-4 machines running the H3 server. First-time setup asks for them:
  `rea-movie pool --proj-dir <proj> setup` (or `--nodes h1,h2`). Passwordless SSH is
  required (`ssh <node>` must work with no password prompt). Nodes and base port are
  stored per-project in `rea-movie.toml` under `[pool]` (`nodes`, `base_port`).
  Alternatively, point directly at an H3 server with `REA_MOVIE_H3_ENDPOINT=http://<host>:<port>`
  (optional: `REA_MOVIE_H3_ENGINE=fl2va|ref2va`).
- Screenplay / prompt generation: `export MINIMAX_API_KEY=...` (MiniMax-M3 vision model) must be set. Without it, prompts fall back to a local template instead of AI-generated direction.

## Default paths (zero config)

- Projects (git-tracked, lightweight): `~/rea-movie-artifacts/h3-projects/<name>` (override with `-o` or `REA_MOVIE_PROJECTS`)
- Render outputs (heavy video): `~/rea-movie-artifacts/h3-results/<name>` (sibling `../h3-results/`, resolved automatically)
- Pool workspace (serve-h3.sh, node-state): `~/rea-movie-artifacts` (override with `[pool] remote_workspace` or `REA_MOVIE_WORKSPACE`)
- On machines with a shared NAS, point at it with a single symlink: `ln -s /rosenas/data/rea-movie-artifacts ~/rea-movie-artifacts`.

## Usage (via Claude)

You don't run `rea-movie` directly. Just ask Claude to download, decompile, edit, and reassemble:

```bash
claude "Download <youtube_url> and decompile it with rea-movie, then apply the changes below and reassemble it.
- Change 1: replace the background with a neon city at night
- Change 2: change the dialogue to 'We have to go now.'
- Change 3: render in full quality and give me the compare video"
```

Claude handles the whole flow: download → `rea-movie decompile` → edit `shots/shot_XXXX/prompt.ko.md` → `render` → `assemble`.

One-line request agents handle autonomously (see `skills/rea-movie/SKILL.md` §8 playbook):

```bash
claude "Reverse-engineer <youtube_url/mp4> into <dir>"
# Agent protocol: ① input-details/ cache + series-watch → ② scene-bible axis/A-B-A check →
# ③ <Picture 1>→<Picture 2> delta + on/off-screen check → ④ smoke render & assemble →
# ⑤ _compare.mp4 + 4-point set (ko prompt + h3 prompt + render + first_frame) review
```

## Features (Issues 8–11)

### `input-details/` permanent cache + `series-watch` pre-analysis (Issue 8)

- Reusable assets live once under `<root>/input-details/<source_id>/`:
  `video.mp4`, `probe.json`, `scene_cuts.json`, `subtitles/`, `keyframes/`,
  `shots_cache/<start>_<end>/{original_clip.mp4,original_audio.wav,contact_sheet.jpg}`.
  Repeat decompiles skip slow `select='gt(scene,0.14)'` detection and copy shot assets atomically.
- `source_id`: YouTube → `yt-<11-char-id>`; local file → `file-<FNV-1a(size+head/tail 1MiB)>`;
  indexed in `input-details/index.json` (pure input videos in `<root>/input/`; legacy `<root>/sources/` and `<root>/source/` auto-detected).
- `rea-movie series-watch --out-root <root> [episodes...]` (also MCP tool `series_watch`)
  pre-analyzes whole series (ffprobe + 5s keyframes + subtitle parsing) into
  `<root>/story/{world.json,characters.json,timeline-days.md}` — deterministic offline
  default, one-shot MiniMax-M3 summary when `MINIMAX_API_KEY` is set.
- `scripts/reverse-url.sh` ties it together: `input-details/yt-<id>/` caching with yt-dlp
  skip, `.vtt`→`.srt` skip-if-exists, `series-watch` first for playlists,
  `--episodes 1,2` / `1-3` selective decompile.

### H3 anchor-preserving retime + prompt pipeline (Issue 9)

- `--min-shot-duration <sec>` (default `0.0` = no merging) merges micro-cuts
  with a neighbor within `max-shot-duration`.
- Assemble no longer truncates sub-4.0s render clips: the full render range is
  retimed to `target_dur` (`setpts` video + `atempo` audio chain, `retime_render_clip`),
  preserving the `last_frame` convergence the old `-t` cut destroyed.
- M3 shot generation receives `<Picture 1>` (first frame) + `<Picture 2>` (last frame)
  multimodal inputs with Delta-trajectory direction, plus scene-bible and
  series-world summaries; watermark/timestamp/hardsub depiction is banned by Negative rule.
- `prompt.h3.txt` carries an anchor-matched `reference_line` header; HTML comment
  markers (`<!-- rea-movie:... -->`) are stripped only for H3 transmission,
  and the offline `compile_ko_to_h3` fallback keeps non-dialogue sections English-only.

### Pro director grammar (Issue 10)

- A-B-A Shot/Reverse-Shot awareness: `group_scenes_with_lookback()` keeps
  `A → B → A → B` cross-edits in one scene when the 2-hop/3-hop lookback histogram
  distance ≤ `0.28`, while true location moves and silence/cap splits still split.
- On-screen vs off-screen delivery (`delivery_mode`: `on_screen` | `off_screen` |
  `reaction` | `silent`): `[off]`/`(VO)`/`(내레이션)` markers or a speaker tag absent
  from the shot's on-screen characters → reactor stays silent with closed lips
  (`no lip movement`) while the voice speaks off-screen — never lip-syncs чужой lines.
- Scene bibles carry the 180° axis (`axis_of_action`, e.g. `S1 on screen-left looking
  right`) and `camera_grammar` (shot sizes, OTS/reverse-shot coverage), injected into
  both KO (`- 180도 축:`) and H3 (`Axis of action.`) scene blocks.

### Verbatim dialogue + 6-language subs + original-audio default (Issue 11)

- Dialogue is reproduced **verbatim by default**; `--translate-dub` restores the legacy
  KO dubbing path. `<d>[Language]</d>` tags always match the body via
  `detect_dialogue_language()` (Hangul→Korean, kana→Japanese, hanzi→Chinese,
  ß/äöü→German, é/è/ê→French, Latin→English).
- `assemble` writes six external subtitles `<movie>_<profile>.{ko,zh,ja,en,fr,de}.srt`
  plus always-on `<movie>_<profile>.proof.srt` (M3 batch translation + cache in
  `story/subtitles_cache/`, deterministic fallback offline).
- Final movie keeps dual audio H3 + Original with **Original as the default track**;
  `assemble --proof` additionally builds `<movie>_<profile>_proof.mp4` with
  burned-in `[Speaker] line` subtitles.

## CLI quick reference

```bash
rea-movie decompile in.mp4 -o proj --offline [--range 0-60] [--min-shot-duration 1.0] [--translate-dub]
rea-movie series-watch --out-root <root> ep1.mp4 ep2.mp4 [--offline]
rea-movie render --proj-dir proj --smoke --mock-h3   # no-GPU pipeline check
rea-movie assemble --proj-dir proj --smoke [--offline] [--proof]
./scripts/reverse-url.sh "<youtube-url>" ~/rea-movie-artifacts [--episodes 1,2] [--offline]
```

## Regression tests

```bash
./regression-tests/verify-issue-12.sh   # skill/README docs + one-line E2E (series-watch → decompile → mock-h3 → proof assemble)
cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test
```
