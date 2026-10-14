import { describe, expect, it } from "vitest"
import { createMutationQueue } from "../host.js"

function setup() {
  const batches: unknown[][] = []
  const images: { id: number; bytes: number[] }[] = []
  const renderer = {
    applyBatch(json: string) {
      batches.push(JSON.parse(json))
      return []
    },
    setImage(id: number, bytes: Uint8Array) {
      images.push({ id, bytes: [...bytes] })
    },
  }
  return { batches, images, mutations: createMutationQueue(renderer) }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 20))

describe("img blob: sources", () => {
  it("fetches the blob and hands its bytes to setImage after the element exists", async () => {
    const { batches, images, mutations } = setup()
    const url = URL.createObjectURL(new Blob([new Uint8Array([1, 2, 3])]))
    mutations.createElement(7, "img")
    mutations.setCustomProp(7, "src", url)
    await settle()
    expect(images).toEqual([{ id: 7, bytes: [1, 2, 3] }])
    // The element was created natively before the bytes arrived, and the src
    // itself still reaches native, which treats a blob: src as pending.
    expect(batches.flat()).toEqual([
      ["createElement", 7, "img"],
      ["setCustomProp", 7, "src", url],
    ])
    URL.revokeObjectURL(url)
  })

  it("reports a revoked blob through srcError", async () => {
    const { batches, images, mutations } = setup()
    const url = URL.createObjectURL(new Blob(["x"]))
    URL.revokeObjectURL(url)
    mutations.createElement(1, "img")
    mutations.setCustomProp(1, "src", url)
    mutations.flushMutations()
    await settle()
    mutations.flushMutations()
    expect(images).toEqual([])
    const errors = batches.flat().filter((mutation) => mutation[2] === "srcError")
    expect(errors).toHaveLength(1)
    expect(typeof (errors[0]?.[3] as { message: string }).message).toBe("string")
  })

  it("gives each failure its own value, so a repeated message is still applied", async () => {
    const { batches, mutations } = setup()
    const garbage = URL.createObjectURL(new Blob(["x"]))
    URL.revokeObjectURL(garbage)
    mutations.createElement(1, "img")
    mutations.setCustomProp(1, "src", garbage)
    mutations.flushMutations()
    await settle()
    mutations.flushMutations()
    mutations.setCustomProp(1, "src", "/tmp/other.png")
    mutations.setCustomProp(1, "src", garbage)
    mutations.flushMutations()
    await settle()
    mutations.flushMutations()
    const errors = batches.flat().filter((mutation) => mutation[2] === "srcError")
    expect(errors).toHaveLength(2)
    expect(errors[1]?.[3]).not.toEqual(errors[0]?.[3])
  })

  it("drops a late result when the src changed or the element was destroyed", async () => {
    const { images, mutations } = setup()
    const first = URL.createObjectURL(new Blob([new Uint8Array([1])]))
    mutations.createElement(1, "img")
    mutations.setCustomProp(1, "src", first)
    mutations.setCustomProp(1, "src", "/tmp/other.png")
    mutations.createElement(2, "img")
    mutations.setCustomProp(2, "src", first)
    mutations.destroyElement(2)
    await settle()
    expect(images).toEqual([])
    URL.revokeObjectURL(first)
  })

  it("ignores blob: values on other props and other elements", async () => {
    const { images, mutations } = setup()
    const url = URL.createObjectURL(new Blob([new Uint8Array([1])]))
    mutations.createElement(1, "div")
    mutations.setCustomProp(1, "src", url)
    mutations.createElement(2, "img")
    mutations.setCustomProp(2, "alt", url)
    await settle()
    expect(images).toEqual([])
    URL.revokeObjectURL(url)
  })
})
