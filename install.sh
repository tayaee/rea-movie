#!/bin/sh
# rea-movie standard installer — Linux / WSL2. No toolchain needed.
# Usage: curl -fsSL https://raw.githubusercontent.com/tayaee/rea-movie/main/install.sh | sh
set -eu
REPO="tayaee/rea-movie"
BASE="https://github.com/${REPO}/releases/latest/download"
BIN_DIR="${HOME}/.local/bin"
SKILL_DIR="${HOME}/.claude/skills/rea-movie"

mkdir -p "${BIN_DIR}" "${SKILL_DIR}"
echo "[rea-movie] downloading binary..."
curl -fsSL "${BASE}/rea-movie-linux" -o "${BIN_DIR}/rea-movie"
chmod +x "${BIN_DIR}/rea-movie"
echo "[rea-movie] downloading skill..."
curl -fsSL "${BASE}/rea-movie-skill.zip" -o /tmp/rea-movie-skill.zip
unzip -oq /tmp/rea-movie-skill.zip -d "${SKILL_DIR}"
rm -f /tmp/rea-movie-skill.zip
echo "[rea-movie] installed: ${BIN_DIR}/rea-movie + ${SKILL_DIR}/"
echo "[rea-movie] ensure ${BIN_DIR} is on PATH, then run: rea-movie pool status"
"${BIN_DIR}/rea-movie" pool status
