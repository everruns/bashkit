#!/usr/bin/env bash
# Resolve the commit an npm publish workflow builds, and its npm dist-tag.
#
# Decisions:
# - Default: build the workflow's own commit. Only a release-tag ref
#   (refs/tags/vX.Y.Z) publishes as `latest`; any other ref publishes `next`.
# - Recovery: a manual dispatch may name an existing release tag (`tag` input).
#   The workflow definition then comes from that tag or current main, the
#   package source from the tag's exact commit, and it publishes as `latest`.
#   This re-publishes a release whose npm lane failed without cutting a new
#   version (0.20.1: the tag's own workflow still used a revoked NPM_TOKEN).
# - The source commit must always be reachable from origin/main.
#
# Env: INPUT_TAG (may be empty), GITHUB_SHA, GITHUB_REF, GITHUB_OUTPUT.
# Outputs: sha, dist_tag, version (expected package version, empty for next).
set -euo pipefail

git fetch --no-tags --force origin main:refs/remotes/origin/main
MAIN_SHA=$(git rev-parse origin/main)
TAG_RE='^v[0-9]+\.[0-9]+\.[0-9]+$'

if [ -n "${INPUT_TAG:-}" ]; then
  if [[ ! "$INPUT_TAG" =~ $TAG_RE ]]; then
    echo "Error: tag must match vMAJOR.MINOR.PATCH, got '$INPUT_TAG'" >&2
    exit 1
  fi
  git fetch --no-tags --force origin "refs/tags/$INPUT_TAG:refs/tags/$INPUT_TAG"
  SOURCE_SHA=$(git rev-list -n 1 "refs/tags/$INPUT_TAG")
  if [ "$GITHUB_SHA" != "$SOURCE_SHA" ] && [ "$GITHUB_SHA" != "$MAIN_SHA" ]; then
    echo "Error: with a tag input the workflow ref must be $INPUT_TAG or current main" >&2
    exit 1
  fi
  DIST_TAG=latest
  VERSION="${INPUT_TAG#v}"
else
  SOURCE_SHA="$GITHUB_SHA"
  if [[ "$GITHUB_REF" == refs/tags/* ]] && [[ "${GITHUB_REF#refs/tags/}" =~ $TAG_RE ]]; then
    DIST_TAG=latest
    VERSION="${GITHUB_REF#refs/tags/v}"
  else
    DIST_TAG=next
    VERSION=""
  fi
fi

if ! git merge-base --is-ancestor "$SOURCE_SHA" "$MAIN_SHA"; then
  echo "Error: publish source $SOURCE_SHA is not reachable from origin/main" >&2
  exit 1
fi

echo "Publish source $SOURCE_SHA (dist-tag $DIST_TAG${VERSION:+, version $VERSION})"
{
  echo "sha=$SOURCE_SHA"
  echo "dist_tag=$DIST_TAG"
  echo "version=$VERSION"
} >> "$GITHUB_OUTPUT"
