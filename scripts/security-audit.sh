#!/usr/bin/env bash
set -euo pipefail

readonly quick_xml_version="quick-xml@0.39.4"
readonly expected_parent="1wayland-scanner v0.31.10 (proc-macro)"

dependency_tree="$(cargo tree -i "$quick_xml_version" --prefix depth)"
depth_one="$(sed -n '/^1/p' <<<"$dependency_tree")"
if [[ "$depth_one" != "$expected_parent" ]]; then
  echo "Refusing the quick-xml RustSec exception: its dependency path changed." >&2
  echo "$dependency_tree" >&2
  exit 1
fi

# wayland-scanner only parses the protocol XML bundled with dependencies at compile time. The
# application-facing quick-xml dependency is >=0.41.0. Keep these exceptions coupled to the
# dependency-tree guard above so a new runtime path fails this script.
cargo audit \
  --ignore RUSTSEC-2026-0194 \
  --ignore RUSTSEC-2026-0195

if [[ "${1:-}" == "--native" ]]; then
  command -v syft >/dev/null
  command -v grype >/dev/null
  audit_root="$PWD"
  temporary_root=""
  sbom=""
  cleanup() {
    if [[ -n "$temporary_root" ]]; then
      rm -rf -- "$temporary_root"
    fi
    if [[ -n "$sbom" ]]; then
      rm -f -- "$sbom"
    fi
  }
  trap cleanup EXIT
  if [[ -n "$(git ls-files --others --exclude-standard)" ]]; then
    temporary_root="$(mktemp -d)"
    git ls-files --cached --others --exclude-standard -z \
      | tar --null --ignore-failed-read --files-from=- --create --file=- \
      | tar --extract --file=- --directory="$temporary_root"
    audit_root="$temporary_root"
  fi
  package="$(nix build "path:$audit_root#default" --no-link --print-out-paths)"
  sbom="$(mktemp --suffix=.cdx.json)"
  syft "dir:$package" -o "cyclonedx-json=$sbom"
  grype "sbom:$sbom" --fail-on high
fi
