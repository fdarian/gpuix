import { describe, expect, it } from "vitest"
import { sameMutationValue } from "../mutations.js"

describe("sameMutationValue", () => {
  it("ignores undefined keys and compares nested values", () => {
    expect(sameMutationValue({ a: 1, b: undefined }, { a: 1 })).toBe(true)
    expect(sameMutationValue({ hover: { a: 1 } }, { hover: { a: 2 } })).toBe(false)
  })

  it("sees keys written in another order as another value: the later of a shorthand and a longhand wins", () => {
    expect(sameMutationValue({ padding: 8, paddingLeft: 4 }, { paddingLeft: 4, padding: 8 })).toBe(false)
    expect(sameMutationValue({ hover: { padding: 8, paddingLeft: 4 } }, { hover: { paddingLeft: 4, padding: 8 } })).toBe(
      false
    )
    expect(sameMutationValue({ a: undefined, padding: 8, paddingLeft: 4 }, { padding: 8, b: undefined, paddingLeft: 4 })).toBe(
      true
    )
  })
})
