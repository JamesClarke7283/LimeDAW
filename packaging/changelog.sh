#!/usr/bin/env bash
# Print a markdown changelog for a release.
#
#   packaging/changelog.sh              every commit since the previous tag
#   packaging/changelog.sh v0.1.0       every commit since the tag before v0.1.0
#
# Commits are grouped by the conventional-commit prefix when there is one, and
# listed plainly when there is not, so the output reads well either way.
set -euo pipefail

target="${1:-HEAD}"
previous="$(git describe --tags --match 'v[0-9]*' --abbrev=0 "${target}^" 2>/dev/null || true)"

if [ -n "$previous" ]; then
    range="${previous}..${target}"
    echo "### Changes since ${previous}"
else
    range="$target"
    echo "### Changes"
fi
echo

emit() {
    local pattern="$1" heading="$2" found=""
    while IFS= read -r line; do
        [ -z "$line" ] && continue
        if [ -z "$found" ]; then
            printf '\n**%s**\n\n' "$heading"
            found=yes
        fi
        printf -- '- %s\n' "$line"
    done < <(git log --no-merges --pretty=format:'%s (%h)' "$range" | grep -iE "$pattern" | sed -E 's/^[a-zA-Z]+(\([^)]*\))?!?: //')
}

emit '^feat' 'Added'
emit '^fix' 'Fixed'
emit '^perf' 'Performance'
emit '^(docs|doc)' 'Documentation'
emit '^(refactor|style|chore|build|ci|test)' 'Internal'

# Anything that did not use a conventional prefix still deserves listing.
uncategorised="$(git log --no-merges --pretty=format:'%s (%h)' "$range" \
    | grep -ivE '^(feat|fix|perf|docs|doc|refactor|style|chore|build|ci|test)(\([^)]*\))?!?:' || true)"
if [ -n "$uncategorised" ]; then
    printf '\n**Other**\n\n'
    while IFS= read -r line; do
        [ -n "$line" ] && printf -- '- %s\n' "$line"
    done <<< "$uncategorised"
fi

printf '\n---\n\n%s commits by %s contributors.\n' \
    "$(git rev-list --no-merges --count "$range")" \
    "$(git log --no-merges --pretty=format:'%an' "$range" | sort -u | wc -l | tr -d ' ')"
