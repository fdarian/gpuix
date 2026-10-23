Read [AGENTS.md](./AGENTS.md) first.

This checkout is the `fdarian/gpuix` fork; [FORK.md](./FORK.md) overrides AGENTS.md wherever they disagree.
AGENTS.md started as upstream's file and the fork now edits it for its own behaviour, so upstream syncs can
conflict there. The `zed/` submodule is `fdarian/zed` on `poc`, not `remorses/zed` on `gpuix`: keep it on
local `poc`, and land zed changes there and update the submodule as described in FORK.md's "Working on the fork".
