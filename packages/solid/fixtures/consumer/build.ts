import { fileURLToPath } from "node:url"
import solidPlugin from "@ghia/solid/bun-plugin"

const result = await Bun.build({
  entrypoints: [fileURLToPath(new URL("./build-entry.tsx", import.meta.url))],
  target: "bun",
  plugins: [solidPlugin],
  write: false,
})

if (!result.success) throw new Error(result.logs.join("\n"))
const output = await result.outputs[0]!.text()
if (!output.includes("createElement")) throw new Error("Solid JSX was not transformed")
console.log("build plugin ok")
