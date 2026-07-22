#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
exec "$root_dir/lib/@dynamic_linker@" \
  --library-path "$root_dir/lib" \
  "$root_dir/libexec/gnil-fm" "$@"
