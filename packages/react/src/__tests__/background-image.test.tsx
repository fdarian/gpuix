import fs from "fs"
import { pathToFileURL } from "url"
import zlib from "zlib"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
type TestRoot = ReturnType<typeof createTestRoot>
type Sampler = (x: number, y: number) => Rgba

const RED: Rgba = [255, 0, 0, 255]
const GREEN: Rgba = [0, 255, 0, 255]
const BLUE: Rgba = [0, 0, 255, 255]
const WHITE: Rgba = [255, 255, 255, 255]

function svg(width: number, height: number, body: string) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">${body}</svg>`
}

function dataUrl(text: string) {
  return `data:image/svg+xml;base64,${Buffer.from(text).toString("base64")}`
}

/** A `width` x `height` image of one colour, as the address inside `url()`. */
function solid(width: number, height: number, fill: string) {
  return dataUrl(svg(width, height, `<rect width="${width}" height="${height}" fill="${fill}"/>`))
}

/** Left half `left`, right half `right`. */
function halves(width: number, height: number, left: string, right: string) {
  return dataUrl(
    svg(
      width,
      height,
      `<rect width="${width / 2}" height="${height}" fill="${left}"/><rect x="${width / 2}" width="${width / 2}" height="${height}" fill="${right}"/>`
    )
  )
}

const url = (address: string) => `url(${address})`

const crcTable = Array.from({ length: 256 }, (_, n) => {
  let c = n
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
  return c >>> 0
})

function crc32(bytes: Buffer) {
  let c = 0xffffffff
  for (const byte of bytes) c = crcTable[(c ^ byte) & 0xff]! ^ (c >>> 8)
  return (c ^ 0xffffffff) >>> 0
}

/** A solid-colour PNG, built here so no encoder is needed. */
function png(width: number, height: number, [r, g, b]: [number, number, number]) {
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data])
    const head = Buffer.alloc(4)
    head.writeUInt32BE(data.length)
    const tail = Buffer.alloc(4)
    tail.writeUInt32BE(crc32(body))
    return Buffer.concat([head, body, tail])
  }
  const header = Buffer.alloc(13)
  header.writeUInt32BE(width, 0)
  header.writeUInt32BE(height, 4)
  header.set([8, 2, 0, 0, 0], 8)
  const row = Buffer.concat([Buffer.from([0]), Buffer.from(Array.from({ length: width }, () => [r, g, b]).flat())])
  const raw = Buffer.concat(Array.from({ length: height }, () => row))
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", zlib.deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ])
}

let counter = 0

function capture(testRoot: TestRoot): Sampler {
  const file = `${SHOTS_DIR}/background-image-${counter++}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  const pixels = readLogicalPixels(file, testRoot.renderer)
  return (x, y) => pixels.at(x, y)
}

/** A white 320x240 stage with a 100x100 box at its corner, `style` on the box. */
function stage(style: Record<string, unknown>, children?: ReactElement) {
  return (
    <div style={{ width: 320, height: 240, backgroundColor: "#ffffff" }}>
      <div style={{ width: 100, height: 100, boxSizing: "border-box", ...style }}>{children}</div>
    </div>
  )
}

function paint(tree: ReactElement): Sampler {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    testRoot.renderer.dispatchNativeEvents()
    return capture(testRoot)
  } finally {
    testRoot.unmount()
  }
}

function near(actual: Rgba, expected: Rgba, tolerance = 6) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
  }
}

function isNear(actual: Rgba, expected: Rgba, tolerance = 6) {
  return actual.every((channel, index) => Math.abs(channel - expected[index]!) <= tolerance)
}

/** Draws until `done` holds for a fresh capture, for an image that loads off the frame. */
async function until(testRoot: TestRoot, done: (at: Sampler) => boolean, ms = 4000) {
  const deadline = Date.now() + ms
  let at = capture(testRoot)
  while (!done(at) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 15))
    testRoot.renderer.flush()
    testRoot.renderer.dispatchNativeEvents()
    at = capture(testRoot)
  }
  return at
}

describeNative("url() in backgroundImage", () => {
  it("places one tile with no-repeat and a position", () => {
    const at = paint(
      stage({
        backgroundImage: url(solid(20, 20, "#ff0000")),
        backgroundRepeat: "no-repeat",
        backgroundPosition: "30px 40px",
      })
    )
    near(at(40, 50), RED)
    near(at(31, 41), RED)
    near(at(10, 10), WHITE)
    near(at(55, 50), WHITE)
    near(at(40, 65), WHITE)
  })

  it("repeats the tile over the box and no further", () => {
    const at = paint(stage({ backgroundImage: url(halves(20, 20, "#ff0000", "#0000ff")) }))
    near(at(5, 5), RED)
    near(at(15, 5), BLUE)
    near(at(25, 5), RED)
    near(at(85, 95), RED)
    near(at(95, 95), BLUE)
    near(at(105, 5), WHITE)
    near(at(5, 105), WHITE)
  })

  it("spreads whole tiles apart with space", () => {
    const at = paint(
      stage({ backgroundImage: url(solid(30, 30, "#ff0000")), backgroundRepeat: "space" })
    )
    // Three tiles across 100px leave 10px, 5px between each.
    near(at(15, 15), RED)
    near(at(32, 15), WHITE)
    near(at(50, 15), RED)
    near(at(67, 15), WHITE)
    near(at(85, 85), RED)
  })

  it("stretches the tiles to a whole number with round", () => {
    const at = paint(
      stage({ backgroundImage: url(halves(30, 30, "#ff0000", "#0000ff")), backgroundRepeat: "round" })
    )
    // 100px holds three 33.3px tiles. Unrounded, x=48 would be in the blue half of the second.
    near(at(10, 10), RED)
    near(at(25, 10), BLUE)
    near(at(48, 10), RED)
    near(at(60, 10), BLUE)
    near(at(90, 10), BLUE)
  })

  it("sizes with cover, contain, explicit lengths and one length", () => {
    const image = url(solid(20, 10, "#ff0000"))
    const base = { backgroundImage: image, backgroundRepeat: "no-repeat" }
    const contain = paint(stage({ ...base, backgroundSize: "contain" }))
    near(contain(50, 25), RED)
    near(contain(50, 75), WHITE)
    const cover = paint(stage({ ...base, backgroundSize: "cover" }))
    near(cover(50, 25), RED)
    near(cover(95, 95), RED)
    const explicit = paint(stage({ ...base, backgroundSize: "40px 20px" }))
    near(explicit(30, 15), RED)
    near(explicit(50, 15), WHITE)
    near(explicit(30, 25), WHITE)
    const oneLength = paint(stage({ ...base, backgroundSize: "40px" }))
    near(oneLength(30, 15), RED)
    near(oneLength(30, 25), WHITE)
    const percent = paint(stage({ ...base, backgroundSize: "50% auto" }))
    near(percent(40, 20), RED)
    near(percent(40, 30), WHITE)
    near(percent(60, 20), WHITE)
    const natural = paint(stage({ ...base }))
    near(natural(15, 5), RED)
    near(natural(25, 5), WHITE)
  })

  it("starts the tiles at the origin box", () => {
    const box = { padding: 10, borderWidth: 10, borderStyle: "solid", borderColor: "transparent" }
    const image = url(solid(20, 20, "#ff0000"))
    const paddingBox = paint(stage({ ...box, backgroundImage: image, backgroundRepeat: "no-repeat" }))
    near(paddingBox(15, 15), RED)
    near(paddingBox(25, 25), RED)
    const contentBox = paint(
      stage({ ...box, backgroundImage: image, backgroundRepeat: "no-repeat", backgroundOrigin: "content-box" })
    )
    near(contentBox(15, 15), WHITE)
    near(contentBox(25, 25), RED)
    const borderBox = paint(
      stage({ ...box, backgroundImage: image, backgroundRepeat: "no-repeat", backgroundOrigin: "border-box" })
    )
    near(borderBox(5, 5), RED)
  })

  it("cuts the tiles to the clip box", () => {
    const box = { padding: 10, borderWidth: 10, borderStyle: "solid", borderColor: "transparent" }
    const image = url(solid(20, 20, "#ff0000"))
    const border = paint(stage({ ...box, backgroundImage: image }))
    near(border(5, 5), RED)
    near(border(15, 15), RED)
    const padding = paint(stage({ ...box, backgroundImage: image, backgroundClip: "padding-box" }))
    near(padding(5, 5), WHITE)
    near(padding(15, 15), RED)
    const content = paint(stage({ ...box, backgroundImage: image, backgroundClip: "content-box" }))
    near(content(15, 15), WHITE)
    near(content(25, 25), RED)
    near(content(95, 95), WHITE)
  })

  it("rounds the tiles at the corners of a borderRadius", () => {
    const at = paint(stage({ backgroundImage: url(solid(20, 20, "#ff0000")), borderRadius: 30 }))
    near(at(2, 2), WHITE)
    near(at(97, 97), WHITE)
    near(at(50, 2), RED)
    near(at(2, 50), RED)
    near(at(50, 50), RED)
    near(at(12, 12), RED)
  })

  it("paints the first layer on top, a gradient above an image included", () => {
    const stacked = paint(
      stage({
        backgroundImage: `${url(solid(20, 20, "#ff0000"))}, ${url(solid(20, 20, "#0000ff"))}`,
        backgroundSize: "100px 100px",
        backgroundRepeat: "no-repeat",
      })
    )
    near(stacked(50, 50), RED)
    const mixed = paint(
      stage({
        backgroundImage: `linear-gradient(to right, #00ff00 0 50%, transparent 50%), ${url(solid(20, 20, "#0000ff"))}`,
        backgroundSize: "100% 100%",
      })
    )
    near(mixed(25, 50), GREEN)
    near(mixed(75, 50), BLUE)
    const underneath = paint(
      stage({
        backgroundImage: `${url(solid(20, 20, "#ff0000"))}, linear-gradient(#00ff00, #00ff00)`,
        backgroundSize: "50px 100px, 100% 100%",
        backgroundRepeat: "no-repeat",
      })
    )
    near(underneath(25, 50), RED)
    near(underneath(75, 50), GREEN)
  })

  it("takes a url anywhere in the background shorthand, with its layer parts", () => {
    const at = paint(
      stage({
        background: `no-repeat 20px 30px / 40px 40px ${url(solid(20, 20, "#ff0000"))}, ${url(solid(20, 20, "#0000ff"))} content-box, #00ff00`,
        padding: 10,
      })
    )
    near(at(40, 50), RED)
    // The second layer repeats over the content box.
    near(at(15, 15), BLUE)
    near(at(85, 85), BLUE)
    // The last layer is the colour alone, so its clip is the border box.
    near(at(5, 5), GREEN)
    near(at(95, 95), GREEN)
    const bare = paint(stage({ background: url(solid(20, 20, "#ff0000")) }))
    near(bare(5, 5), RED)
    near(bare(95, 95), RED)
    near(bare(105, 5), WHITE)
  })

  it("reads a PNG as well as an SVG", () => {
    const address = `data:image/png;base64,${png(2, 2, [0, 128, 0]).toString("base64")}`
    const at = paint(stage({ backgroundImage: url(address), backgroundSize: "100px 100px" }))
    near(at(50, 50), [0, 128, 0, 255])
  })

  it("swaps the image on hover, on the first hovered frame", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        stage({
          backgroundImage: url(solid(20, 20, "#ff0000")),
          hover: { backgroundImage: url(solid(20, 20, "#0000ff")) },
        })
      )
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      near(capture(testRoot)(50, 50), RED)
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      near(capture(testRoot)(50, 50), BLUE)
      testRoot.renderer.nativeSimulateMouseMove(500, 500)
      near(capture(testRoot)(50, 50), RED)
    } finally {
      testRoot.unmount()
    }
  })

  it("resets the base layers when a hover declares the background shorthand", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        stage({
          backgroundImage: url(solid(20, 20, "#ff0000")),
          hover: { background: `${url(solid(20, 20, "#0000ff"))} no-repeat 0 0 / 50px 50px` },
        })
      )
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const hovered = capture(testRoot)
      near(hovered(25, 25), BLUE)
      near(hovered(75, 75), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("loads the new image when the address changes", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(stage({ backgroundImage: url(solid(20, 20, "#ff0000")) }))
      near(capture(testRoot)(50, 50), RED)
      testRoot.render(stage({ backgroundImage: url(solid(20, 20, "#0000ff")) }))
      near(capture(testRoot)(50, 50), BLUE)
      testRoot.render(stage({ backgroundImage: url(solid(20, 20, "#ff0000")) }))
      near(capture(testRoot)(50, 50), RED)
      testRoot.render(stage({ backgroundImage: "none" }))
      near(capture(testRoot)(50, 50), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("shows an image inside the glyphs with backgroundClip: text", () => {
    const at = paint(
      <div style={{ width: 300, height: 100, backgroundColor: "#ffffff" }}>
        <div
          style={{
            width: 300,
            height: 100,
            backgroundImage: url(solid(20, 20, "#ff0000")),
            backgroundClip: "text",
            color: "transparent",
          }}
        >
          <text style={{ fontSize: 80, lineHeight: "90px", fontWeight: 900 }}>MMM</text>
        </div>
      </div>
    )
    let red = 0
    let other = 0
    for (let y = 0; y < 100; y++) {
      for (let x = 0; x < 300; x++) {
        const [r, g, b] = at(x, y)
        if (r > 240 && g < 20 && b < 20) red++
        else if (r < 200 || g < 200 || b < 200) other++
      }
    }
    expect(red).toBeGreaterThan(200)
    // Only the anti-aliased edges of the glyphs are anything else.
    expect(other).toBeLessThan(red)
    near(at(1, 1), WHITE)
    near(at(298, 98), WHITE)
  })

  it("paints in a virtual-list row, which is built in list layout", async () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <virtual-list overdraw={0} estimatedItemHeight={100} style={{ width: 200, height: 200 }}>
          <div
            key={0}
            style={{ width: 200, height: 100, flexShrink: 0, backgroundImage: url(solid(20, 20, "#ff0000")) }}
          />
          <div
            key={1}
            style={{ width: 200, height: 100, flexShrink: 0, backgroundImage: url(solid(20, 20, "#0000ff")) }}
          />
        </virtual-list>
      )
      const at = await until(testRoot, (sample) => isNear(sample(50, 50), RED) && isNear(sample(50, 150), BLUE))
      near(at(50, 50), RED)
      near(at(150, 150), BLUE)
    } finally {
      testRoot.unmount()
    }
  })

  it("still does not paint a url() in a mask", () => {
    const at = paint(
      stage({ backgroundColor: "#ff0000", maskImage: `${url(solid(20, 20, "#000000"))}, linear-gradient(#000, #000)` })
    )
    near(at(50, 50), RED)
    const only = paint(stage({ backgroundColor: "#ff0000", maskImage: url(solid(20, 20, "#000000")) }))
    near(only(50, 50), RED)
  })
})

describeNative("url() backgrounds that load off the frame", () => {
  const files = `${SHOTS_DIR}/background-image-files`
  fs.mkdirSync(files, { recursive: true })
  const redFile = `${files}/red 20.svg`
  fs.writeFileSync(redFile, svg(20, 20, '<rect width="20" height="20" fill="#ff0000"/>'))
  const bluePng = `${files}/blue.png`
  fs.writeFileSync(bluePng, png(4, 4, [0, 0, 255]))

  it("loads a file: URL and paints it once it has", async () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(stage({ backgroundImage: url(`"${pathToFileURL(redFile).href}"`) }))
      const at = await until(testRoot, (sample) => isNear(sample(50, 50), RED))
      near(at(50, 50), RED)
      near(at(105, 50), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("loads a path, and a file that changes with the address", async () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(stage({ background: `${url(`"${bluePng}"`)} 0 0 / 100px 100px` }))
      let at = await until(testRoot, (sample) => isNear(sample(50, 50), BLUE))
      near(at(50, 50), BLUE)
      testRoot.render(stage({ background: `${url(`'${redFile}'`)} 0 0 / 100px 100px` }))
      at = await until(testRoot, (sample) => isNear(sample(50, 50), RED))
      near(at(50, 50), RED)
    } finally {
      testRoot.unmount()
    }
  })

  it("loads an http: URL through the renderer's HTTP client", async () => {
    // The test renderer answers every request with one SVG (240x140, a #1e2d59 field with a
    // yellow dot at 68,70) and has no real network, so this proves the URI path, not a server.
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div style={{ width: 320, height: 240, backgroundColor: "#ffffff" }}>
          <div
            style={{
              width: 300,
              height: 200,
              backgroundImage: url("http://example.test/cover.png"),
              backgroundRepeat: "no-repeat",
            }}
          />
        </div>
      )
      const at = await until(testRoot, (sample) => sample(5, 5)[0] !== 255)
      near(at(5, 5), [30, 45, 89, 255], 8)
      near(at(68, 70), [255, 209, 102, 255], 8)
      near(at(280, 190), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("reads a blob: URL from JS", async () => {
    const address = URL.createObjectURL(new Blob([svg(20, 20, '<rect width="20" height="20" fill="#ff0000"/>')]))
    const testRoot = createTestRoot()
    try {
      testRoot.render(stage({ backgroundImage: url(address) }))
      const at = await until(testRoot, (sample) => isNear(sample(50, 50), RED))
      near(at(50, 50), RED)
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(address)
    }
  })

  it("reads a blob: URL that only a hover style names, in a quoted shorthand", async () => {
    const address = URL.createObjectURL(new Blob([svg(20, 20, '<rect width="20" height="20" fill="#0000ff"/>')]))
    const testRoot = createTestRoot()
    try {
      testRoot.render(stage({ backgroundColor: "#00ff00", hover: { background: `url("${address}") repeat` } }))
      await new Promise((resolve) => setTimeout(resolve, 200))
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const at = await until(testRoot, (sample) => isNear(sample(50, 50), BLUE))
      near(at(50, 50), BLUE)
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(address)
    }
  })

  it("paints nothing, and does not throw, for a revoked blob, a missing file and bad data", async () => {
    const revoked = URL.createObjectURL(new Blob(["x"]))
    URL.revokeObjectURL(revoked)
    const testRoot = createTestRoot()
    try {
      for (const address of [revoked, "/nonexistent/background-image.png", "data:image/png;base64,***", "data:image/png;base64,AAAA"]) {
        testRoot.render(stage({ backgroundImage: url(`"${address}"`), backgroundColor: "#ffff00" }))
        await new Promise((resolve) => setTimeout(resolve, 100))
        testRoot.renderer.flush()
        testRoot.renderer.dispatchNativeEvents()
        // The box keeps its colour: a failed image is an empty layer, not an error.
        near(capture(testRoot)(50, 50), [255, 255, 0, 255])
      }
    } finally {
      testRoot.unmount()
    }
  })
})
