/**
 * The text `getByText` and `findByText` compare: a string with its white space
 * collapsed and trimmed, which is what the DOM paints for a `white-space:
 * normal` string and what Testing Library's default normalizer matches. A
 * query is normalised the same way, so `getByText("a b")` finds `"a\n  b"`.
 */
export function matchableText(text: string): string {
  return text.replace(/[ \t\n\r\f]+/g, " ").trim()
}
