/// The Solid half of `motion-transform.test.tsx` of the React package: Motion's transform keys on a
/// `motion.div` give the box of the CSS transform they stand for, and animate.

import { createSignal } from "solid-js"
import { describe, expect, it } from "bun:test"
import { motion } from "../index.js"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import type { TestRenderer } from "../testing.js"

const frame = { display: "flex", padding: 200 } as const
const box = { width: 100, height: 50, backgroundColor: "#336699" } as const

function boundsOf(renderer: TestRenderer): [number, number, number, number] {
  const bounds = renderer.getElementBounds(renderer.findByTestId("box")!.id)!
  return [bounds.x, bounds.y, bounds.width, bounds.height]
}

function expectBox(got: number[], want: number[]) {
  for (const [index, value] of want.entries()) expect(got[index]).toBeCloseTo(value, 1)
}

function reference(transform: string): [number, number, number, number] {
  const app = createTestRoot()
  try {
    app.renderer.clockPause()
    app.render(() => (
      <div style={frame}>
        <div testId="box" style={{ ...box, transform }} />
      </div>
    ))
    return boundsOf(app.renderer)
  } finally {
    app.unmount()
  }
}

describe.skipIf(!hasNativeTestRenderer)("Solid motion transform keys", () => {
  it("composes a static style like the CSS transform, with scale and rotate Motion's", () => {
    const app = createTestRoot()
    try {
      app.renderer.clockPause()
      app.render(() => (
        <div style={frame}>
          <motion.div testId="box" style={{ ...box, x: 10, y: "50%", scale: 2, rotate: 30 }} />
        </div>
      ))
      expectBox(boundsOf(app.renderer), reference("translateX(10px) translateY(50%) scale(2) rotate(30deg)"))
    } finally {
      app.unmount()
    }
  })

  it("animates x, rotate and scale from Motion's defaults", () => {
    const app = createTestRoot()
    try {
      const renderer = app.renderer
      renderer.clockPause()
      app.render(() => (
        <div style={frame}>
          <motion.div
            testId="box"
            style={box}
            animate={{ x: 100, rotate: 90, scale: 2 }}
            transition={{ duration: 1, ease: "linear" }}
          />
        </div>
      ))
      renderer.clockFastForward(500)
      renderer.flush()
      renderer.dispatchNativeEvents()
      expectBox(boundsOf(renderer), reference("translateX(50px) scale(1.5) rotate(45deg)"))
    } finally {
      app.unmount()
    }
  })

  it("follows a reactive style", () => {
    const app = createTestRoot()
    try {
      const [x, setX] = createSignal(10)
      app.renderer.clockPause()
      app.render(() => (
        <div style={frame}>
          <motion.div testId="box" style={{ ...box, x: x() }} />
        </div>
      ))
      expectBox(boundsOf(app.renderer), reference("translateX(10px)"))
      setX(60)
      app.renderer.flush()
      expectBox(boundsOf(app.renderer), reference("translateX(60px)"))
    } finally {
      app.unmount()
    }
  })
})
