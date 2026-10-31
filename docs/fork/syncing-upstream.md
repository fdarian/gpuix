# Syncing with upstream

Alpina takes commits from gpuix and zed one at a time. There is no mirror branch and no rebase: `main` and zed `poc` only
grow (see [Repos and branches](working-on-the-fork.md#repos-and-branches)), and a ledger below records how far each upstream
has been reviewed.

- gpuix upstream is `upstream/main` (`remorses/gpuix`).
- zed upstream is `upstream/gpuix` (`remorses/zed`), in the `zed/` submodule.

## Ledger

Reviewed through, one SHA per repo. A commit at or before the SHA was taken, adapted or skipped. The start values are the
merge bases of the fork and upstream when the ledger began.

| Repo | Reviewed through |
|---|---|
| gpuix (`upstream/main`) | `515df198fef2685f5ed7dd120bf0db638eb5c360` |
| zed (`upstream/gpuix`) | `2565b8a3e33537c3d3f56d0d45f11e50a17e512d` |

Skips worth remembering, with the reason (a commit that looks wanted but breaks something):

- none yet

## Procedure

1. Fetch both upstreams: `git fetch upstream` in the repo root and in `zed/`.
2. List what is new, oldest first: `git log --reverse <ledger>..upstream/main`, and the same in `zed/` against
   `upstream/gpuix`.
3. Decide per commit: **take** it, **adapt** it, or **skip** it.
   - Take with `git cherry-pick -x`, so `git log --grep 'cherry picked from'` finds what came from upstream.
   - Adapt when the pick conflicts or does not fit; the invariants to keep are the conflict notes in
     [working-on-the-fork.md](working-on-the-fork.md#resolving-conflicts).
   - Skip release and version commits, Windows-only and Linux-only work, paths the fork removed, and anything against the
     DOM-semantics principle in [index.md](index.md#the-principle-dom-and-css-semantics). Fixes and additions the fork
     deems fit are taken.
4. Run the suites ([testing.md](testing.md)).
5. Advance the ledger in its own commit. A skip worth remembering is a line under "Skips" with its reason.

A picked commit has upstream's npm scope rewritten to `@ghia`, because the packages are published under it. The script
is in the body of the commit "The npm scope is @ghia".

A zed pick goes to zed `poc` first and is pushed; then a gpuix commit bumps the `zed` gitlink.
