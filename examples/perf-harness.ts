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
