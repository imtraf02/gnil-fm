#!/bin/sh
set -eu

root_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
program=${0##*/}
case "$program" in
  gnil-fm|gnil-fm-portal) ;;
  *)
    echo "unsupported gnil-fm launcher name: $program" >&2
    exit 64
    ;;
esac
exec "$root_dir/lib/@dynamic_linker@" \
  --library-path "$root_dir/lib" \
  "$root_dir/libexec/$program" "$@"
