#!/usr/bin/env bash
set -euo pipefail

project_root="${1:-.}"
default_limit=2000
ui_limit=600
failed=0

while IFS= read -r -d '' source_file; do
  limit="$default_limit"
  case "$source_file" in
    */gnil-app/src/file_manager/view_*.rs|*/gnil-app/src/picker/view.rs|*/gnil-app/src/ui/*.rs)
      limit="$ui_limit"
      ;;
  esac
  line_count="$(wc -l < "$source_file")"
  if (( line_count > limit )); then
    printf '%s has %s lines (limit: %s)\n' \
      "${source_file#"$project_root"/}" "$line_count" "$limit" >&2
    failed=1
  fi
done < <(find "$project_root/crates" -type f -name '*.rs' -print0)

exit "$failed"
