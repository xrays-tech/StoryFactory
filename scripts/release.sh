#!/usr/bin/env bash
# 发布辅助：工作区校验 → 版本一致性 → tag 提示（实际构建由 CI 矩阵完成）
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ -n "$(git status --porcelain)" ]]; then
  echo "工作区不干净，请先提交" >&2
  exit 1
fi
VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"(.*)".*/\1/')
FE_VERSION=$(node -p "require('./frontend/package.json').version" 2>/dev/null || echo "missing")
if [[ "$VERSION" != "$FE_VERSION" ]]; then
  echo "版本不一致: Rust=$VERSION frontend=$FE_VERSION" >&2
  exit 1
fi
echo "准备发布 v$VERSION"
echo "下一步：git tag v$VERSION && git push origin v$VERSION（触发 release.yml 矩阵构建）"
