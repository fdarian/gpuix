import fs from "fs"
import { pathToFileURL } from "url"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

/** An uncompressed 24-bit BMP, which every platform decodes and needs no encoder. */
function bmp(width: number, height: number, [r, g, b]: [number, number, number]): Buffer {
  const rowSize = Math.ceil((width * 3) / 4) * 4
  const header = Buffer.alloc(54)
  header.write("BM", 0)
  header.writeUInt32LE(54 + rowSize * height, 2)
  header.writeUInt32LE(54, 10)
  header.writeUInt32LE(40, 14)
  header.writeInt32LE(width, 18)
  header.writeInt32LE(height, 22)
  header.writeUInt16LE(1, 26)
  header.writeUInt16LE(24, 28)
  header.writeUInt32LE(rowSize * height, 34)
  const pixels = Buffer.alloc(rowSize * height)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      pixels.set([b, g, r], y * rowSize + x * 3)
    }
  }
  return Buffer.concat([header, pixels])
}

const RED_6x3 = bmp(6, 3, [255, 0, 0])
const redPath = `${SHOTS_DIR}/img-load-red.bmp`
fs.writeFileSync(redPath, RED_6x3)
const bluePath = `${SHOTS_DIR}/img-load-blue.bmp`
fs.writeFileSync(bluePath, bmp(10, 5, [0, 0, 255]))

const SVG_10x6 = '<svg xmlns="http://www.w3.org/2000/svg" width="10" height="6"><rect width="10" height="6"/></svg>'
const svgPath = `${SHOTS_DIR}/img-load-10x6.svg`
fs.writeFileSync(svgPath, SVG_10x6)

type Load = { type: "load" | "error"; width?: number; height?: number; reason?: string }

/** Pumps the renderer and the event loop until `done` or it times out. */
async function settle(testRoot: TestRoot, done: () => boolean, ms = 3000) {
  const deadline = Date.now() + ms
  while (!done() && Date.now() < deadline) {
    testRoot.renderer.flush()
    testRoot.renderer.dispatchNativeEvents()
    await new Promise((resolve) => setTimeout(resolve, 10))
  }
}

function recorder() {
  const events: Load[] = []
  return {
    events,
    onLoad: (event: { naturalWidth?: number; naturalHeight?: number }) =>
      events.push({ type: "load", width: event.naturalWidth, height: event.naturalHeight }),
    onError: (event: { value?: string }) => events.push({ type: "error", reason: event.value }),
  }
}

async function mount(src: string | undefined, extra: Record<string, unknown> = {}) {
  const testRoot = createTestRoot()
  const record = recorder()
  const element = (source: string | undefined) => (
    <img src={source} onLoad={record.onLoad} onError={record.onError} style={{ width: 60, height: 30 }} {...extra} />
  )
  testRoot.render(element(src))
  return { testRoot, record, element }
}

describeNative("img onLoad and onError", () => {
  it("fires load once with the natural size for a path", async () => {
    const { testRoot, record } = await mount(redPath)
    try {
      await settle(testRoot, () => record.events.length > 0)
      // Keep painting: nothing fires a second time.
      for (let i = 0; i < 5; i++) testRoot.renderer.flush()
      await settle(testRoot, () => false, 100)
      expect(record.events).toEqual([{ type: "load", width: 6, height: 3 }])
    } finally {
      testRoot.unmount()
    }
  })

  it("accepts a file:// URL, including escaped characters", async () => {
    const spaced = `${SHOTS_DIR}/img load spaced.bmp`
    fs.writeFileSync(spaced, RED_6x3)
    for (const url of [pathToFileURL(redPath).href, pathToFileURL(spaced).href]) {
      const { testRoot, record } = await mount(url)
      try {
        await settle(testRoot, () => record.events.length > 0)
        expect(record.events).toEqual([{ type: "load", width: 6, height: 3 }])
      } finally {
        testRoot.unmount()
      }
    }
  })

  it("fires load for a data URL", async () => {
    const { testRoot, record } = await mount(`data:image/bmp;base64,${RED_6x3.toString("base64")}`)
    try {
      await settle(testRoot, () => record.events.length > 0)
      expect(record.events).toEqual([{ type: "load", width: 6, height: 3 }])
    } finally {
      testRoot.unmount()
    }
  })

  it("fires error, not load, for a missing file and a bad data URL", async () => {
    for (const src of [`${SHOTS_DIR}/does-not-exist.png`, "file:///does/not/exist.png", "data:image/png;base64,@@@"]) {
      const { testRoot, record } = await mount(src)
      try {
        await settle(testRoot, () => record.events.length > 0)
        for (let i = 0; i < 3; i++) testRoot.renderer.flush()
        expect(record.events.map((event) => event.type)).toEqual(["error"])
        expect(record.events[0]?.reason).toBeTruthy()
      } finally {
        testRoot.unmount()
      }
    }
  })

  it("fires nothing for an empty or missing src", async () => {
    for (const src of ["", undefined]) {
      const { testRoot, record } = await mount(src)
      try {
        await settle(testRoot, () => false, 150)
        expect(record.events).toEqual([])
      } finally {
        testRoot.unmount()
      }
    }
  })

  it("fires again for each src change, but not for the same src", async () => {
    const { testRoot, record, element } = await mount(redPath)
    try {
      await settle(testRoot, () => record.events.length >= 1)
      testRoot.render(element(redPath))
      await settle(testRoot, () => false, 100)
      expect(record.events).toHaveLength(1)
      testRoot.render(element(bluePath))
      await settle(testRoot, () => record.events.length >= 2)
      testRoot.render(element(`${SHOTS_DIR}/nope.bmp`))
      await settle(testRoot, () => record.events.length >= 3)
      expect(record.events.map((event) => event.type)).toEqual(["load", "load", "error"])
      expect(record.events[1]).toEqual({ type: "load", width: 10, height: 5 })
    } finally {
      testRoot.unmount()
    }
  })

  it("reports an svg's intrinsic size in CSS px, not its supersampled raster", async () => {
    const dataUrl = `data:image/svg+xml;base64,${Buffer.from(SVG_10x6).toString("base64")}`
    for (const src of [dataUrl, svgPath, pathToFileURL(svgPath).href]) {
      const { testRoot, record } = await mount(src)
      try {
        await settle(testRoot, () => record.events.length > 0)
        expect(record.events).toEqual([{ type: "load", width: 10, height: 6 }])
      } finally {
        testRoot.unmount()
      }
    }
  })

  it("works with only onLoad or only onError attached", async () => {
    const testRoot = createTestRoot()
    const seen: string[] = []
    try {
      testRoot.render(<img src={redPath} onLoad={() => seen.push("load")} style={{ width: 60, height: 30 }} />)
      await settle(testRoot, () => seen.length > 0)
      expect(seen).toEqual(["load"])
    } finally {
      testRoot.unmount()
    }
  })
})

describeNative("img blob: URLs", () => {
  it("loads a blob, fires load and paints it", async () => {
    const url = URL.createObjectURL(new Blob([RED_6x3]))
    const { testRoot, record } = await mount(url, { objectFit: "fill" })
    try {
      await settle(testRoot, () => record.events.length > 0)
      expect(record.events).toEqual([{ type: "load", width: 6, height: 3 }])
      const file = `${SHOTS_DIR}/img-load-blob.png`
      testRoot.renderer.captureScreenshot(file)
      expect(readLogicalPixels(file, testRoot.renderer).at(30, 15)).toEqual([255, 0, 0, 255])
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(url)
    }
  })

  it("fires error for a revoked blob and for bytes that are not an image", async () => {
    const revoked = URL.createObjectURL(new Blob([RED_6x3]))
    URL.revokeObjectURL(revoked)
    const garbage = URL.createObjectURL(new Blob([Buffer.from("not an image")]))
    for (const src of [revoked, garbage]) {
      const { testRoot, record } = await mount(src)
      try {
        await settle(testRoot, () => record.events.length > 0)
        expect(record.events.map((event) => event.type)).toEqual(["error"])
        expect(record.events[0]?.reason).toBeTruthy()
      } finally {
        testRoot.unmount()
      }
    }
    URL.revokeObjectURL(garbage)
  })

  it("keeps only the latest src when it changes while a blob is loading", async () => {
    const first = URL.createObjectURL(new Blob([RED_6x3]))
    const { testRoot, record, element } = await mount(first)
    try {
      testRoot.render(element(bluePath))
      await settle(testRoot, () => record.events.length > 0)
      await settle(testRoot, () => false, 150)
      expect(record.events).toEqual([{ type: "load", width: 10, height: 5 }])
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(first)
    }
  })

  it("fires error for each new blob that fails, though the message is the same", async () => {
    const first = URL.createObjectURL(new Blob([Buffer.from("not an image")]))
    const second = URL.createObjectURL(new Blob([Buffer.from("also not an image")]))
    const { testRoot, record, element } = await mount(first)
    try {
      await settle(testRoot, () => record.events.length >= 1)
      testRoot.render(element(second))
      await settle(testRoot, () => record.events.length >= 2)
      expect(record.events.map((event) => event.type)).toEqual(["error", "error"])
      expect(record.events[1]?.reason).toBe(record.events[0]?.reason)
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(first)
      URL.revokeObjectURL(second)
    }
  })

  it("fires error again when src moves away from a failing blob and back", async () => {
    const bad = URL.createObjectURL(new Blob([Buffer.from("not an image")]))
    const { testRoot, record, element } = await mount(bad)
    try {
      await settle(testRoot, () => record.events.length >= 1)
      testRoot.render(element(bluePath))
      await settle(testRoot, () => record.events.length >= 2)
      testRoot.render(element(bad))
      await settle(testRoot, () => record.events.length >= 3)
      expect(record.events.map((event) => event.type)).toEqual(["error", "load", "error"])
      testRoot.render(element(bad))
      await settle(testRoot, () => false, 150)
      expect(record.events).toHaveLength(3)
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(bad)
    }
  })

  it("swaps between blobs", async () => {
    const red = URL.createObjectURL(new Blob([RED_6x3]))
    const blue = URL.createObjectURL(new Blob([bmp(10, 5, [0, 0, 255])]))
    const { testRoot, record, element } = await mount(red)
    try {
      await settle(testRoot, () => record.events.length >= 1)
      testRoot.render(element(blue))
      await settle(testRoot, () => record.events.length >= 2)
      expect(record.events).toEqual([
        { type: "load", width: 6, height: 3 },
        { type: "load", width: 10, height: 5 },
      ])
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(red)
      URL.revokeObjectURL(blue)
    }
  })
})
