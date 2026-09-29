#!/usr/bin/env bash
set -euo pipefail

# publish-public.sh
# Adds a commit to the 'public' branch that contains only the files on the
# allowlist below, taken from HEAD. The 'public' branch is pushed to GitHub's
# main; GitHub never sees the development history.
#
# Allowlist, not strip list: forgetting a file means it's missing on GitHub
# (harmless), not that internal files leak (dangerous).
#
# The working tree is never touched: the commit is built from a temporary
# index. (An earlier version checked out 'public' and ran `git rm -rf .` plus
# `git clean -fd`, which also deleted ignored files such as node_modules/,
# target/ and references/*.pdf.)
#
# If there is no local 'public' branch, it is created from github/main, so a
# fresh clone continues the public history instead of starting a new one.
#
# Usage:
#   ./scripts/publish-public.sh
#
# Then push (see the output for the exact commands):
#   git push github public:main       # always
#   git push github vX.Y.Z            # only for a release: starts the release build
#
# Environment:
#   PUBLIC_AUTHOR_NAME / PUBLIC_AUTHOR_EMAIL   author of the public commit
#   GITHUB_REMOTE (default: github)
#   GIT_SSH_COMMAND to push/fetch with a deploy key instead of your own SSH key

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

cd "$PROJECT_DIR"

# --- Files/dirs INCLUDED in the public branch (allowlist) ---
# Everything else is excluded by default. Add new public files here.
INCLUDE=(
  "app/"
  "cli/"
  "lib/"
  "protocol/"
  "ui/"
  "scripts/"
  ".github/"
  "app-icon.png"
  "package.json"
  "package-lock.json"
  "Cargo.toml"
  "Cargo.lock"
  "rustfmt.toml"
  ".nvmrc"
  "eslint.config.mjs"
  "README.md"
  "CHANGELOG.md"
  "LICENSE"
  ".gitignore"
)

# Identity for the public commit, so it links to the right GitHub account
AUTHOR_NAME="${PUBLIC_AUTHOR_NAME:-Lars Meinel}"
AUTHOR_EMAIL="${PUBLIC_AUTHOR_EMAIL:-lmeinel@gmail.com}"
REMOTE="${GITHUB_REMOTE:-github}"
REMOTE_BRANCH="main"
PUBLIC_REF="refs/heads/public"

die() {
  echo "ERROR: $*" >&2
  exit 1
}

# --- Guard: only committed changes are published ---
if ! git diff --quiet || ! git diff --cached --quiet; then
  die "Working tree has uncommitted changes. Commit or stash them first (only HEAD is published)."
fi

VERSION=$(node -p "require('./package.json').version")
TAG="v$VERSION"
echo "==> Publishing HEAD of $(git rev-parse --abbrev-ref HEAD) ($(git rev-parse --short HEAD)) for $TAG"

# --- Make sure 'public' continues GitHub's history ---
git remote get-url "$REMOTE" >/dev/null 2>&1 \
  || die "No remote '$REMOTE'. Add it first: git remote add $REMOTE git@github.com:3dvisionlabs/ecm-discovery-tool.git"

echo "    Fetching $REMOTE/$REMOTE_BRANCH..."
git fetch --quiet "$REMOTE" "$REMOTE_BRANCH" --tags \
  || die "Cannot fetch from '$REMOTE'. Check access (GIT_SSH_COMMAND with the deploy key?)."
REMOTE_HEAD=$(git rev-parse --verify "refs/remotes/$REMOTE/$REMOTE_BRANCH")

if ! git show-ref --verify --quiet "$PUBLIC_REF"; then
  git branch public "$REMOTE_HEAD"
  echo "    Created local 'public' from $REMOTE/$REMOTE_BRANCH"
elif ! git merge-base --is-ancestor "$REMOTE_HEAD" public; then
  die "Local 'public' does not contain $REMOTE/$REMOTE_BRANCH (published from another machine?).
       If 'public' has nothing you still need: git branch -f public $REMOTE/$REMOTE_BRANCH"
fi

# --- Collect the allowlisted files from HEAD ---
WORKDIR=$(mktemp -d)
trap 'rm -rf "$WORKDIR"' EXIT
SNAPSHOT="$WORKDIR/snapshot"
PUBDIR="$WORKDIR/public"
mkdir -p "$SNAPSHOT" "$PUBDIR"

git archive HEAD | tar -x -C "$SNAPSHOT"

echo "    Selecting public files..."
for item in "${INCLUDE[@]}"; do
  src="$SNAPSHOT/$item"
  if [ -e "$src" ]; then
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

# --- Build the commit from a temporary index (working tree stays untouched) ---
export GIT_INDEX_FILE="$WORKDIR/index"
git --work-tree="$PUBDIR" add --all --force .
TREE=$(git write-tree)
unset GIT_INDEX_FILE

PARENT=$(git rev-parse --verify public)
if [ "$TREE" = "$(git rev-parse "public^{tree}")" ]; then
  echo "    Nothing changed since the last publish."
  COMMIT="$PARENT"
else
  if git rev-parse --verify --quiet "refs/tags/$TAG" >/dev/null; then
    # Tag exists already: update between releases (README, workflows, ...)
    MESSAGE="Update public files ($TAG)"
    BODY=""
  else
    MESSAGE="Release $TAG"
    BODY=$(awk -v v="$VERSION" 'index($0, "## [" v "]") == 1 {found=1; next} found && /^## \[/ {exit} found {print}' CHANGELOG.md \
      | sed '/./,$!d' | sed -e :a -e '/^\n*$/{$d;N;ba}')
    [ -n "$BODY" ] || echo "      WARNING: no CHANGELOG entry '## [$VERSION]' found"
  fi
  COMMIT=$(printf '%s\n\n%s\n' "$MESSAGE" "$BODY" | \
    GIT_AUTHOR_NAME="$AUTHOR_NAME" GIT_AUTHOR_EMAIL="$AUTHOR_EMAIL" \
    GIT_COMMITTER_NAME="$AUTHOR_NAME" GIT_COMMITTER_EMAIL="$AUTHOR_EMAIL" \
    git commit-tree "$TREE" -p "$PARENT" -F - | head -1)
  git update-ref "$PUBLIC_REF" "$COMMIT" "$PARENT"
  echo "    Committed $(git rev-parse --short "$COMMIT"): $MESSAGE"
fi

# --- Tag: only for a new version; existing tags are never moved ---
TAG_COMMIT=$(git rev-parse --verify --quiet "refs/tags/$TAG^{commit}" || true)
PUSH_TAG=""
if [ -z "$TAG_COMMIT" ]; then
  git tag "$TAG" "$COMMIT"
  echo "    Tagged $(git rev-parse --short "$COMMIT") as $TAG"
  PUSH_TAG=1
elif [ "$TAG_COMMIT" = "$COMMIT" ]; then
  echo "    Tag $TAG already points to this commit."
  git ls-remote --exit-code --tags "$REMOTE" "refs/tags/$TAG" >/dev/null 2>&1 || PUSH_TAG=1
else
  echo "    Tag $TAG stays on $(git rev-parse --short "$TAG_COMMIT") (already released; this is an update between releases)."
  echo "    To redo an unpushed release instead: git tag -f $TAG public"
fi

echo ""
echo "==> Done. Branch 'public' is at $(git rev-parse --short public). Push with:"
echo "      git push $REMOTE public:$REMOTE_BRANCH"
if [ -n "$PUSH_TAG" ]; then
  echo "      git push $REMOTE $TAG    # starts the release build"
fi
