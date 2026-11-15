# REA-Movie 

REA-Movie (Reverse Engineer Any Movie) - reverse-engineer any movie into an editable shot-by-shot project and re-render it with MiniMax-H3.

## Install

```bash
curl -fsSL https://raw.githubusercontent.com/tayaee/rea-movie/main/install.sh | sh
rea-movie --version
```

## Requirements

- H3 render node: passwordless SSH access is required (`ssh spark1.local` must work with no password prompt). Nodes and base port are set in `rea-movie.toml` under `[pool]` (`nodes`, `base_port`). Alternatively, point directly at an H3 server with `REA_MOVIE_H3_ENDPOINT=http://<host>:<port>` (optional: `REA_MOVIE_H3_ENGINE=fl2va|ref2va`).
- Screenplay / prompt generation: `export MINIMAX_API_KEY=...` (MiniMax-M3 vision model) must be set. Without it, prompts fall back to a local template instead of AI-generated direction.

## Default paths (zero config)

- Projects: `~/rea-movie-projects/<name>` (override with `-o` or `REA_MOVIE_PROJECTS`)
- Render workspace: `~/rea-movie-workspace` (override with `[pool] remote_workspace` or `REA_MOVIE_WORKSPACE`)
- On machines with a shared NAS, point both at it with symlinks: `ln -s /rosenas/data/rea-movie-projects ~/rea-movie-projects` and `ln -s /rosenas/data/rea-movie-workspace ~/rea-movie-workspace`.

## Usage (via Claude)

You don't run `rea-movie` directly. Just ask Claude to download, decompile, edit, and reassemble:

```bash
claude "Download <youtube_url> and decompile it with rea-movie, then apply the changes below and reassemble it.
- Change 1: replace the background with a neon city at night
- Change 2: change the dialogue to 'We have to go now.'
- Change 3: render in full quality and give me the compare video"
```

Claude handles the whole flow: download → `rea-movie decompile` → edit `shots/shot_XXXX/prompt.ko.md` → `render` → `assemble`.
