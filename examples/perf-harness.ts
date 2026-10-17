/**
 * Shared sampling for the `*.perf.test.tsx` profiling benchmarks of the
 * perf pass (#36 to #39). They print numbers and assert nothing.
 *
 * CPU time is reported next to wall time: the machine is often shared, and a
 * frame that waited for a core is not a slower frame.
 */

export interface Sample {
  wallMs: number
  cpuMs: number
}

export function percentile(values: number[], q: number): number {
  const sorted = [...values].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)]!
}

export function sample(run: (index: number) => void, warmup: number, samples: number): Sample[] {
  const out: Sample[] = []
  for (let i = 0; i < warmup + samples; i++) {
    const cpu = process.cpuUsage()
    const start = performance.now()
    run(i)
    const wallMs = performance.now() - start
    const used = process.cpuUsage(cpu)
    if (i >= warmup) out.push({ wallMs, cpuMs: (used.user + used.system) / 1000 })
  }
  return out
}

export interface Summary {
  cpuP50: number
  cpuMin: number
  wallP50: number
  wallMin: number
}

export function summarize(samples: Sample[]): Summary {
  const cpu = samples.map((s) => s.cpuMs)
  const wall = samples.map((s) => s.wallMs)
  return {
    cpuP50: percentile(cpu, 0.5),
    cpuMin: Math.min(...cpu),
    wallP50: percentile(wall, 0.5),
    wallMin: Math.min(...wall),
  }
}

export function formatSummary(summary: Summary): string {
  return (
    `cpu p50=${summary.cpuP50.toFixed(2)}ms min=${summary.cpuMin.toFixed(2)}ms | ` +
    `wall p50=${summary.wallP50.toFixed(2)}ms min=${summary.wallMin.toFixed(2)}ms`
  )
}

/** The part of `getBuildCounters()` that says how much of the build a retained replay could skip (#37 step 4a). */
export interface RetainCounters {
  elementsBuilt: number
  retainableRoots: number
  retainableElements: number
  retainableElementsLarge: number
  retainMismatches: number
  retainReasons: string
}

/**
 * Share of the built elements that are in an unchanged, retainable subtree, and the reasons the
 * rest are not, largest first. Every built element is counted under exactly one reason.
 */
export function formatRetain(counters: RetainCounters, top = 6): string {
  const percent = (count: number) => ((100 * count) / counters.elementsBuilt).toFixed(1)
  const reasons = Object.entries(JSON.parse(counters.retainReasons) as Record<string, number>)
    .filter(([reason]) => reason !== 'retained')
    .sort((left, right) => right[1] - left[1])
    .slice(0, top)
    .map(([reason, count]) => `${reason}=${percent(count)}%`)
  return (
    `retainable=${percent(counters.retainableElements)}% roots=${counters.retainableRoots} ` +
    `in-large-roots=${percent(counters.retainableElementsLarge)}% not-because: ${reasons.join(' ')}`
  )
}

/** Every built element is counted once, as retainable or under one reason. */
export function countedReasons(counters: RetainCounters): number {
  return Object.values(JSON.parse(counters.retainReasons) as Record<string, number>).reduce((sum, count) => sum + count, 0)
}
