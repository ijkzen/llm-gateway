#!/bin/sh

set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
. "${script_dir}/sccache-env.sh"

if command -v sccache >/dev/null 2>&1; then
  exec sccache "$@"
fi

echo "warning: sccache not found in PATH, falling back to direct rustc invocation" >&2
exec "$@"
