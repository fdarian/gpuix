// Run by thread-exit.test.tsx in a child process; skipped in a normal run.
import { describe, it } from "bun:test"
import {
  Dialog,
  DialogBackdrop,
  DialogPopup,
  DialogPortal,
  DialogTrigger,
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const scenario = process.env.GPUIX_THREAD_EXIT_FIXTURE

describe.skipIf(!hasNativeTestRenderer || !scenario)("thread exit fixture", () => {
  it("throws with a floating layer still mounted", () => {
    const app = createTestRoot()
    if (scenario === "tooltip") {
      app.render(() => (
        <Tooltip defaultOpen>
          <TooltipTrigger style={{ width: 100, height: 32 }}><text>hover</text></TooltipTrigger>
          <TooltipContent><text>help</text></TooltipContent>
        </Tooltip>
      ))
    } else {
      app.render(() => (
        <Dialog defaultOpen>
          <DialogTrigger style={{ width: 80, height: 24 }}><text>Open</text></DialogTrigger>
          <DialogPortal>
            <DialogBackdrop style={{ backgroundColor: "#00000080" }} />
            <DialogPopup style={{ width: 300, backgroundColor: "#1e293b" }}><text>Settings</text></DialogPopup>
          </DialogPortal>
        </Dialog>
      ))
    }
    throw new Error("fixture failure")
  })
})
