import { screen as nativeScreen } from "@ghia/native/screen"
import { expect, it } from "vitest"
import { screen } from "../index.js"

it("exports the native screen object", () => {
  expect(screen).toBe(nativeScreen)
  expect(screen.getPrimaryDisplay().bounds).toMatchObject({ x: 0, y: 0 })
})
