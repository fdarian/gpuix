#!/usr/bin/env bash
# Rebases this fork's `poc` branches on upstream, in both repos.
#
#   scripts/sync-upstream.sh              sync, then push
#   scripts/sync-upstream.sh --no-push    sync only
#
# Layout it relies on (see FORK.md):
#   - gpuix `main` and zed `gpuix` mirror upstream and carry no patches.
#   - zed `poc` = upstream zed `gpuix` + the fork's zed commits.
#   - gpuix `poc` = upstream/main + the fork's commits, with the fork commit
#     (.gitmodules, submodule bump, FORK.md, this script, README notice) last.
set -euo pipefail

push=1
[ "${1:-}" = "--no-push" ] && push=0

root="$(cd "$(dirname "$0")/.." && pwd)"
zed="$root/zed"

if [ -n "$(git -C "$root" status --porcelain --untracked-files=no -- . ':!zed')" ]; then
	echo "gpuix has uncommitted changes; commit or stash them first" >&2
	exit 1
fi
[ -e "$zed/.git" ] || git -C "$root" submodule update --init zed
if [ -n "$(git -C "$zed" status --porcelain --untracked-files=no)" ]; then
	echo "zed has uncommitted changes; commit or stash them first" >&2
	exit 1
fi
git -C "$zed" remote get-url upstream >/dev/null 2>&1 ||
	git -C "$zed" remote add upstream https://github.com/remorses/zed.git

git -C "$root" fetch --all
git -C "$zed" fetch --all

# A clone that was last pushed from elsewhere is behind origin/poc. Fast-forward
# it; a branch that is ahead or diverged (a rebase not pushed yet) is kept as is.
fast_forward_poc() {
	git -C "$1" checkout --quiet poc
	if git -C "$1" rev-parse --verify --quiet origin/poc >/dev/null &&
		git -C "$1" merge-base --is-ancestor poc origin/poc; then
		git -C "$1" merge --ff-only --quiet origin/poc
	fi
}
fast_forward_poc "$zed"
fast_forward_poc "$root"

# zed: replay the fork's commits (everything on poc that upstream/gpuix does not
# contain). The target is the tip, not upstream gpuix's pin, which may lag it:
# the test suites are what check that the fork builds against the tip.
git -C "$zed" rebase upstream/gpuix
zed_tip="$(git -C "$zed" rev-parse HEAD)"

# gpuix: replay the fork's commits on upstream/main. The fork commit moves the
# submodule pointer, which conflicts whenever upstream moved it too. That is the
# one conflict this script resolves itself, by pointing at the rebased zed tip.
export GIT_EDITOR=true
git -C "$root" rebase upstream/main || true
while [ -d "$(git -C "$root" rev-parse --git-path rebase-merge)" ]; do
	conflicts="$(git -C "$root" diff --name-only --diff-filter=U)"
	if [ "$conflicts" != "zed" ]; then
		echo "Conflicts in: ${conflicts:-none}" >&2
		echo "Resolve them, run 'git rebase --continue', then run this script again." >&2
		exit 1
	fi
	git -C "$zed" checkout --quiet "$zed_tip"
	git -C "$root" add zed
	git -C "$root" rebase --continue || true
done

# The fork commit is the tip of poc, so a new zed tip is folded into it.
git -C "$zed" checkout --quiet poc
if [ "$(git -C "$root" rev-parse HEAD:zed)" != "$zed_tip" ]; then
	git -C "$root" add zed
	git -C "$root" commit --amend --no-edit
fi

git -C "$root" submodule status zed

if [ "$push" = 1 ]; then
	git -C "$root" push origin upstream/main:refs/heads/main
	git -C "$zed" push origin upstream/gpuix:refs/heads/gpuix
	git -C "$zed" push --force-with-lease origin poc
	git -C "$root" push --force-with-lease origin poc
fi
