#!/usr/bin/env bash
# 启用 git hooks：把 hooks 目录指到仓库内 .githooks/（随提交走，新 clone 后执行一次即可）。
# 用法：bash scripts/install-hooks.sh
set -euo pipefail

cd "$(dirname "$0")/.." # → suishouji/

git config core.hooksPath .githooks
chmod +x .githooks/pre-commit 2>/dev/null || true

echo "✓ 已启用 git hooks（core.hooksPath=$(git config --get core.hooksPath)）"
