// Type-only check, compiled by tsconfig.test-support.json against the test-support index.d.ts.
// Fails the build when a counter interface in js/testing.ts drifts from the generated one.
import type * as Generated from "../../index.js"
import type * as Hand from "../testing.js"

type Equal<A, B> = [A] extends [B] ? ([B] extends [A] ? true : false) : false
type Assert<T extends true> = T

type BuildCountersMatch = Assert<Equal<Hand.BuildCounters, Generated.BuildCounters>>
type FrameWorkCountersMatch = Assert<Equal<Hand.FrameWorkCounters, Generated.FrameWorkCounters>>
type LayoutCountersMatch = Assert<Equal<Hand.LayoutCounters, Generated.LayoutCounters>>
type RetainedCountersMatch = Assert<Equal<Hand.RetainedCounters, Generated.RetainedCounters>>
