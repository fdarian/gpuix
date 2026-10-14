import { describe, expect, it } from "vitest"
import { parseMediaQueryList } from "../media-query.js"
import type { MediaEnvironment } from "../media-query.js"

function environment(overrides: Partial<MediaEnvironment> = {}): MediaEnvironment {
  return {
    width: 800,
    height: 600,
    colorScheme: "light",
    reducedMotion: false,
    increasedContrast: false,
    forcedColors: false,
    ...overrides,
  }
}

function matches(query: string, overrides: Partial<MediaEnvironment> = {}): boolean {
  return parseMediaQueryList(query).matches(environment(overrides))
}

describe("media query parsing and serialization", () => {
  it("serializes in canonical form", () => {
    const cases: Array<[string, string]> = [
      ["(MIN-WIDTH:400PX)", "(min-width: 400px)"],
      ["  screen   and (max-width : 100px) ", "screen and (max-width: 100px)"],
      ["only screen", "only screen"],
      ["NOT print", "not print"],
      ["(width)", "(width)"],
      ["(400px<=width<800px)", "(400px <= width < 800px)"],
      ["(width>=400px)", "(width >= 400px)"],
      ["(16/9 < aspect-ratio)", "(16 / 9 < aspect-ratio)"],
      ["(a) , screen", "not all, screen"],
      ["screen and (color) and (monochrome: 0)", "screen and (color) and (monochrome: 0)"],
      ["(color) or (monochrome)", "(color) or (monochrome)"],
      ["not ((color) and (hover))", "not ((color) and (hover))"],
      ["screen and not (hover)", "screen and not (hover)"],
    ]
    for (const [query, media] of cases) {
      expect(parseMediaQueryList(query).media, query).toBe(media)
    }
  })

  it("keeps the empty list empty and matching", () => {
    const list = parseMediaQueryList("")
    expect(list.media).toBe("")
    expect(list.matches(environment())).toBe(true)
  })

  it("turns an unknown feature or an invalid value into not all", () => {
    for (const query of [
      "(unknown-feature)",
      "(unknown: 1)",
      "(prefers-color-scheme: blue)",
      "(min-hover: hover)",
      "(width: 400)",
      "(width: 4furlongs)",
      "(width >= 100px) and (foo)",
      "(prefers-color-scheme < dark)",
      "(100px < width > 50px)",
      "(color: 1.5)",
      "(constructor)",
      "(__proto__: 1)",
      "(min-toString: 1)",
      "(hasOwnProperty)",
    ]) {
      const list = parseMediaQueryList(query)
      expect(list.media, query).toBe("not all")
      expect(list.matches(environment()), query).toBe(false)
    }
  })

  it("turns a syntax error into not all", () => {
    for (const query of [
      "(",
      "screen and",
      "screen (color)",
      "(color) and",
      "(color) and (hover) or (width)",
      "and (color)",
      "screen, ,print",
      "(color) # x",
      "not (color) and (hover)",
      "screen and(color)",
    ]) {
      expect(parseMediaQueryList(query).media.split(", ").includes("not all"), query).toBe(true)
    }
  })

  it("keeps the other items of a comma list when one is invalid", () => {
    const list = parseMediaQueryList("(bogus), (min-width: 100px)")
    expect(list.media).toBe("not all, (min-width: 100px)")
    expect(list.matches(environment())).toBe(true)
  })
})

describe("media query evaluation", () => {
  it("matches media types", () => {
    expect(matches("all")).toBe(true)
    expect(matches("screen")).toBe(true)
    expect(matches("print")).toBe(false)
    expect(matches("speech")).toBe(false)
    expect(matches("tv")).toBe(false)
    expect(matches("not print")).toBe(true)
    expect(matches("not screen")).toBe(false)
    expect(matches("only screen and (min-width: 100px)")).toBe(true)
  })

  it("compares width and height with min, max and ranges", () => {
    expect(matches("(width: 800px)")).toBe(true)
    expect(matches("(min-width: 800px)")).toBe(true)
    expect(matches("(min-width: 801px)")).toBe(false)
    expect(matches("(max-width: 799px)")).toBe(false)
    expect(matches("(max-height: 600px)")).toBe(true)
    expect(matches("(width >= 800px)")).toBe(true)
    expect(matches("(width > 800px)")).toBe(false)
    expect(matches("(400px <= width < 801px)")).toBe(true)
    expect(matches("(400px <= width < 800px)")).toBe(false)
    expect(matches("(900px > width)")).toBe(true)
    expect(matches("(height = 600px)")).toBe(true)
    expect(matches("(width)")).toBe(true)
    expect(matches("(width)", { width: 0 })).toBe(false)
  })

  it("resolves em, rem and viewport units", () => {
    expect(matches("(min-width: 50em)")).toBe(true)
    expect(matches("(min-width: 50.1rem)")).toBe(false)
    expect(matches("(width: 100vw)")).toBe(true)
    expect(matches("(height: 100vh)")).toBe(true)
    expect(matches("(min-width: 1in)")).toBe(true)
    expect(matches("(min-width: 0)")).toBe(true)
  })

  it("evaluates aspect-ratio and orientation", () => {
    expect(matches("(aspect-ratio: 4/3)")).toBe(true)
    expect(matches("(min-aspect-ratio: 16/9)")).toBe(false)
    expect(matches("(max-aspect-ratio: 2)")).toBe(true)
    expect(matches("(orientation: landscape)")).toBe(true)
    expect(matches("(orientation: portrait)")).toBe(false)
    expect(matches("(orientation: portrait)", { width: 300, height: 400 })).toBe(true)
    // A square window is portrait, as in browsers.
    expect(matches("(orientation: portrait)", { width: 300, height: 300 })).toBe(true)
  })

  it("evaluates the preference features", () => {
    expect(matches("(prefers-color-scheme: light)")).toBe(true)
    expect(matches("(prefers-color-scheme: dark)")).toBe(false)
    expect(matches("(prefers-color-scheme: dark)", { colorScheme: "dark" })).toBe(true)
    expect(matches("(prefers-color-scheme)")).toBe(true)

    expect(matches("(prefers-reduced-motion: reduce)")).toBe(false)
    expect(matches("(prefers-reduced-motion: no-preference)")).toBe(true)
    expect(matches("(prefers-reduced-motion: reduce)", { reducedMotion: true })).toBe(true)
    expect(matches("(prefers-reduced-motion)", { reducedMotion: true })).toBe(true)
    expect(matches("(prefers-reduced-motion)")).toBe(false)

    expect(matches("(prefers-contrast: more)")).toBe(false)
    expect(matches("(prefers-contrast: more)", { increasedContrast: true })).toBe(true)
    expect(matches("(prefers-contrast: no-preference)")).toBe(true)
    expect(matches("(forced-colors: none)")).toBe(true)
    expect(matches("(forced-colors: active)", { forcedColors: true })).toBe(true)
  })

  it("reports a hover-capable fine pointer", () => {
    expect(matches("(hover: hover)")).toBe(true)
    expect(matches("(any-hover: hover)")).toBe(true)
    expect(matches("(hover: none)")).toBe(false)
    expect(matches("(pointer: fine)")).toBe(true)
    expect(matches("(any-pointer: fine)")).toBe(true)
    expect(matches("(pointer: coarse)")).toBe(false)
    expect(matches("(pointer)")).toBe(true)
    expect(matches("(color)")).toBe(true)
    expect(matches("(monochrome)")).toBe(false)
  })

  it("combines with and, or and not", () => {
    expect(matches("(min-width: 100px) and (max-width: 900px)")).toBe(true)
    expect(matches("(min-width: 100px) and (max-width: 500px)")).toBe(false)
    expect(matches("(max-width: 100px) or (min-width: 700px)")).toBe(true)
    expect(matches("not (max-width: 100px)")).toBe(true)
    expect(matches("not ((min-width: 100px) and (max-width: 900px))")).toBe(false)
    expect(matches("((min-width: 100px) and (max-width: 500px)) or (hover)")).toBe(true)
    expect(matches("screen and (prefers-color-scheme: dark)", { colorScheme: "dark" })).toBe(true)
    expect(matches("(max-width: 100px), (prefers-color-scheme: dark)")).toBe(false)
    expect(matches("(max-width: 100px), (prefers-color-scheme: dark)", { colorScheme: "dark" })).toBe(
      true
    )
  })

  it("reports whether a result can depend on the window size", () => {
    expect(parseMediaQueryList("(min-width: 100px)").usesSize).toBe(true)
    expect(parseMediaQueryList("(orientation: landscape)").usesSize).toBe(true)
    expect(parseMediaQueryList("(hover), not (max-height: 5px)").usesSize).toBe(true)
    expect(parseMediaQueryList("(prefers-reduced-motion: reduce)").usesSize).toBe(false)
    expect(parseMediaQueryList("screen and (prefers-color-scheme: dark)").usesSize).toBe(false)
  })

  it("reads the environment lazily", () => {
    let reads = 0
    const env = {
      ...environment(),
      get width() {
        reads += 1
        return 800
      },
    }
    expect(parseMediaQueryList("(prefers-reduced-motion: reduce)").matches(env)).toBe(false)
    expect(reads).toBe(0)
    expect(parseMediaQueryList("(min-width: 1px)").matches(env)).toBe(true)
    expect(reads).toBe(1)
  })
})
