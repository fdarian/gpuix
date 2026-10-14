import { afterEach, describe, expect, it } from "vitest"
import { bindClipboard, clipboard, createClipboard } from "../clipboard.js"

function fakeHost(initial: string | null = null) {
  const host = {
    text: initial,
    writeClipboardText(text: string) {
      host.text = text
    },
    readClipboardText: () => host.text,
  }
  return host
}

describe("createClipboard", () => {
  it("round-trips text through the host", async () => {
    const host = fakeHost()
    const board = createClipboard(() => host)
    await board.writeText("héllo")
    expect(host.text).toBe("héllo")
    expect(await board.readText()).toBe("héllo")
  })

  it("reads an empty string when the clipboard holds no text", async () => {
    expect(await createClipboard(() => fakeHost(null)).readText()).toBe("")
  })

  it("rejects when no renderer has a clipboard", async () => {
    const board = createClipboard(() => undefined)
    await expect(board.writeText("x")).rejects.toThrow("clipboard")
    await expect(board.readText()).rejects.toThrow("clipboard")
  })
})

describe("bindClipboard", () => {
  const scope = globalThis as { navigator?: { clipboard?: unknown } }
  const original = Object.getOwnPropertyDescriptor(scope.navigator ?? {}, "clipboard")

  afterEach(() => {
    if (original) Object.defineProperty(scope.navigator, "clipboard", original)
    else delete scope.navigator?.clipboard
  })

  it("installs navigator.clipboard and follows the latest renderer", async () => {
    const first = fakeHost("first")
    const second = fakeHost("second")
    bindClipboard(first)
    expect(scope.navigator?.clipboard).toBe(clipboard)
    expect(await clipboard.readText()).toBe("first")
    bindClipboard(second)
    expect(await clipboard.readText()).toBe("second")
  })

  it("leaves an existing navigator.clipboard alone", () => {
    const existing = { readText: async () => "web" }
    Object.defineProperty(scope.navigator, "clipboard", {
      value: existing,
      configurable: true,
    })
    bindClipboard(fakeHost())
    expect(scope.navigator?.clipboard).toBe(existing)
  })
})
