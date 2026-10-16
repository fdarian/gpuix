import { describe, expect, it } from "vitest"
import { createMutationQueue } from "../host.js"
import { forEachBlobUrl } from "../style-images.js"

function setup() {
  const images: { url: string; bytes: number[] }[] = []
  const failures: { url: string; message: string }[] = []
  const renderer = {
    applyBatch() {
      return []
    },
    setStyleImage(url: string, bytes: Uint8Array) {
      images.push({ url, bytes: [...bytes] })
    },
    failStyleImage(url: string, message: string) {
      failures.push({ url, message })
    },
  }
  return { images, failures, mutations: createMutationQueue(renderer) }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 20))

function blobUrls(style: unknown) {
  const found: string[] = []
  forEachBlobUrl(style, (url) => found.push(url))
  return found
}

describe("blob: addresses in a style", () => {
  it("finds every quoting form of a blob url() in any string", () => {
    expect(
      blobUrls({
        background: "url(blob:a) no-repeat, url( blob:b ), url(\"blob:c\"), url('blob:d')",
        backgroundImage: 'linear-gradient(red, blue), url(  "blob:http://x/e"  )',
      })
    ).toEqual(["blob:a", "blob:b", "blob:c", "blob:d", "blob:http://x/e"])
  })

  it("looks inside state styles, arrays and escapes, and ignores other schemes", () => {
    expect(
      blobUrls({
        hover: { backgroundImage: "url(blob:hover)" },
        focusVisible: { nested: [{ background: "url('blob:it\\'s')" }] },
        background: "url(data:image/png;base64,AAAA), url(https://x/blob:y.png)",
        content: "blob:not-in-url",
      })
    ).toEqual(["blob:hover", "blob:it's"])
    expect(blobUrls({ width: 10, color: null, background: undefined })).toEqual([])
  })
})

describe("style images from blob: URLs", () => {
  it("fetches a blob named by a style once and hands its bytes to the renderer", async () => {
    const { images, mutations } = setup()
    const url = URL.createObjectURL(new Blob([new Uint8Array([1, 2, 3])]))
    mutations.createElement(1, "div")
    mutations.setStyle(1, { background: `url(${url})` })
    mutations.setStyle(1, { background: `url("${url}") center` })
    mutations.setStyle(1, { backgroundColor: "red", hover: { backgroundImage: `url(${url})` } })
    await settle()
    expect(images).toEqual([{ url, bytes: [1, 2, 3] }])
    URL.revokeObjectURL(url)
  })

  it("finds a blob in the universal style and in a hover style of another element", async () => {
    const { images, mutations } = setup()
    const first = URL.createObjectURL(new Blob(["a"]))
    const second = URL.createObjectURL(new Blob(["b"]))
    mutations.setUniversalStyle({ backgroundImage: `url(${first})` })
    mutations.createElement(2, "div")
    mutations.setStyle(2, { hover: { background: `url(${second})` } })
    await settle()
    expect(images.map((image) => image.url).sort()).toEqual([first, second].sort())
    URL.revokeObjectURL(first)
    URL.revokeObjectURL(second)
  })

  it("finds a blob in maskImage, in the mask shorthand and in a hover mask", async () => {
    const { images, mutations } = setup()
    const urls = ["a", "b", "c"].map((text) => URL.createObjectURL(new Blob([text])))
    expect(blobUrls({ maskImage: `linear-gradient(red, blue), url(${urls[0]})`, mask: `url("${urls[1]}") luminance` })).toEqual([
      urls[0],
      urls[1],
    ])
    mutations.createElement(1, "div")
    mutations.setStyle(1, {
      maskImage: `url(${urls[0]})`,
      mask: `url('${urls[1]}') no-repeat center / 20px`,
      hover: { maskImage: `url(${urls[2]}), url(${urls[0]})` },
    })
    await settle()
    expect(images.map((image) => image.url).sort()).toEqual([...urls].sort())
    for (const url of urls) URL.revokeObjectURL(url)
  })

  it("reports a revoked blob through failStyleImage", async () => {
    const { images, failures, mutations } = setup()
    const url = URL.createObjectURL(new Blob(["x"]))
    URL.revokeObjectURL(url)
    mutations.createElement(1, "div")
    mutations.setStyle(1, { backgroundImage: `url(${url})` })
    await settle()
    expect(images).toEqual([])
    expect(failures).toHaveLength(1)
    expect(failures[0]?.url).toBe(url)
    expect(typeof failures[0]?.message).toBe("string")
  })

  it("does nothing for a style without a blob", async () => {
    const { images, failures, mutations } = setup()
    mutations.createElement(1, "div")
    mutations.setStyle(1, { background: "url(a.png)", backgroundColor: "red" })
    await settle()
    expect([images, failures]).toEqual([[], []])
  })
})
