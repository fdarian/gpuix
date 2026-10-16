import fs from "fs"
import { pathToFileURL } from "url"
import zlib from "zlib"
import type { ReactElement } from "react"
import { describe, expect, it } from "vitest"
import { createTestRoot, hasNativeTestRenderer } from "../testing.js"
import { expectScreenshotsEqual, readLogicalPixels, SHOTS_DIR } from "./test-utils.js"

const describeNative = hasNativeTestRenderer ? describe : describe.skip

type Rgba = [number, number, number, number]
type TestRoot = ReturnType<typeof createTestRoot>
type Sampler = (x: number, y: number) => Rgba

const RED: Rgba = [255, 0, 0, 255]
const WHITE: Rgba = [255, 255, 255, 255]
/** Red at half alpha over the white stage. */
const HALF_RED: Rgba = [255, 127, 127, 255]

function svg(width: number, height: number, body: string) {
  return `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">${body}</svg>`
}

function dataUrl(text: string) {
  return `data:image/svg+xml;base64,${Buffer.from(text).toString("base64")}`
}

function solid(width: number, height: number, fill: string) {
  return dataUrl(svg(width, height, `<rect width="${width}" height="${height}" fill="${fill}"/>`))
}

/** The left half of each 20px tile is `fill`, the right half is transparent. */
const LEFT_HALF = dataUrl(svg(20, 20, '<rect width="10" height="20" fill="#000000"/>'))

/** 2x2 cells of 20px: white at the top left and bottom right, black at the other two. */
const CHECKER = dataUrl(
  svg(
    40,
    40,
    '<rect width="20" height="20" fill="#ffffff"/><rect x="20" y="20" width="20" height="20" fill="#ffffff"/>' +
      '<rect x="20" width="20" height="20" fill="#000000"/><rect y="20" width="20" height="20" fill="#000000"/>'
  )
)

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

/** A 1x1 RGBA PNG, built here so no encoder is needed. */
function pixelPng(r: number, g: number, b: number, a: number) {
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data])
    const head = Buffer.alloc(4)
    head.writeUInt32BE(data.length)
    const tail = Buffer.alloc(4)
    tail.writeUInt32BE(crc32(body))
    return Buffer.concat([head, body, tail])
  }
  const header = Buffer.alloc(13)
  header.writeUInt32BE(1, 0)
  header.writeUInt32BE(1, 4)
  header.set([8, 6, 0, 0, 0], 8)
  const raw = Buffer.from([0, r, g, b, a])
  const bytes = Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", zlib.deflateSync(raw)),
    chunk("IEND", Buffer.alloc(0)),
  ])
  return `data:image/png;base64,${bytes.toString("base64")}`
}

const url = (address: string) => `url(${address})`
const BLACK = url(solid(20, 20, "#000000"))
const HALF_BLACK = "linear-gradient(rgba(0,0,0,0.5), rgba(0,0,0,0.5))"
const OPAQUE = "linear-gradient(#000, #000)"

let counter = 0

function capture(testRoot: TestRoot): { file: string; at: Sampler } {
  const file = `${SHOTS_DIR}/mask-image-${counter++}.png`
  fs.rmSync(file, { force: true })
  testRoot.renderer.captureScreenshot(file)
  const pixels = readLogicalPixels(file, testRoot.renderer)
  return { file, at: (x, y) => pixels.at(x, y) }
}

/** A white 200x200 stage with a 100x200 red column at (50, 0) inside the masked box. */
function masked(style: Record<string, unknown>): ReactElement {
  return (
    <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
      <div style={{ position: "absolute", left: 50, top: 0, width: 100, height: 200, ...style }}>
        <div style={{ width: 100, height: 200, backgroundColor: "#ff0000" }} />
      </div>
    </div>
  )
}

function shoot(tree: ReactElement) {
  const testRoot = createTestRoot()
  try {
    testRoot.render(tree)
    testRoot.renderer.dispatchNativeEvents()
    return capture(testRoot)
  } finally {
    testRoot.unmount()
  }
}

function near(actual: Rgba, expected: Rgba, tolerance = 8) {
  for (let i = 0; i < 4; i++) {
    expect(Math.abs(actual[i]! - expected[i]!), `${actual} vs ${expected}`).toBeLessThanOrEqual(tolerance)
  }
}

function isNear(actual: Rgba, expected: Rgba, tolerance = 8) {
  return actual.every((channel, index) => Math.abs(channel - expected[index]!) <= tolerance)
}

/** Draws until `done` holds for a fresh capture, for an image that loads off the frame. */
async function until(testRoot: TestRoot, done: (at: Sampler) => boolean, ms = 4000) {
  const deadline = Date.now() + ms
  let at = capture(testRoot).at
  while (!done(at) && Date.now() < deadline) {
    await new Promise((resolve) => setTimeout(resolve, 15))
    testRoot.renderer.flush()
    testRoot.renderer.dispatchNativeEvents()
    at = capture(testRoot).at
  }
  return at
}

describeNative("url() in maskImage", () => {
  it("shows only the tile of a no-repeat image mask", () => {
    const { at } = shoot(
      masked({
        maskImage: BLACK,
        maskSize: "40px 40px",
        maskPosition: "20px 30px",
        maskRepeat: "no-repeat",
      })
    )
    near(at(90, 50), RED)
    near(at(73, 33), RED)
    near(at(60, 50), WHITE)
    near(at(90, 20), WHITE)
    near(at(120, 50), WHITE)
    near(at(90, 80), WHITE)
    near(at(20, 50), WHITE)
  })

  it("repeats the image over the box and keeps what its transparent part covers", () => {
    const { at } = shoot(masked({ maskImage: url(LEFT_HALF) }))
    near(at(55, 10), RED)
    near(at(65, 10), WHITE)
    near(at(75, 10), RED)
    near(at(85, 10), WHITE)
    near(at(145, 190), WHITE)
    near(at(135, 190), RED)
    near(at(30, 10), WHITE)
  })

  it("sizes with contain and cover", () => {
    const image = url(solid(20, 10, "#000000"))
    const contain = shoot(masked({ maskImage: image, maskSize: "contain", maskRepeat: "no-repeat" })).at
    near(contain(100, 25), RED)
    near(contain(100, 100), WHITE)
    const cover = shoot(masked({ maskImage: image, maskSize: "cover", maskRepeat: "no-repeat" })).at
    near(cover(100, 100), RED)
    near(cover(70, 150), RED)
    near(cover(130, 60), RED)
    near(cover(30, 100), WHITE)
  })

  it("positions the tile with a keyword", () => {
    const { at } = shoot(
      masked({ maskImage: url(solid(40, 40, "#000000")), maskPosition: "center", maskRepeat: "no-repeat" })
    )
    near(at(100, 100), RED)
    near(at(82, 82), RED)
    near(at(100, 60), WHITE)
    near(at(65, 100), WHITE)
  })

  it("reads the alpha of a translucent PNG", () => {
    const half = url(pixelPng(0, 0, 0, 128))
    const fill = { maskImage: half, maskSize: "100px 200px", maskRepeat: "no-repeat" }
    near(shoot(masked(fill)).at(100, 100), HALF_RED)
    near(shoot(masked({ ...fill, maskMode: "alpha" })).at(100, 100), HALF_RED)
    // A black pixel has no luminance, whatever its alpha.
    near(shoot(masked({ ...fill, maskMode: "luminance" })).at(100, 100), WHITE)
    const white = url(pixelPng(255, 255, 255, 128))
    near(shoot(masked({ ...fill, maskImage: white, maskMode: "luminance" })).at(100, 100), HALF_RED)
  })
})

describeNative("maskMode", () => {
  const checker = { maskImage: url(CHECKER), maskSize: "40px 40px", maskRepeat: "no-repeat" }

  it("reads the same image by alpha or by luminance", () => {
    for (const mode of [undefined, "match-source", "alpha", "ALPHA"]) {
      const { at } = shoot(masked({ ...checker, maskMode: mode }))
      near(at(60, 10), RED)
      near(at(80, 10), RED)
      near(at(60, 30), RED)
      near(at(80, 30), RED)
      near(at(120, 10), WHITE)
    }
    const { at } = shoot(masked({ ...checker, maskMode: "luminance" }))
    near(at(60, 10), RED)
    near(at(80, 30), RED)
    near(at(80, 10), WHITE)
    near(at(60, 30), WHITE)
    near(at(120, 10), WHITE)
  })

  it("reads a gradient by luminance too", () => {
    const gradient = "linear-gradient(to right, #fff 0 50%, #000 50% 100%)"
    const { at } = shoot(masked({ maskImage: gradient, maskMode: "luminance" }))
    near(at(75, 100), RED)
    near(at(125, 100), WHITE)
    const alpha = shoot(masked({ maskImage: gradient, maskMode: "alpha" })).at
    near(alpha(75, 100), RED)
    near(alpha(125, 100), RED)
  })

  it("repeats the list over the layers", () => {
    const layers = {
      maskImage: `${url(CHECKER)}, ${url(CHECKER)}`,
      maskSize: "40px 40px, 40px 40px",
      maskPosition: "0 0, 0 100px",
      maskRepeat: "no-repeat",
    }
    const one = shoot(masked({ ...layers, maskMode: "luminance" })).at
    near(one(80, 10), WHITE)
    near(one(60, 10), RED)
    near(one(80, 110), WHITE)
    near(one(60, 110), RED)
    const two = shoot(masked({ ...layers, maskMode: "luminance, alpha" })).at
    near(two(80, 10), WHITE)
    near(two(80, 110), RED)
  })

  it("ignores a list with an invalid entry", () => {
    const plain = shoot(masked({ ...checker, maskMode: "alpha" })).file
    expectScreenshotsEqual(plain, shoot(masked({ ...checker, maskMode: "luminance, bogus" })).file)
    expectScreenshotsEqual(plain, shoot(masked({ ...checker, maskMode: "add" })).file)
  })

  it("is not a mask on its own", () => {
    const plain = shoot(masked({})).file
    expectScreenshotsEqual(plain, shoot(masked({ maskMode: "luminance", maskComposite: "intersect" })).file)
  })
})

describeNative("maskComposite", () => {
  /** A fully opaque image over x 0..60 of the box, and a half-opaque gradient over x 40..100. */
  const top = { maskImage: `${BLACK}, ${HALF_BLACK}`, maskSize: "60px 200px", maskRepeat: "no-repeat" }
  const placed = { ...top, maskPosition: "0 0, 40px 0" }

  /** The top layer alone is x 0..40, both are x 40..60, the bottom alone is x 60..100. */
  const regions = (at: Sampler) => [at(70, 100), at(100, 100), at(130, 100)]

  const expected: Record<string, [Rgba, Rgba, Rgba]> = {
    add: [RED, RED, HALF_RED],
    subtract: [RED, HALF_RED, WHITE],
    intersect: [WHITE, HALF_RED, WHITE],
    exclude: [RED, HALF_RED, HALF_RED],
  }

  for (const [operator, colours] of Object.entries(expected)) {
    it(`combines an image over a gradient with ${operator}`, () => {
      const actual = regions(shoot(masked({ ...placed, maskComposite: operator })).at)
      colours.forEach((colour, index) => near(actual[index]!, colour))
    })
  }

  it("is add by default and with a bad value", () => {
    const base = shoot(masked(placed)).file
    expectScreenshotsEqual(base, shoot(masked({ ...placed, maskComposite: "add" })).file)
    expectScreenshotsEqual(base, shoot(masked({ ...placed, maskComposite: "source-over" })).file)
    expectScreenshotsEqual(base, shoot(masked({ ...placed, maskComposite: "subtract, nope" })).file)
  })

  it("combines a gradient over an image, in that order", () => {
    // The gradient is half opaque over x 40..100 and the image fully opaque over x 0..60.
    const swapped = {
      maskImage: `${HALF_BLACK}, ${BLACK}`,
      maskSize: "60px 200px",
      maskPosition: "40px 0, 0 0",
      maskRepeat: "no-repeat",
      maskComposite: "subtract",
    }
    const actual = regions(shoot(masked(swapped)).at)
    near(actual[0]!, WHITE)
    near(actual[1]!, WHITE)
    near(actual[2]!, HALF_RED)
  })

  const rects = {
    maskImage: `${OPAQUE}, ${OPAQUE}, ${OPAQUE}`,
    maskSize: "70px 200px, 70px 200px, 60px 200px",
    maskPosition: "0 0, 20px 0, 40px 0",
    maskRepeat: "no-repeat",
  }

  it("repeats the list over the layers", () => {
    // Three layers cover x 0..70, 20..90 and 40..100 of the box.
    const everywhere = shoot(masked({ ...rects, maskComposite: "intersect" })).at
    near(everywhere(65, 100), WHITE)
    near(everywhere(80, 100), WHITE)
    near(everywhere(105, 100), RED)
    near(everywhere(125, 100), WHITE)
    // The first layer intersects the union of the two below it.
    const mixed = shoot(masked({ ...rects, maskComposite: "intersect, add" })).at
    near(mixed(65, 100), WHITE)
    near(mixed(75, 100), RED)
    near(mixed(105, 100), RED)
    near(mixed(125, 100), WHITE)
  })

  it("ignores the operator of the bottom layer", () => {
    const two = {
      maskImage: `${OPAQUE}, ${BLACK}`,
      maskSize: "70px 200px, 70px 200px",
      maskPosition: "0 0, 30px 0",
      maskRepeat: "no-repeat",
    }
    const added = shoot(masked({ ...two, maskComposite: "add" }))
    near(added.at(60, 100), RED)
    near(added.at(140, 100), RED)
    expectScreenshotsEqual(added.file, shoot(masked({ ...two, maskComposite: "add, subtract" })).file)
    expectScreenshotsEqual(added.file, shoot(masked({ ...two, maskComposite: "add, intersect" })).file)
    const subtracted = shoot(masked({ ...two, maskComposite: "subtract" })).at
    near(subtracted(60, 100), RED)
    near(subtracted(100, 100), WHITE)
    // One layer has nothing below it to combine with.
    const lone = { maskImage: BLACK, maskSize: "100px 200px" }
    expectScreenshotsEqual(shoot(masked(lone)).file, shoot(masked({ ...lone, maskComposite: "exclude" })).file)
  })
})

describeNative("the mask shorthand with a mode and a composite", () => {
  const image = url(solid(20, 20, "#ffffff"))

  it("takes the keywords in any order, as the longhands do", () => {
    const longhands = shoot(
      masked({
        maskImage: `${image}, ${HALF_BLACK}`,
        maskMode: "luminance, match-source",
        maskComposite: "subtract, add",
        maskSize: "60px 200px",
        maskPosition: "0 0, 40px 0",
        maskRepeat: "no-repeat",
      })
    )
    const shorthand = shoot(
      masked({
        mask: `luminance no-repeat subtract ${image} 0 0 / 60px 200px, ${HALF_BLACK} no-repeat add 40px 0 / 60px 200px`,
      })
    )
    expectScreenshotsEqual(longhands.file, shorthand.file)
    near(shorthand.at(70, 100), RED)
    near(shorthand.at(100, 100), HALF_RED)
    near(shorthand.at(130, 100), WHITE)
    const reordered = shoot(
      masked({
        mask: `${HALF_BLACK} 40px 0 / 60px 200px add no-repeat, subtract ${image} luminance no-repeat 0 0 / 60px 200px`,
      })
    )
    // The layers swap places, so this is a different mask with the same parts.
    near(reordered.at(70, 100), RED)
    near(reordered.at(130, 100), HALF_RED)
  })

  it("reads the mode keyword", () => {
    const black = url(solid(20, 20, "#000000"))
    near(shoot(masked({ mask: `${black} luminance` })).at(100, 100), WHITE)
    near(shoot(masked({ mask: `${black} alpha` })).at(100, 100), RED)
    near(shoot(masked({ mask: `${black} match-source` })).at(100, 100), RED)
    const tile = shoot(masked({ mask: `alpha ${black} no-repeat` })).at
    near(tile(60, 10), RED)
    near(tile(100, 100), WHITE)
  })

  it("is overridden by the longhands", () => {
    const black = url(solid(20, 20, "#000000"))
    near(shoot(masked({ mask: `${black} luminance`, maskMode: "alpha" })).at(100, 100), RED)
    const shorthand = `${black} no-repeat 0 0 / 60px 200px, ${HALF_BLACK} no-repeat 40px 0 / 60px 200px`
    const overridden = shoot(masked({ mask: shorthand, maskComposite: "intersect" }))
    near(overridden.at(70, 100), WHITE)
    near(overridden.at(100, 100), HALF_RED)
  })

  it("is invalid with a repeated keyword, a colour or a legacy operator", () => {
    const plain = shoot(masked({})).file
    for (const mask of [
      `${BLACK} luminance alpha`,
      `${BLACK} add subtract`,
      `${BLACK} red`,
      `${BLACK} source-over`,
    ]) {
      expectScreenshotsEqual(plain, shoot(masked({ mask })).file)
    }
  })
})

describeNative("an unloaded mask image", () => {
  const files = `${SHOTS_DIR}/mask-image-files`
  fs.mkdirSync(files, { recursive: true })
  const blackFile = `${files}/black 20.svg`
  fs.writeFileSync(blackFile, svg(20, 20, '<rect width="20" height="20" fill="#000000"/>'))
  const missing = url('"/nonexistent/mask-image.png"')

  it("hides the element while a blob: image has not arrived, then shows it", async () => {
    const address = URL.createObjectURL(new Blob([svg(20, 20, '<rect width="20" height="20" fill="#000000"/>')]))
    const testRoot = createTestRoot()
    try {
      testRoot.render(masked({ maskImage: url(address) }))
      testRoot.renderer.dispatchNativeEvents()
      // The bytes are fetched on the JS event loop, which has not run since the render.
      near(capture(testRoot).at(100, 100), WHITE)
      const at = await until(testRoot, (sample) => isNear(sample(100, 100), RED))
      near(at(100, 100), RED)
      near(at(30, 100), WHITE)
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(address)
    }
  })

  it("shows the element once a file: image has loaded", async () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(masked({ maskImage: url(`"${pathToFileURL(blackFile).href}"`) }))
      const at = await until(testRoot, (sample) => isNear(sample(100, 100), RED))
      near(at(100, 100), RED)
      near(at(30, 100), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("keeps the element hidden when the file is missing or the data is bad", async () => {
    const testRoot = createTestRoot()
    try {
      for (const address of [missing, url('"data:image/png;base64,AAAA"'), url('"data:image/png;base64,***"')]) {
        testRoot.render(masked({ maskImage: address }))
        await new Promise((resolve) => setTimeout(resolve, 100))
        testRoot.renderer.flush()
        testRoot.renderer.dispatchNativeEvents()
        const at = capture(testRoot).at
        near(at(100, 100), WHITE)
        near(at(60, 10), WHITE)
        near(at(145, 190), WHITE)
      }
    } finally {
      testRoot.unmount()
    }
  })

  it("is a transparent layer that the other layers combine with", async () => {
    const testRoot = createTestRoot()
    try {
      const size = { maskSize: "100px 200px" }
      testRoot.render(masked({ ...size, maskImage: `${missing}, ${OPAQUE}`, maskComposite: "add" }))
      await new Promise((resolve) => setTimeout(resolve, 100))
      testRoot.renderer.flush()
      // The missing image on top adds nothing to the gradient under it.
      near(capture(testRoot).at(100, 100), RED)
      const onTop: Record<string, Rgba> = { add: RED, exclude: RED, subtract: WHITE, intersect: WHITE }
      for (const [operator, colour] of Object.entries(onTop)) {
        testRoot.render(masked({ ...size, maskImage: `${missing}, ${OPAQUE}`, maskComposite: operator }))
        near(capture(testRoot).at(100, 100), colour)
      }
      const below: Record<string, Rgba> = { add: RED, exclude: RED, subtract: RED, intersect: WHITE }
      for (const [operator, colour] of Object.entries(below)) {
        testRoot.render(masked({ ...size, maskImage: `${OPAQUE}, ${missing}`, maskComposite: operator }))
        near(capture(testRoot).at(100, 100), colour)
      }
    } finally {
      testRoot.unmount()
    }
  })

  it("reads a blob: image that only a hover mask names", async () => {
    const address = URL.createObjectURL(new Blob([svg(20, 20, '<rect width="20" height="20" fill="#000000"/>')]))
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        <div style={{ width: 200, height: 200, backgroundColor: "#ffffff" }}>
          <div
            style={{
              width: 100,
              height: 100,
              backgroundColor: "#ff0000",
              maskImage: OPAQUE,
              hover: { mask: `url("${address}") no-repeat` },
            }}
          />
        </div>
      )
      await new Promise((resolve) => setTimeout(resolve, 200))
      testRoot.renderer.nativeSimulateMouseMove(50, 50)
      const at = await until(testRoot, (sample) => isNear(sample(10, 10), RED) && isNear(sample(50, 50), WHITE))
      near(at(10, 10), RED)
      near(at(50, 50), WHITE)
    } finally {
      testRoot.unmount()
      URL.revokeObjectURL(address)
    }
  })
})

describeNative("mask image glue", () => {
  /** The box carries the red fill itself, so the hover lands on it. */
  const hoverable = (style: Record<string, unknown>) => (
    <div style={{ position: "relative", width: 200, height: 200, backgroundColor: "#ffffff" }}>
      <div
        style={{
          position: "absolute",
          left: 50,
          top: 0,
          width: 100,
          height: 200,
          backgroundColor: "#ff0000",
          ...style,
        }}
      />
    </div>
  )

  it("swaps the image mask on hover, on the first hovered frame", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        hoverable({
          mask: `${BLACK} no-repeat 0 0 / 100px 100px`,
          hover: { mask: `${url(solid(30, 30, "#000000"))} no-repeat 0 100px / 100px 100px` },
        })
      )
      testRoot.renderer.nativeSimulateMouseMove(5, 100)
      let at = capture(testRoot).at
      near(at(100, 50), RED)
      near(at(100, 150), WHITE)
      testRoot.renderer.nativeSimulateMouseMove(100, 100)
      at = capture(testRoot).at
      near(at(100, 50), WHITE)
      near(at(100, 150), RED)
      testRoot.renderer.nativeSimulateMouseMove(5, 100)
      at = capture(testRoot).at
      near(at(100, 50), RED)
      near(at(100, 150), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("lets a hover maskMode change how the base image is read", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(hoverable({ maskImage: BLACK, hover: { maskMode: "luminance" } }))
      testRoot.renderer.nativeSimulateMouseMove(5, 100)
      near(capture(testRoot).at(100, 100), RED)
      testRoot.renderer.nativeSimulateMouseMove(100, 100)
      near(capture(testRoot).at(100, 100), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("lets a hover mask shorthand clear the base mode and composite", () => {
    const testRoot = createTestRoot()
    try {
      testRoot.render(
        hoverable({
          maskImage: BLACK,
          maskMode: "luminance",
          maskComposite: "subtract",
          hover: { mask: BLACK },
        })
      )
      testRoot.renderer.nativeSimulateMouseMove(5, 100)
      near(capture(testRoot).at(100, 100), WHITE)
      testRoot.renderer.nativeSimulateMouseMove(100, 100)
      near(capture(testRoot).at(100, 100), RED)
    } finally {
      testRoot.unmount()
    }
  })

  it("repaints when only the mode or the composite changes", () => {
    const testRoot = createTestRoot()
    try {
      const layers = {
        maskImage: `${BLACK}, ${OPAQUE}`,
        maskSize: "100px 200px",
        maskPosition: "0 0",
        maskRepeat: "no-repeat",
      }
      testRoot.render(masked({ ...layers, maskMode: "alpha" }))
      near(capture(testRoot).at(100, 100), RED)
      testRoot.render(masked({ ...layers, maskMode: "luminance, alpha" }))
      near(capture(testRoot).at(100, 100), RED)
      testRoot.render(masked({ ...layers, maskMode: "luminance" }))
      near(capture(testRoot).at(100, 100), WHITE)
      testRoot.render(masked({ ...layers, maskMode: "luminance", maskComposite: "intersect" }))
      near(capture(testRoot).at(100, 100), WHITE)
      testRoot.render(masked({ ...layers, maskMode: "alpha", maskComposite: "intersect" }))
      near(capture(testRoot).at(100, 100), RED)
      testRoot.render(masked({ ...layers, maskMode: "alpha", maskComposite: "subtract" }))
      near(capture(testRoot).at(100, 100), WHITE)
    } finally {
      testRoot.unmount()
    }
  })

  it("jumps to the new mask under a transition instead of animating it", () => {
    const testRoot = createTestRoot()
    try {
      const transition = "all 10s linear"
      testRoot.render(masked({ transition, maskImage: OPAQUE }))
      near(capture(testRoot).at(100, 100), RED)
      testRoot.render(masked({ transition, maskImage: "linear-gradient(transparent, transparent)" }))
      near(capture(testRoot).at(100, 100), WHITE)
      testRoot.render(masked({ transition, maskImage: BLACK }))
      near(capture(testRoot).at(100, 100), RED)
    } finally {
      testRoot.unmount()
    }
  })

  it("still clips the masked element to its rounded corners", () => {
    const { at } = shoot(
      masked({
        borderRadius: 40,
        backgroundColor: "#ff0000",
        maskImage: url(solid(20, 20, "#000000")),
        maskSize: "100px 200px",
      })
    )
    near(at(100, 100), RED)
    near(at(100, 3), RED)
    near(at(52, 2), WHITE)
    near(at(148, 197), WHITE)
    near(at(52, 197), WHITE)
    near(at(148, 2), WHITE)
  })

  it("leaves the element alone for an image-set(), none, or an invalid list", () => {
    const plain = shoot(masked({})).file
    expectScreenshotsEqual(plain, shoot(masked({ maskImage: "image-set(url(a.png) 1x)" })).file)
    expectScreenshotsEqual(plain, shoot(masked({ maskImage: "none" })).file)
    expectScreenshotsEqual(plain, shoot(masked({ maskImage: "url(a.png" })).file)
    expectScreenshotsEqual(plain, shoot(masked({ mask: "url(a.png) url(b.png)" })).file)
  })

  it("lets a gradient mask above an image take the box where the image is empty", () => {
    const { at } = shoot(
      masked({
        maskImage: `linear-gradient(to right, #000 0 50%, transparent 50%), ${url(LEFT_HALF)}`,
        maskSize: "100px 200px, 20px 20px",
        maskRepeat: "no-repeat, repeat",
      })
    )
    near(at(75, 100), RED)
    near(at(115, 100), RED)
    near(at(125, 100), WHITE)
  })
})
