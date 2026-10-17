// @ts-nocheck
/**
 * What the retention bookkeeping holds follows the elements that are alive: a page that mounts
 * and unmounts rows for as long as an app runs does not grow it, whether or not a row had a motion
 * running when it went.
 */

import React from "react"
import { describe, expect, it } from "vitest"
import { flushSync, motion } from "../index"
import { createTestRoot, hasNativeTestRenderer } from "../testing"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

describeNative("retain bookkeeping", () => {
  it("does not keep records of elements that were destroyed", () => {
    let setGeneration!: (generation: number) => void
    function App() {
      const [generation, set] = React.useState(0)
      setGeneration = set
      return (
        <div>
          {Array.from({ length: 20 }, (_, index) => (
            <div key={`${generation}-${index}`} style={{ height: 20, hover: { backgroundColor: "#ff0000" } }}>
              <text>{`row ${generation} ${index}`}</text>
              {index === 0 ? (
                <motion.div
                  style={{ width: 10, height: 4 }}
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  transition={{ duration: 100, ease: "linear" }}
                />
              ) : null}
            </div>
          ))}
        </div>
      )
    }
    const root = createTestRoot({ width: 300, height: 600 })
    const { renderer } = root
    try {
      root.render(<App />)
      renderer.clockPause()
      for (let frame = 0; frame < 4; frame++) renderer.flush()
      const [recordsAtFirst, dynamicAtFirst] = renderer.native.getRetainRecordCounts()
      expect(recordsAtFirst).toBeGreaterThan(20)
      for (let generation = 1; generation <= 50; generation++) {
        flushSync(() => setGeneration(generation))
        renderer.flush()
        renderer.clockFastForward(16)
        renderer.flush()
      }
      for (let frame = 0; frame < 4; frame++) renderer.flush()
      const [records, dynamic] = renderer.native.getRetainRecordCounts()
      expect(records).toBeLessThanOrEqual(recordsAtFirst)
      expect(dynamic).toBeLessThanOrEqual(dynamicAtFirst)
      expect(records).toBeLessThanOrEqual(renderer.getRetainedElementCount())
    } finally {
      root.unmount()
    }
  })
})
