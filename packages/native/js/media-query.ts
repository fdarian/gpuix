/// Media Queries Level 4: parser, serializer and evaluator.
///
/// Pure: it knows nothing about windows or renderers. `match-media.ts` feeds
/// it a `MediaEnvironment` and turns the result into `MediaQueryList`s.
///
/// A query list item that does not parse, or that names a feature or value the
/// engine does not know, becomes `not all`: it never matches and serializes as
/// `not all`, as in browsers.

export interface MediaEnvironment {
  /** Content size of the window in CSS pixels. */
  readonly width: number
  readonly height: number
  readonly colorScheme: "light" | "dark"
  readonly reducedMotion: boolean
  readonly increasedContrast: boolean
  readonly forcedColors: boolean
}

type Comparison = "<" | "<=" | ">" | ">=" | "="

type MediaValue =
  | { type: "number"; value: number; text: string }
  | { type: "length"; amount: number; unit: string; text: string }
  | { type: "ratio"; value: number; text: string }
  | { type: "ident"; value: string }

type MediaCondition =
  | { kind: "boolean"; feature: string }
  | { kind: "plain"; feature: string; prefix: "min" | "max" | undefined; value: MediaValue }
  | {
      kind: "range"
      feature: string
      /** `value op feature`. */
      left: { op: Comparison; value: MediaValue } | undefined
      /** `feature op value`. */
      right: { op: Comparison; value: MediaValue } | undefined
    }
  | { kind: "not"; child: MediaCondition }
  | { kind: "and"; children: MediaCondition[] }
  | { kind: "or"; children: MediaCondition[] }

interface MediaQueryItem {
  readonly text: string
  readonly usesSize: boolean
  matches(env: MediaEnvironment): boolean
}

export interface ParsedMediaQueryList {
  /** Canonical serialization, as `MediaQueryList.media` reports it. */
  readonly media: string
  /** True when a result can change as the window is resized. */
  readonly usesSize: boolean
  matches(env: MediaEnvironment): boolean
}

// ── Features ─────────────────────────────────────────────────────────

type FeatureType =
  | { kind: "length"; read(env: MediaEnvironment): number }
  | { kind: "ratio"; read(env: MediaEnvironment): number }
  | { kind: "integer"; read(env: MediaEnvironment): number }
  | {
      kind: "discrete"
      values: readonly string[]
      /** The value `(feature)` in boolean context treats as false. */
      falsy: string | undefined
      read(env: MediaEnvironment): string
    }

const SIZE_FEATURES = new Set(["width", "height", "aspect-ratio", "orientation"])

/** There is no touch input or hover-less pointer on the platforms GPUIX targets. */
const FEATURES: Record<string, FeatureType> = {
  width: { kind: "length", read: (env) => env.width },
  height: { kind: "length", read: (env) => env.height },
  "aspect-ratio": { kind: "ratio", read: (env) => env.width / env.height },
  color: { kind: "integer", read: () => 8 },
  monochrome: { kind: "integer", read: () => 0 },
  orientation: {
    kind: "discrete",
    values: ["portrait", "landscape"],
    falsy: undefined,
    read: (env) => (env.height >= env.width ? "portrait" : "landscape"),
  },
  "prefers-color-scheme": {
    kind: "discrete",
    values: ["light", "dark"],
    falsy: undefined,
    read: (env) => env.colorScheme,
  },
  "prefers-reduced-motion": {
    kind: "discrete",
    values: ["no-preference", "reduce"],
    falsy: "no-preference",
    read: (env) => (env.reducedMotion ? "reduce" : "no-preference"),
  },
  "prefers-contrast": {
    kind: "discrete",
    values: ["no-preference", "more", "less", "custom"],
    falsy: "no-preference",
    read: (env) => (env.increasedContrast ? "more" : "no-preference"),
  },
  "forced-colors": {
    kind: "discrete",
    values: ["none", "active"],
    falsy: "none",
    read: (env) => (env.forcedColors ? "active" : "none"),
  },
  hover: {
    kind: "discrete",
    values: ["none", "hover"],
    falsy: "none",
    read: () => "hover",
  },
  "any-hover": {
    kind: "discrete",
    values: ["none", "hover"],
    falsy: "none",
    read: () => "hover",
  },
  pointer: {
    kind: "discrete",
    values: ["none", "coarse", "fine"],
    falsy: "none",
    read: () => "fine",
  },
  "any-pointer": {
    kind: "discrete",
    values: ["none", "coarse", "fine"],
    falsy: "none",
    read: () => "fine",
  },
}

/** Media queries resolve `em` and `rem` against the initial font size. */
const INITIAL_FONT_SIZE = 16

const FIXED_LENGTH_UNITS: Record<string, number> = {
  px: 1,
  em: INITIAL_FONT_SIZE,
  rem: INITIAL_FONT_SIZE,
  pt: 96 / 72,
  pc: 16,
  in: 96,
  cm: 96 / 2.54,
  mm: 96 / 25.4,
  q: 96 / 101.6,
}

const VIEWPORT_LENGTH_UNITS = new Set(["vw", "vh", "vmin", "vmax"])

const NON_TYPE_IDENTS = new Set(["not", "and", "or", "only", "layer"])
const KNOWN_TYPES = new Map<string, boolean>([
  ["all", true],
  ["screen", true],
  ["print", false],
  ["speech", false],
])

// ── Tokenizer ────────────────────────────────────────────────────────

type Token =
  | { type: "ident"; value: string; spaceBefore: boolean }
  | { type: "number"; value: number; unit: string; text: string; spaceBefore: boolean }
  | { type: "(" | ")" | "," | ":" | "/" | Comparison; spaceBefore: boolean }
  | { type: "error"; spaceBefore: boolean }

const NUMBER = /^[+-]?(?:\d+\.?\d*|\.\d+)(?:e[+-]?\d+)?/i
const IDENT = /^-{0,2}[a-z_][a-z0-9_-]*/i

function tokenize(text: string): Token[] {
  const tokens: Token[] = []
  let index = 0
  let spaceBefore = false
  while (index < text.length) {
    const char = text[index]
    if (/\s/.test(char)) {
      spaceBefore = true
      index += 1
      continue
    }
    const rest = text.slice(index)
    const space = spaceBefore
    spaceBefore = false
    if (char === "(" || char === ")" || char === "," || char === ":" || char === "/") {
      tokens.push({ type: char, spaceBefore: space })
      index += 1
      continue
    }
    if (char === "<" || char === ">") {
      if (rest[1] === "=") {
        tokens.push({ type: `${char}=` as const, spaceBefore: space })
        index += 2
      } else {
        tokens.push({ type: char, spaceBefore: space })
        index += 1
      }
      continue
    }
    if (char === "=") {
      tokens.push({ type: "=", spaceBefore: space })
      index += 1
      continue
    }
    const number = NUMBER.exec(rest)
    if (number) {
      const unit = /^[a-z%]*/i.exec(rest.slice(number[0].length))?.[0] ?? ""
      tokens.push({
        type: "number",
        value: Number(number[0]),
        unit: unit.toLowerCase(),
        text: `${number[0]}${unit.toLowerCase()}`,
        spaceBefore: space,
      })
      index += number[0].length + unit.length
      continue
    }
    const ident = IDENT.exec(rest)
    if (ident) {
      tokens.push({ type: "ident", value: ident[0].toLowerCase(), spaceBefore: space })
      index += ident[0].length
      continue
    }
    tokens.push({ type: "error", spaceBefore: space })
    index += 1
  }
  return tokens
}

// ── Parser ───────────────────────────────────────────────────────────

/** A syntax error, or a feature or value that is not known. */
class Unsupported extends Error {}

function isComparison(token: Token | undefined): token is Token & { type: Comparison } {
  return (
    token !== undefined &&
    (token.type === "<" ||
      token.type === "<=" ||
      token.type === ">" ||
      token.type === ">=" ||
      token.type === "=")
  )
}

class Parser {
  private index = 0

  constructor(private readonly tokens: Token[]) {}

  parseItem(): { type: string | undefined; qualifier: string | undefined; condition: MediaCondition | undefined } {
    const first = this.peek()
    if (first?.type === "(" || (first?.type === "ident" && first.value === "not" && this.peekAt(1)?.type === "(")) {
      const condition = this.parseCondition(true)
      this.expectEnd()
      return { type: undefined, qualifier: undefined, condition }
    }
    let qualifier: string | undefined
    if (first?.type === "ident" && (first.value === "not" || first.value === "only")) {
      qualifier = first.value
      this.index += 1
    }
    const type = this.next()
    if (type?.type !== "ident" || NON_TYPE_IDENTS.has(type.value)) throw new Unsupported()
    let condition: MediaCondition | undefined
    const joiner = this.peek()
    if (joiner) {
      if (joiner.type !== "ident" || joiner.value !== "and") throw new Unsupported()
      this.index += 1
      condition = this.parseCondition(false)
    }
    this.expectEnd()
    return { type: type.value, qualifier, condition }
  }

  private parseCondition(allowOr: boolean): MediaCondition {
    const first = this.peek()
    if (first?.type === "ident" && first.value === "not") {
      this.index += 1
      return { kind: "not", child: this.parseInParens() }
    }
    const operands = [this.parseInParens()]
    const operator = this.peek()
    if (operator?.type !== "ident" || (operator.value !== "and" && operator.value !== "or")) {
      return operands[0]
    }
    if (operator.value === "or" && !allowOr) throw new Unsupported()
    while (this.peek()?.type === "ident") {
      const next = this.peek() as Token & { type: "ident" }
      if (next.value !== operator.value) throw new Unsupported()
      this.index += 1
      operands.push(this.parseInParens())
    }
    return { kind: operator.value, children: operands }
  }

  private parseInParens(): MediaCondition {
    const open = this.next()
    if (open?.type !== "(") throw new Unsupported()
    const first = this.peek()
    let result: MediaCondition
    if (first?.type === "(" || (first?.type === "ident" && first.value === "not" && this.peekAt(1)?.type === "(")) {
      result = this.parseCondition(true)
    } else {
      result = this.parseFeature()
    }
    if (this.next()?.type !== ")") throw new Unsupported()
    return result
  }

  private parseFeature(): MediaCondition {
    const first = this.next()
    if (first?.type === "ident") {
      const after = this.peek()
      if (after?.type === ")") return { kind: "boolean", feature: first.value }
      if (after?.type === ":") {
        this.index += 1
        const value = this.parseValue()
        return plainFeature(first.value, value)
      }
      if (isComparison(after)) {
        this.index += 1
        const value = this.parseValue()
        const right = { op: after.type, value }
        if (isComparison(this.peek())) throw new Unsupported()
        return { kind: "range", feature: first.value, left: undefined, right }
      }
      throw new Unsupported()
    }
    if (first?.type === "number") {
      this.index -= 1
      const leftValue = this.parseValue()
      const leftOp = this.next()
      if (!isComparison(leftOp)) throw new Unsupported()
      const name = this.next()
      if (name?.type !== "ident") throw new Unsupported()
      const left = { op: leftOp.type, value: leftValue }
      const after = this.peek()
      if (!isComparison(after)) {
        return { kind: "range", feature: name.value, left, right: undefined }
      }
      this.index += 1
      const right = { op: after.type, value: this.parseValue() }
      const sameDirection =
        (isLess(left.op) && isLess(right.op)) || (isGreater(left.op) && isGreater(right.op))
      if (!sameDirection) throw new Unsupported()
      return { kind: "range", feature: name.value, left, right }
    }
    throw new Unsupported()
  }

  private parseValue(): MediaValue {
    const token = this.next()
    if (token?.type === "ident") return { type: "ident", value: token.value }
    if (token?.type !== "number") throw new Unsupported()
    if (this.peek()?.type === "/") {
      this.index += 1
      const denominator = this.next()
      if (token.unit !== "" || denominator?.type !== "number" || denominator.unit !== "") {
        throw new Unsupported()
      }
      return {
        type: "ratio",
        value: token.value / denominator.value,
        text: `${token.text} / ${denominator.text}`,
      }
    }
    if (token.unit === "") return { type: "number", value: token.value, text: token.text }
    if (!(token.unit in FIXED_LENGTH_UNITS) && !VIEWPORT_LENGTH_UNITS.has(token.unit)) {
      throw new Unsupported()
    }
    return { type: "length", amount: token.value, unit: token.unit, text: token.text }
  }

  private expectEnd(): void {
    if (this.index < this.tokens.length) throw new Unsupported()
  }

  private peek(): Token | undefined {
    return this.tokens[this.index]
  }

  private peekAt(offset: number): Token | undefined {
    return this.tokens[this.index + offset]
  }

  private next(): Token | undefined {
    const token = this.tokens[this.index]
    this.index += 1
    return token
  }
}

function isLess(op: Comparison): boolean {
  return op === "<" || op === "<="
}

function isGreater(op: Comparison): boolean {
  return op === ">" || op === ">="
}

function plainFeature(name: string, value: MediaValue): MediaCondition {
  if (name.startsWith("min-")) return { kind: "plain", feature: name.slice(4), prefix: "min", value }
  if (name.startsWith("max-")) return { kind: "plain", feature: name.slice(4), prefix: "max", value }
  return { kind: "plain", feature: name, prefix: undefined, value }
}

// ── Validation and evaluation ────────────────────────────────────────

function featureOf(name: string): FeatureType {
  if (!Object.hasOwn(FEATURES, name)) throw new Unsupported()
  return FEATURES[name]
}

/** Throws `Unsupported` when the condition names an unknown feature or a value the feature cannot take. */
function validate(condition: MediaCondition): void {
  switch (condition.kind) {
    case "not":
      validate(condition.child)
      return
    case "and":
    case "or":
      for (const child of condition.children) validate(child)
      return
    case "boolean":
      featureOf(condition.feature)
      return
    case "plain": {
      const feature = featureOf(condition.feature)
      if (feature.kind === "discrete") {
        if (condition.prefix) throw new Unsupported()
        if (condition.value.type !== "ident" || !feature.values.includes(condition.value.value)) {
          throw new Unsupported()
        }
        return
      }
      checkRangeValue(feature, condition.value)
      return
    }
    case "range": {
      const feature = featureOf(condition.feature)
      if (feature.kind === "discrete") throw new Unsupported()
      if (condition.left) checkRangeValue(feature, condition.left.value)
      if (condition.right) checkRangeValue(feature, condition.right.value)
      return
    }
  }
}

function checkRangeValue(feature: FeatureType, value: MediaValue): void {
  if (feature.kind === "length") {
    const zero = value.type === "number" && value.value === 0
    if (value.type !== "length" && !zero) throw new Unsupported()
  } else if (feature.kind === "ratio") {
    if (value.type !== "ratio" && value.type !== "number") throw new Unsupported()
  } else if (feature.kind === "integer") {
    if (value.type !== "number" || !Number.isInteger(value.value)) throw new Unsupported()
  } else {
    throw new Unsupported()
  }
}

function numericValue(value: MediaValue, env: MediaEnvironment): number {
  switch (value.type) {
    case "number":
    case "ratio":
      return value.value
    case "length":
      return lengthToPx(value.amount, value.unit, env)
    case "ident":
      throw new Error("Unreachable: validated as numeric")
  }
}

function lengthToPx(amount: number, unit: string, env: MediaEnvironment): number {
  const fixed = FIXED_LENGTH_UNITS[unit]
  if (fixed !== undefined) return amount * fixed
  const edge =
    unit === "vw"
      ? env.width
      : unit === "vh"
        ? env.height
        : unit === "vmin"
          ? Math.min(env.width, env.height)
          : Math.max(env.width, env.height)
  return (amount * edge) / 100
}

function compare(left: number, op: Comparison, right: number): boolean {
  switch (op) {
    case "<":
      return left < right
    case "<=":
      return left <= right
    case ">":
      return left > right
    case ">=":
      return left >= right
    case "=":
      return left === right
  }
}

function flip(op: Comparison): Comparison {
  switch (op) {
    case "<":
      return ">"
    case "<=":
      return ">="
    case ">":
      return "<"
    case ">=":
      return "<="
    case "=":
      return "="
  }
}

function evaluate(condition: MediaCondition, env: MediaEnvironment): boolean {
  switch (condition.kind) {
    case "not":
      return !evaluate(condition.child, env)
    case "and":
      return condition.children.every((child) => evaluate(child, env))
    case "or":
      return condition.children.some((child) => evaluate(child, env))
    case "boolean": {
      const feature = featureOf(condition.feature)
      if (feature.kind === "discrete") return feature.read(env) !== feature.falsy
      return feature.read(env) !== 0
    }
    case "plain": {
      const feature = featureOf(condition.feature)
      if (feature.kind === "discrete") {
        if (condition.value.type !== "ident") throw new Error("Unreachable: validated as a keyword")
        return feature.read(env) === condition.value.value
      }
      const actual = feature.read(env)
      const wanted = numericValue(condition.value, env)
      if (condition.prefix === "min") return actual >= wanted
      if (condition.prefix === "max") return actual <= wanted
      return actual === wanted
    }
    case "range": {
      const feature = featureOf(condition.feature)
      if (feature.kind === "discrete") throw new Error("Unreachable: validated as numeric")
      const actual = feature.read(env)
      // `400px <= width` reads as `width >= 400px`.
      if (condition.left && !compare(actual, flip(condition.left.op), numericValue(condition.left.value, env))) {
        return false
      }
      if (condition.right && !compare(actual, condition.right.op, numericValue(condition.right.value, env))) {
        return false
      }
      return true
    }
  }
}

function usesSize(condition: MediaCondition): boolean {
  switch (condition.kind) {
    case "not":
      return usesSize(condition.child)
    case "and":
    case "or":
      return condition.children.some(usesSize)
    default:
      return SIZE_FEATURES.has(condition.feature)
  }
}

// ── Serializer ───────────────────────────────────────────────────────

function serializeValue(value: MediaValue): string {
  return value.type === "ident" ? value.value : value.text
}

/** `nested` wraps compound conditions, which have no parentheses of their own. */
function serializeCondition(condition: MediaCondition, nested: boolean): string {
  switch (condition.kind) {
    case "boolean":
      return `(${condition.feature})`
    case "plain": {
      const prefix = condition.prefix ? `${condition.prefix}-` : ""
      return `(${prefix}${condition.feature}: ${serializeValue(condition.value)})`
    }
    case "range": {
      const left = condition.left ? `${serializeValue(condition.left.value)} ${condition.left.op} ` : ""
      const right = condition.right ? ` ${condition.right.op} ${serializeValue(condition.right.value)}` : ""
      return `(${left}${condition.feature}${right})`
    }
    case "not": {
      const text = `not ${serializeCondition(condition.child, true)}`
      return nested ? `(${text})` : text
    }
    case "and":
    case "or": {
      const text = condition.children
        .map((child) => serializeCondition(child, true))
        .join(` ${condition.kind} `)
      return nested ? `(${text})` : text
    }
  }
}

// ── Public ───────────────────────────────────────────────────────────

const NEVER: MediaQueryItem = {
  text: "not all",
  usesSize: false,
  matches: () => false,
}

function compileItem(tokens: Token[]): MediaQueryItem {
  if (tokens.some((token) => token.type === "error")) return NEVER
  for (let i = 0; i + 1 < tokens.length; i += 1) {
    const token = tokens[i]
    const after = tokens[i + 1]
    // `and(` is a function token in CSS, not the keyword `and`.
    if (token.type === "ident" && after.type === "(" && !after.spaceBefore) {
      if (token.value === "and" || token.value === "or" || token.value === "not") return NEVER
    }
  }
  try {
    const parsed = new Parser(tokens).parseItem()
    if (parsed.condition) validate(parsed.condition)
    const typeMatches = parsed.type === undefined ? true : (KNOWN_TYPES.get(parsed.type) ?? false)
    const negate = parsed.qualifier === "not"
    const condition = parsed.condition
    const parts: string[] = []
    if (parsed.qualifier) parts.push(parsed.qualifier)
    if (parsed.type) parts.push(parsed.type)
    if (condition) {
      const text = serializeCondition(condition, false)
      parts.push(parsed.type ? `and ${text}` : text)
    }
    return {
      text: parts.join(" "),
      usesSize: condition ? usesSize(condition) : false,
      matches(env) {
        const result = typeMatches && (condition ? evaluate(condition, env) : true)
        return negate ? !result : result
      },
    }
  } catch (error) {
    if (error instanceof Unsupported) return NEVER
    throw error
  }
}

function splitTopLevel(tokens: Token[]): Token[][] {
  const items: Token[][] = [[]]
  let depth = 0
  for (const token of tokens) {
    if (token.type === "(") depth += 1
    if (token.type === ")") depth -= 1
    if (token.type === "," && depth === 0) items.push([])
    else items[items.length - 1].push(token)
  }
  return items
}

/**
 * Parse a media query list, as `window.matchMedia` does. An empty string is the
 * empty list, which matches. An item that is invalid or names an unknown
 * feature is `not all`; the other items of a comma list still count.
 */
export function parseMediaQueryList(text: string): ParsedMediaQueryList {
  const tokens = tokenize(text)
  const items = tokens.length === 0 ? [] : splitTopLevel(tokens).map(compileItem)
  return {
    media: items.map((item) => item.text).join(", "),
    usesSize: items.some((item) => item.usesSize),
    matches: (env) => items.length === 0 || items.some((item) => item.matches(env)),
  }
}
