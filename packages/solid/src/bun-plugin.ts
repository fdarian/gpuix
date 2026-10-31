// Compiler setup follows @opentui/solid (MIT, anomalyco/opentui).
import { transformAsync } from "@babel/core"
import typescript from "@babel/preset-typescript"
import solid from "babel-preset-solid"
import { plugin as registerBunPlugin, type BunPlugin } from "bun"
import { readFile } from "node:fs/promises"

const PLUGIN_STATE = Symbol.for("@ghia/solid/bun-plugin")

function stripQuery(path: string): string {
  return path.split(/[?#]/, 1)[0]!
}

async function transformSource(code: string, filename: string): Promise<string> {
  const presets: NonNullable<Parameters<typeof transformAsync>[1]>["presets"] = []
  if (/\.[cm]?[jt]sx$/.test(filename)) {
    presets.push([solid, { generate: "universal", moduleName: "@ghia/solid" }])
  }
  if (/\.[cm]?tsx?$/.test(filename)) presets.push([typescript])
  const result = await transformAsync(code, {
    filename,
    configFile: false,
    babelrc: false,
    presets,
  })
  return result?.code ?? code
}

export function createSolidPlugin(): BunPlugin {
  return {
    name: "gpuix-solid",
    setup(build) {
      build.onLoad(
        { filter: /[/\\]node_modules[/\\]solid-js[/\\]dist[/\\]server\.js(?:[?#].*)?$/ },
        async ({ path }) => ({
          contents: await readFile(stripQuery(path).replace("server.js", "solid.js"), "utf8"),
          loader: "js",
        })
      )
      build.onLoad(
        { filter: /[/\\]node_modules[/\\]solid-js[/\\]store[/\\]dist[/\\]server\.js(?:[?#].*)?$/ },
        async ({ path }) => ({
          contents: await readFile(stripQuery(path).replace("server.js", "store.js"), "utf8"),
          loader: "js",
        })
      )
      build.onLoad(
        { filter: /^(?!.*[/\\]node_modules[/\\]).*\.[cm]?[jt]sx(?:[?#].*)?$/ },
        async ({ path }) => ({
          contents: await transformSource(await readFile(stripQuery(path), "utf8"), stripQuery(path)),
          loader: "js",
        })
      )
    },
  }
}

export function ensureSolidPlugin(): boolean {
  const state = globalThis as typeof globalThis & { [PLUGIN_STATE]?: boolean }
  if (state[PLUGIN_STATE]) return false
  void registerBunPlugin(createSolidPlugin())
  state[PLUGIN_STATE] = true
  return true
}

const solidPlugin = createSolidPlugin()
export default solidPlugin
