import { describe, expect, it } from "bun:test"
import { hasNativeTestRenderer } from "../testing.js"

// The test window lives in a thread-local. Tearing it down from the thread's
// destructors touched thread-locals that were already gone, and the process
// aborted (exit 134) after any run that left a floating layer mounted.
describe.skipIf(!hasNativeTestRenderer)("test renderer at thread exit", () => {
  for (const scenario of ["tooltip", "dialog"]) {
    it(`a failed ${scenario} test ends the process with the test failure`, async () => {
      const child = Bun.spawn(
        ["bun", "test", new URL("./thread-exit.fixture.test.tsx", import.meta.url).pathname],
        {
          cwd: new URL("../..", import.meta.url).pathname,
          env: { ...process.env, GPUIX_THREAD_EXIT_FIXTURE: scenario },
          stdout: "pipe",
          stderr: "pipe",
        }
      )
      const [stdout, stderr, code] = await Promise.all([
        new Response(child.stdout).text(),
        new Response(child.stderr).text(),
        child.exited,
      ])
      const output = stdout + stderr
      expect(output).toContain("fixture failure")
      expect(output).not.toContain("thread local panicked")
      expect(code).toBe(1)
    }, 60_000)
  }
})
