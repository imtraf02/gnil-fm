#!/usr/bin/env bash
set -euo pipefail

project_root="${1:-.}"
review_limit=2000
warning_limit=800
failed=0

while IFS= read -r -d '' source_file; do
  relative_path="${source_file#"$project_root"/}"
  case "$source_file" in
    */crates/gnil-gpui/*|*/tests/*|*/test/*|*/generated/*|*/generated.rs|*_test.rs|*_tests.rs)
      continue
      ;;
  esac

  if head -n 5 "$source_file" | rg -qi '(@generated|generated (file|code)|do not edit)'; then
    continue
  fi

  line_count="$(wc -l < "$source_file")"
  if (( line_count > review_limit )); then
    printf '%s has %s lines (mandatory responsibility review above %s)\n' \
      "$relative_path" "$line_count" "$review_limit" >&2
    failed=1
  elif (( line_count > warning_limit )); then
    printf 'warning: %s has %s lines; consider whether it carries too many responsibilities\n' \
      "$relative_path" "$line_count" >&2
  fi
done < <(
  find "$project_root/crates" \
    -path "$project_root/crates/gnil-gpui" -prune -o \
    -type f -name '*.rs' -print0
)

exit "$failed"
