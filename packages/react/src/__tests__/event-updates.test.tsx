import { describe, expect, it, vi } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const box = { width: 100, height: 100 }

describeNative("event handler updates", () => {
  it("wires a handler added on a later render", () => {
    const { render, renderer, unmount } = createTestRoot()
    const onClick = vi.fn()
    try {
      render(<div style={box} />)
      renderer.nativeSimulateClick(10, 10)
      expect(onClick).not.toHaveBeenCalled()

      render(<div style={box} onClick={onClick} />)
      renderer.nativeSimulateClick(10, 10)
      expect(onClick).toHaveBeenCalledTimes(1)
    } finally {
      unmount()
    }
  })

  it("calls only the latest handler after it is replaced", () => {
    const { render, renderer, unmount } = createTestRoot()
    const first = vi.fn()
    const second = vi.fn()
    try {
      render(<div style={box} onClick={first} />)
      render(<div style={box} onClick={second} />)
      renderer.nativeSimulateClick(10, 10)

      expect(first).not.toHaveBeenCalled()
      expect(second).toHaveBeenCalledTimes(1)
    } finally {
      unmount()
    }
  })

  it("stops calling a handler whose prop is no longer passed", () => {
    const { render, renderer, unmount } = createTestRoot()
    const onClick = vi.fn()
    try {
      render(<div style={box} onClick={onClick} />)
      render(<div style={box} />)
      renderer.nativeSimulateClick(10, 10)

      expect(onClick).not.toHaveBeenCalled()
      expect(renderer.findByType("div")[0]!.events.has("click")).toBe(false)
    } finally {
      unmount()
    }
  })

  it("stops calling a handler that is set to undefined", () => {
    const { render, renderer, unmount } = createTestRoot()
    const onClick = vi.fn()
    try {
      render(<div style={box} onClick={onClick} />)
      render(<div style={box} onClick={undefined} />)
      renderer.nativeSimulateClick(10, 10)

      expect(onClick).not.toHaveBeenCalled()
      expect(renderer.findByType("div")[0]!.events.has("click")).toBe(false)
    } finally {
      unmount()
    }
  })

  it("keeps handlers for other events when one is removed", () => {
    const { render, renderer, unmount } = createTestRoot()
    const onClick = vi.fn()
    const onMouseDown = vi.fn()
    try {
      render(<div style={box} onClick={onClick} onMouseDown={onMouseDown} />)
      render(<div style={box} onMouseDown={onMouseDown} />)

      const events = renderer.findByType("div")[0]!.events
      expect(events.has("click")).toBe(false)
      expect(events.has("mousedown")).toBe(true)
    } finally {
      unmount()
    }
  })
})
