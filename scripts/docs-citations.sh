#!/usr/bin/env bash
# Verifies that the living docs stay truthful:
#   1. every `path/to/file.ext:NN` citation points at an existing file and line
#   2. every relative markdown link target exists
# Exits non-zero if any stale reference is found.

cd "$(dirname "$0")/.." || exit 1

DOCS=(README.md TODO_LIST.md FEATURES.md AGENTS.md ROADMAP.md CHANGELOG.md)

status=0

check_citations() {
  local doc="$1"
  local refs
  refs=$(grep -oE '[a-zA-Z0-9_./-]+\.(rs|nix|toml|md|json|yml|sh):[0-9]+' "$doc" 2>/dev/null | sort -u) || refs=""
  [ -z "$refs" ] && return 0
  local ref path line total
  while read -r ref; do
    path="${ref%:*}"
    line="${ref##*:}"
    if [ ! -f "$path" ]; then
      echo "STALE FILE in $doc: $ref"
      status=1
      continue
    fi
    total=$(wc -l < "$path")
    if [ "$line" -gt "$total" ]; then
      echo "STALE LINE in $doc: $ref (file has $total lines)"
      status=1
    fi
  done <<< "$refs"
}

check_links() {
  local doc="$1"
  local targets
  targets=$(grep -oE '\]\([^)]+\)' "$doc" 2>/dev/null | sed -E 's/^\]\(//; s/\)$//' | grep -vE '^(https?:|mailto:|#)' | sort -u) || targets=""
  [ -z "$targets" ] && return 0
  local target path
  while read -r target; do
    path="${target%%#*}"
    [ -n "$path" ] || continue
    if [ ! -e "$path" ]; then
      echo "BROKEN LINK in $doc: $target"
      status=1
    fi
  done <<< "$targets"
}

for doc in "${DOCS[@]}"; do
  [ -f "$doc" ] || continue
  check_citations "$doc"
  check_links "$doc"
done

if [ "$status" -ne 0 ]; then
  echo "docs-citations: FAILED (stale references above)"
  exit 1
fi
echo "docs-citations: all citations and links resolve"
