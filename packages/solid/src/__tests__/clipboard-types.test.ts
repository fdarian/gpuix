import { afterAll, describe, expect, it } from "bun:test"
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs"
import { join, resolve } from "node:path"

// Inside the package so `@ghia/solid` self-resolves to the built dist and
// `@ghia/native` and `solid-js` resolve through its node_modules.
const root = resolve(import.meta.dir, "../..")
mkdirSync(join(root, "tmp"), { recursive: true })
const temp = mkdtempSync(join(root, "tmp/clipboard-types-"))
afterAll(() => rmSync(temp, { recursive: true, force: true }))

const tscTimeout = 60_000
const tsc = join(root, "node_modules/typescript/bin/tsc")

const source = `import { clipboard } from "@ghia/solid"

export async function roundTrip(): Promise<string> {
  await navigator.clipboard.writeText("x")
  const text: string = await navigator.clipboard.readText()
  return text + (await clipboard.readText())
}
`

// Without the DOM lib, solid-js and the rest of @ghia/native's declarations
// reference DOM types that do not exist, so that consumer needs skipLibCheck.
// With it, errors are still reported in the fixture's own source, which is
// where a missing `navigator.clipboard` shows up.
function check(name: string, lib: string[], skipLibCheck: boolean) {
  const directory = join(temp, name)
  mkdirSync(directory)
  writeFileSync(join(directory, "main.ts"), source)
  writeFileSync(join(directory, "tsconfig.json"), JSON.stringify({
    compilerOptions: {
      target: "ES2022",
      module: "NodeNext",
      moduleResolution: "NodeNext",
      strict: true,
      noEmit: true,
      skipLibCheck,
      lib,
      types: ["bun", "node"],
    },
    files: ["main.ts"],
  }))
  return Bun.spawnSync({
    cmd: ["bun", tsc, "-p", directory],
    cwd: root,
    stdout: "pipe",
    stderr: "pipe",
  })
}

describe("navigator.clipboard types", () => {
  it("are declared for a consumer without the DOM lib", () => {
    const result = check("no-dom", ["ES2022"], true)
    expect(result.exitCode, result.stdout.toString()).toBe(0)
  }, tscTimeout)

  it("do not clash with lib.dom, checked without skipLibCheck", () => {
    const result = check("dom", ["ES2022", "DOM"], false)
    expect(result.exitCode, result.stdout.toString()).toBe(0)
  }, tscTimeout)
})
