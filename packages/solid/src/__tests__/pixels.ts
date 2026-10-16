import fs from "fs"
import zlib from "zlib"
import type { TestRenderer } from "../testing.js"

export interface Pixels {
  /** The pixel at the logical position (x, y) as `[r, g, b, a]`. */
  at(x: number, y: number): [number, number, number, number]
}

/** Decodes a screenshot (8-bit RGBA, not interlaced, which is what the test renderer writes), addressed in logical pixels. */
export function readLogicalPixels(file: string, renderer: TestRenderer): Pixels {
  const png = fs.readFileSync(file)
  const width = png.readUInt32BE(16)
  const height = png.readUInt32BE(20)
  if (png[24] !== 8 || png[25] !== 6 || png[28] !== 0) throw new Error("a screenshot is 8-bit RGBA and not interlaced")
  const chunks: Buffer[] = []
  for (let at = 8; at < png.length; ) {
    const length = png.readUInt32BE(at)
    if (png.toString("latin1", at + 4, at + 8) === "IDAT") chunks.push(png.subarray(at + 8, at + 8 + length))
    at += 12 + length
  }
  const raw = zlib.inflateSync(Buffer.concat(chunks))
  const stride = width * 4
  const data = Buffer.alloc(stride * height)
  for (let y = 0; y < height; y++) {
    const filter = raw[y * (stride + 1)]
    for (let x = 0; x < stride; x++) {
      const byte = raw[y * (stride + 1) + 1 + x]!
      const left = x >= 4 ? data[y * stride + x - 4]! : 0
      const up = y > 0 ? data[(y - 1) * stride + x]! : 0
      const upLeft = x >= 4 && y > 0 ? data[(y - 1) * stride + x - 4]! : 0
      const paeth = () => {
        const estimate = left + up - upLeft
        const distances = [Math.abs(estimate - left), Math.abs(estimate - up), Math.abs(estimate - upLeft)]
        return distances[0]! <= distances[1]! && distances[0]! <= distances[2]! ? left : distances[1]! <= distances[2]! ? up : upLeft
      }
      const predicted =
        filter === 0 ? 0 : filter === 1 ? left : filter === 2 ? up : filter === 3 ? (left + up) >> 1 : paeth()
      data[y * stride + x] = (byte + predicted) & 255
    }
  }
  const scale = width / renderer.getWindowSize().width
  return {
    at: (x, y) => {
      const i = Math.floor((y + 0.5) * scale) * stride + Math.floor((x + 0.5) * scale) * 4
      return [data[i]!, data[i + 1]!, data[i + 2]!, data[i + 3]!]
    },
  }
}
