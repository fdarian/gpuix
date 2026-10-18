/// `disabled`, `enabled`, `hasDisabled` and `hasEnabled`: `:disabled`, `:enabled`,
/// `:has(:disabled)` and `:has(:enabled)` for the form controls gpuix has (`<input>`,
/// `<textarea>`) and the components that stand in for a `<button>` or `<select>`.
// @ts-nocheck

import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { Button } from "../components/button"
import { Select, SelectTrigger } from "../components/select"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type TestRoot = ReturnType<typeof createTestRoot>

function withRoot(tree: ReactElement, run: (testRoot: TestRoot) => void) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    run(testRoot)
  } finally {
    testRoot.unmount()
  }
}

const bounds = (testRoot: TestRoot, testId: string) => {
  testRoot.renderer.dispatchNativeEvents()
  return testRoot.renderer.getElementBounds(testRoot.renderer.findByTestId(testId).id)
}
const widthOf = (testRoot: TestRoot, testId: string) => bounds(testRoot, testId).width

const WIDTH = 50
const states = { disabled: { width: 70 }, enabled: { width: 90 } }

describeNative("disabled and enabled", () => {
  it("styles an input by whether it is disabled, and follows the prop", () => {
    const input = (disabled: boolean) => (
      <input testId="field" disabled={disabled} style={{ width: WIDTH, height: 20, ...states }} />
    )
    withRoot(input(false), (testRoot) => {
      expect(widthOf(testRoot, "field")).toBe(90)
      testRoot.render(input(true))
      expect(widthOf(testRoot, "field")).toBe(70)
      testRoot.render(input(false))
      expect(widthOf(testRoot, "field")).toBe(90)
    })
  })

  it("styles a textarea too", () => {
    const area = (disabled: boolean) => (
      <textarea testId="area" disabled={disabled} style={{ width: WIDTH, height: 20, ...states }} />
    )
    withRoot(area(true), (testRoot) => {
      expect(widthOf(testRoot, "area")).toBe(70)
      testRoot.render(area(false))
      expect(widthOf(testRoot, "area")).toBe(90)
    })
  })

  it("runs a transition declared in disabled, and one in enabled on the way back", () => {
    const input = (disabled: boolean) => (
      <input
        testId="field"
        disabled={disabled}
        style={{
          width: WIDTH,
          height: 20,
          disabled: { width: 150, transition: "width 1s linear" },
          enabled: { transition: "width 1s linear" },
        }}
      />
    )
    const testRoot = createTestRoot()
    try {
      testRoot.renderer.clockPause()
      testRoot.render(input(false))
      testRoot.render(input(true))
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "field")).toBeCloseTo(100, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "field")).toBe(150)
      testRoot.render(input(false))
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "field")).toBeCloseTo(100, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "field")).toBe(WIDTH)
    } finally {
      testRoot.unmount()
    }
  })

  it("wins over hover on the same field, which still applies to a disabled control", () => {
    const tree = (
      <input
        testId="field"
        disabled
        style={{ width: WIDTH, height: 20, hover: { width: 80, height: 30 }, disabled: { width: 70 } }}
      />
    )
    withRoot(tree, (testRoot) => {
      testRoot.renderer.nativeSimulateMouseMove(5, 5)
      expect([widthOf(testRoot, "field"), bounds(testRoot, "field").height]).toEqual([70, 30])
    })
  })

  it("applies inside an inert subtree, where hover does not", () => {
    const tree = (
      <div inert>
        <input testId="field" disabled style={{ width: WIDTH, height: 20, disabled: { width: 70 } }} />
      </div>
    )
    withRoot(tree, (testRoot) => expect(widthOf(testRoot, "field")).toBe(70))
  })

  it("styles a Button by its disabled prop", () => {
    const button = (disabled: boolean) => (
      <Button testId="button" disabled={disabled} style={{ width: WIDTH, height: 20, ...states }} />
    )
    withRoot(button(false), (testRoot) => {
      expect(widthOf(testRoot, "button")).toBe(90)
      testRoot.render(button(true))
      expect(widthOf(testRoot, "button")).toBe(70)
      testRoot.render(button(false))
      expect(widthOf(testRoot, "button")).toBe(90)
    })
  })

  it("styles the root and the trigger of a Select", () => {
    const select = (disabled: boolean) => (
      <Select testId="select" disabled={disabled} style={{ width: WIDTH, height: 20, ...states }}>
        <SelectTrigger testId="trigger" style={{ width: 40, height: 20, ...states }} />
      </Select>
    )
    withRoot(select(false), (testRoot) => {
      expect([widthOf(testRoot, "select"), widthOf(testRoot, "trigger")]).toEqual([90, 90])
      testRoot.render(select(true))
      expect([widthOf(testRoot, "select"), widthOf(testRoot, "trigger")]).toEqual([70, 70])
    })
  })

  it("does not match a plain div that has a disabled prop, as in a DOM", () => {
    const tree = (
      <div>
        <div testId="plain" disabled style={{ width: WIDTH, height: 20, ...states }} />
        <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, hasEnabled: { width: 90 } }}>
          <div disabled style={{ width: 10, height: 10 }} />
        </div>
      </div>
    )
    withRoot(tree, (testRoot) => {
      expect(widthOf(testRoot, "plain")).toBe(WIDTH)
      expect(widthOf(testRoot, "holder")).toBe(WIDTH)
    })
  })
})

describeNative("hasDisabled and hasEnabled", () => {
  const scene = (disabled: boolean, holder: Record<string, unknown> = {}) => (
    <div>
      <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, hasEnabled: { height: 40 }, ...holder }}>
        <div style={{ width: 80, height: 30 }}>
          <input testId="field" disabled={disabled} style={{ width: 20, height: 10 }} />
        </div>
      </div>
      <div testId="sibling" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, hasEnabled: { width: 90 } }} />
    </div>
  )

  it("matches the ancestors of a control by its state, and follows the prop", () => {
    withRoot(scene(false), (testRoot) => {
      expect([widthOf(testRoot, "holder"), bounds(testRoot, "holder").height]).toEqual([WIDTH, 40])
      testRoot.render(scene(true))
      expect([widthOf(testRoot, "holder"), bounds(testRoot, "holder").height]).toEqual([70, 20])
      testRoot.render(scene(false))
      expect([widthOf(testRoot, "holder"), bounds(testRoot, "holder").height]).toEqual([WIDTH, 40])
    })
  })

  it("does not match a sibling or the control itself", () => {
    const tree = (
      <div>
        <input testId="field" disabled style={{ width: WIDTH, height: 20, hasDisabled: { width: 90 } }} />
        {scene(true)}
      </div>
    )
    withRoot(tree, (testRoot) => {
      expect(widthOf(testRoot, "field")).toBe(WIDTH)
      expect(widthOf(testRoot, "sibling")).toBe(WIDTH)
    })
  })

  it("sits below the element's own disabled", () => {
    const control = (
      <Button testId="holder" disabled style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 }, disabled: { width: 90 } }}>
        <input disabled style={{ width: 10, height: 10 }} />
      </Button>
    )
    withRoot(control, (testRoot) => expect(widthOf(testRoot, "holder")).toBe(90))
  })

  it("matches the ancestors of a disabled Button, and runs a transition declared in it", () => {
    const tree = (disabled: boolean) => (
      <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 150, transition: "width 1s linear" } }}>
        <Button disabled={disabled} style={{ width: 10, height: 10 }} />
      </div>
    )
    const testRoot = createTestRoot()
    try {
      testRoot.renderer.clockPause()
      testRoot.render(tree(false))
      testRoot.render(tree(true))
      testRoot.renderer.clockFastForward(500)
      expect(widthOf(testRoot, "holder")).toBeCloseTo(100, 0)
      testRoot.renderer.clockFastForward(1000)
      expect(widthOf(testRoot, "holder")).toBe(150)
    } finally {
      testRoot.unmount()
    }
  })

  it("follows a control that is added or removed", () => {
    const tree = (withField: boolean) => (
      <div testId="holder" style={{ width: WIDTH, height: 20, hasDisabled: { width: 70 } }}>
        {withField && <input disabled style={{ width: 10, height: 10 }} />}
      </div>
    )
    withRoot(tree(false), (testRoot) => {
      expect(widthOf(testRoot, "holder")).toBe(WIDTH)
      testRoot.render(tree(true))
      expect(widthOf(testRoot, "holder")).toBe(70)
      testRoot.render(tree(false))
      expect(widthOf(testRoot, "holder")).toBe(WIDTH)
    })
  })
})
