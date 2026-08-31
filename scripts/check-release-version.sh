#!/usr/bin/env bash
# 校验发布版本号与两个权威版本文件一致（Cargo.toml + web/package.json）。
# 本地发版预检与 Release CI（.github/workflows/release.yml）共用，保证打 v* tag 时版本已同步。
#
# 用法: bash scripts/check-release-version.sh 1.2.0   （不带 v 前缀）
# 退出码: 0 = 一致; 1 = 非法版本号或任一文件不一致
set -euo pipefail

EXPECTED="$1"

if ! [[ "$EXPECTED" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "非法版本号: $EXPECTED （应为 X.Y.Z，例如 1.2.0，不带 v 前缀）" >&2
  exit 1
fi

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

CARGO_VERSION=$(grep -m1 '^version = ' "$ROOT/Cargo.toml" | sed 's/.*"\(.*\)".*/\1/')
NPM_VERSION=$(node -p "require('$ROOT/web/package.json').version")

FAIL=0

if [ "$CARGO_VERSION" != "$EXPECTED" ]; then
  echo "Cargo.toml version=$CARGO_VERSION != $EXPECTED" >&2
  FAIL=1
fi

if [ "$NPM_VERSION" != "$EXPECTED" ]; then
  echo "web/package.json version=$NPM_VERSION != $EXPECTED" >&2
  FAIL=1
fi

if [ "$FAIL" -ne 0 ]; then
  echo "发布前请先同步修改 Cargo.toml 与 web/package.json 的 version 为 $EXPECTED" >&2
  exit 1
fi

echo "OK: Cargo.toml 与 web/package.json 版本一致 ($EXPECTED)"
