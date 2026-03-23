#!/usr/bin/env bash
set -euo pipefail

# publish-public.sh
# Creates/updates the 'public' branch with only explicitly included files.
# Uses an INCLUDE list (allowlist) — new files must be added here to appear
# on GitHub. This is safer than a strip list: forgetting a file means it's
# missing on GitHub (harmless), not that internal files leak (dangerous).
#
# - First run: creates an orphan branch (no dev history)
# - Subsequent runs: appends a new commit (preserves public release history)
#
# Usage:
#   ./scripts/publish-public.sh
#
# After running, push to GitHub:
#   First release:      git push github public:main --force && git push github vX.Y.Z
#   Subsequent releases: git push github public:main && git push github vX.Y.Z

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_DIR"

# --- Files/dirs INCLUDED in the public branch (allowlist) ---
# Everything else is excluded by default. Add new public files here.
INCLUDE=(
  "src/"
  "scripts/"
  ".github/"
  "package.json"
  "package-lock.json"
  "tsconfig.json"
  "forge.config.ts"
  "webpack.main.config.ts"
  "webpack.renderer.config.ts"
  "webpack.rules.ts"
  "webpack.plugins.ts"
  "README.md"
  "CHANGELOG.md"
  "LICENSE"
  ".gitignore"
)

# --- Guard: abort if working tree is dirty ---
if ! git diff --quiet || ! git diff --cached --quiet; then
  echo "ERROR: Working tree has uncommitted changes. Please commit or stash before publishing." >&2
  exit 1
fi

# --- Resolve version and current branch ---
VERSION=$(node -p "require('./package.json').version")
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
TAG="v$VERSION"

echo "==> Publishing public branch for $TAG (from $CURRENT_BRANCH)"

# --- Snapshot current HEAD into a temp dir via git archive ---
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

echo "    Exporting clean snapshot..."
git archive HEAD | tar -x -C "$TMPDIR"

# --- Build a second temp dir with only included files ---
PUBDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR" "$PUBDIR"' EXIT

echo "    Selecting public files..."
for item in "${INCLUDE[@]}"; do
  src="$TMPDIR/$item"
  if [ -e "$src" ]; then
    # Preserve directory structure
    dest="$PUBDIR/$item"
    if [ -d "$src" ]; then
      mkdir -p "$dest"
      cp -r "$src/." "$dest/"
    else
      mkdir -p "$(dirname "$dest")"
      cp "$src" "$dest"
    fi
    echo "      included: $item"
  else
    echo "      WARNING: $item not found in HEAD (skipped)"
  fi
done

# --- Create or append to public branch ---
if git show-ref --verify --quiet refs/heads/public; then
  echo "    Appending to existing public branch..."
  git checkout public
else
  echo "    Creating new orphan public branch..."
  git checkout --orphan public
fi

# Remove all tracked files AND untracked files (clean slate)
git rm -rf . --quiet
git clean -fd --quiet

# --- Copy only included files into working tree ---
cp -r "$PUBDIR/." .

# --- Extract CHANGELOG entry for this version ---
CHANGELOG_BODY=$(awk "/^## \[$VERSION\]/{found=1; next} found && /^## \[/{exit} found{print}" CHANGELOG.md | sed '/./,$!d' | sed -e :a -e '/^\n*$/{$d;N;ba}')

# --- Commit (skip if nothing changed) ---
# Use GitHub identity so public branch commits link to lmeinel on GitHub
GITHUB_NAME="Lars Meinel"
GITHUB_EMAIL="lmeinel@gmail.com"

git add .
if git diff --cached --quiet; then
  echo "    Nothing changed since last publish — re-tagging existing commit."
else
  if [ -n "$CHANGELOG_BODY" ]; then
    GIT_AUTHOR_NAME="$GITHUB_NAME" GIT_AUTHOR_EMAIL="$GITHUB_EMAIL" \
    GIT_COMMITTER_NAME="$GITHUB_NAME" GIT_COMMITTER_EMAIL="$GITHUB_EMAIL" \
    git commit -m "Release $TAG" -m "$CHANGELOG_BODY"
  else
    GIT_AUTHOR_NAME="$GITHUB_NAME" GIT_AUTHOR_EMAIL="$GITHUB_EMAIL" \
    GIT_COMMITTER_NAME="$GITHUB_NAME" GIT_COMMITTER_EMAIL="$GITHUB_EMAIL" \
    git commit -m "Release $TAG"
  fi
fi

# --- Tag the public commit ---
git tag -d "$TAG" 2>/dev/null && echo "      (replaced existing tag $TAG)" || true
git tag "$TAG"
echo "    Tagged commit as $TAG"

# --- Return to original branch ---
git checkout "$CURRENT_BRANCH"

echo ""
echo "==> Done. Branch 'public' is ready at $TAG."
echo ""
echo "    Push to GitHub (first release use --force, subsequent releases omit it):"
echo "      git push github public:main"
echo "      git push github $TAG"
