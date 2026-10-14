import { afterEach, describe, expect, it } from "vitest"
import { bindEyeDropper, createEyeDropper } from "../eyedropper.js"

function pendingHost() {
  const host = {
    resolvers: [] as Array<(hex: string | null) => void>,
    rejecters: [] as Array<(error: Error) => void>,
    supportsEyeDropper: () => true,
    openEyeDropper: () =>
      new Promise<string | null>((resolve, reject) => {
        host.resolvers.push(resolve)
        host.rejecters.push(reject)
      }),
  }
  return host
}

describe("EyeDropper.open", () => {
  it("resolves the picked colour as sRGBHex", async () => {
    const host = pendingHost()
    const dropper = new (createEyeDropper(() => host))()
    const result = dropper.open()
    host.resolvers[0]("#ff8000")
    expect(await result).toEqual({ sRGBHex: "#ff8000" })
  })

  it("rejects with AbortError when the user cancels", async () => {
    const host = pendingHost()
    const result = new (createEyeDropper(() => host))().open()
    host.resolvers[0](null)
    await expect(result).rejects.toMatchObject({ name: "AbortError" })
  })

  it("rejects with the signal's reason on abort, then ignores the late pick", async () => {
    const host = pendingHost()
    const dropper = new (createEyeDropper(() => host))()
    const controller = new AbortController()
    const result = dropper.open({ signal: controller.signal })
    controller.abort()
    await expect(result).rejects.toMatchObject({ name: "AbortError" })
    host.resolvers[0]("#000000")
    await Promise.resolve()
  })

  it("rejects at once for a signal that is already aborted, without opening", async () => {
    const host = pendingHost()
    const controller = new AbortController()
    controller.abort()
    await expect(
      new (createEyeDropper(() => host))().open({ signal: controller.signal })
    ).rejects.toMatchObject({ name: "AbortError" })
    expect(host.resolvers).toHaveLength(0)
  })

  it("rejects a second open until the native session ends, even after an abort", async () => {
    const host = pendingHost()
    const dropper = new (createEyeDropper(() => host))()
    const controller = new AbortController()
    const first = dropper.open({ signal: controller.signal })
    controller.abort()
    await expect(first).rejects.toMatchObject({ name: "AbortError" })
    await expect(dropper.open()).rejects.toMatchObject({
      name: "InvalidStateError",
    })
    host.resolvers[0]("#000000")
    await Promise.resolve()
    const next = dropper.open()
    host.resolvers[1]("#123456")
    expect(await next).toEqual({ sRGBHex: "#123456" })
  })

  it("forwards native failures", async () => {
    const host = pendingHost()
    const result = new (createEyeDropper(() => host))().open()
    host.rejecters[0](new Error("no sRGB"))
    await expect(result).rejects.toThrow("no sRGB")
  })

  it("rejects when the renderer has no eye dropper", async () => {
    await expect(
      new (createEyeDropper(() => ({})))().open()
    ).rejects.toThrow("not available")
  })
})

describe("bindEyeDropper", () => {
  const scope = globalThis as { EyeDropper?: unknown }

  afterEach(() => {
    delete scope.EyeDropper
  })

  it("defines the global only where the platform supports it", () => {
    bindEyeDropper({ supportsEyeDropper: () => false })
    expect(scope.EyeDropper).toBeUndefined()
    bindEyeDropper({})
    expect(scope.EyeDropper).toBeUndefined()
    bindEyeDropper(pendingHost())
    expect(typeof scope.EyeDropper).toBe("function")
  })
})
