/**
 * Frame cost of a page of clipped and scrolled-out content: the shape of the
 * Fluid Functionalism Command Menu demo (8 menus, about 1.4k elements). Every
 * menu is a card with a list scroller whose rows mostly lie outside the
 * scroller's clip, and the cards themselves mostly lie below the page's clip.
 *
 * Three variants build the same page:
 *   full        nothing hidden
 *   far-hidden  cards beyond a quarter viewport of the page clip get
 *               `display: none` and a pinned height (the demo's workaround)
 *   all-hidden  far-hidden, and list rows beyond their scroller's clip too
 *               (`display: none` on the row, a spacer keeps the scroll range)
 *
 * It profiles, it does not guard: no budgets. Run it through
 * `bun run test:perf:menus` (NODE_ENV=production, like `test:perf`). The frame
 * is `renderer.flush()`, which notifies the root view and draws inline, so it
 * covers the gpuix build, GPUI layout, prepaint and paint, but not GPU
 * submission (the test renderer has none).
 */

import React from 'react'
import { describe, expect, it } from 'vitest'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const MENUS = 8
const ROWS = 12
const ROW_HEIGHT = 36
const LIST_HEIGHT = 220
const WINDOW = { width: 1240, height: 780 }
const PAGE_HEADER = 60
// A row whose top is past this many rows of the list is outside its clip.
const VISIBLE_ROWS = Math.ceil(LIST_HEIGHT / ROW_HEIGHT)
const WARMUP = 10
const SAMPLES = 100

type Variant = 'full' | 'far-hidden' | 'all-hidden'

const C = {
  bg: '#0b0b0c',
  surface: '#161618',
  border: '#2a2a2e',
  fg: '#f2f2f3',
  muted: '#9a9aa1',
  hover: '#24242a',
}

const ICON =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="8"/><path d="m21 21-4.3-4.3"/></svg>'

function Cap({ label }: { label: string }) {
  return (
    <div
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        height: 20,
        minWidth: 20,
        paddingLeft: 4,
        paddingRight: 4,
        borderRadius: 5,
        backgroundColor: C.hover,
      }}
    >
      <text style={{ fontSize: 11, color: C.muted, whiteSpace: 'nowrap' }}>{label}</text>
    </div>
  )
}

function Row({ menu, index, hidden }: { menu: number; index: number; hidden: boolean }) {
  return (
    <div
      role="option"
      testId={`menu-${menu}-row-${index}`}
      style={{
        display: hidden ? 'none' : 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        flexShrink: 0,
        height: ROW_HEIGHT,
        gap: 10,
        paddingLeft: 10,
        paddingRight: 10,
        borderRadius: 8,
        cursor: 'pointer',
      }}
    >
      <svg
        source={ICON}
        style={{ width: 16, height: 16, flexShrink: 0, color: C.muted }}
      />
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          flexGrow: 1,
          minWidth: 0,
          gap: 8,
          overflow: 'hidden',
        }}
      >
        <text style={{ fontSize: 14, flexShrink: 0, color: C.fg, whiteSpace: 'nowrap' }}>
          {`Menu ${menu} action ${index}`}
        </text>
        <text
          style={{
            fontSize: 14,
            minWidth: 0,
            color: C.muted,
            whiteSpace: 'nowrap',
            textOverflow: 'ellipsis',
            overflow: 'hidden',
          }}
        >
          {`Description of action ${index}`}
        </text>
      </div>
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', flexShrink: 0, gap: 2 }}>
        <Cap label="⌘" />
        <Cap label={String.fromCharCode(65 + (index % 26))} />
      </div>
    </div>
  )
}

function Menu({ menu, variant }: { menu: number; variant: Variant }) {
  const hideRows = variant === 'all-hidden'
  const rows: React.ReactNode[] = []
  let hidden = 0
  for (let index = 0; index < ROWS; index++) {
    const outside = hideRows && index > VISIBLE_ROWS
    if (outside) hidden++
    rows.push(<Row key={index} menu={menu} index={index} hidden={outside} />)
  }
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div
        style={{
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          gap: 8,
          height: 44,
          paddingLeft: 10,
          paddingRight: 10,
          borderRadius: 8,
          backgroundColor: C.hover,
        }}
      >
        <svg source={ICON} style={{ width: 16, height: 16, color: C.muted }} />
        <text style={{ fontSize: 14, color: C.muted }}>Search…</text>
      </div>
      <div
        testId={`menu-${menu}-list`}
        role="listbox"
        style={{
          display: 'flex',
          flexDirection: 'column',
          height: LIST_HEIGHT,
          overflowY: 'auto',
        }}
      >
        {rows}
        {hidden > 0 ? <div style={{ height: hidden * ROW_HEIGHT, flexShrink: 0 }} /> : null}
      </div>
    </div>
  )
}

const CARD_CONTENT_HEIGHT = 20 * 2 + 44 + 8 + LIST_HEIGHT

function Card({ menu, variant, far }: { menu: number; variant: Variant; far: boolean }) {
  const hide = variant !== 'full' && far
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        borderRadius: 12,
        borderWidth: 1,
        borderColor: C.border,
        backgroundColor: C.surface,
        flexShrink: 0,
      }}
    >
      <div style={{ padding: 20, height: hide ? CARD_CONTENT_HEIGHT : undefined }}>
        <div style={{ display: hide ? 'none' : 'contents' }}>
          <Menu menu={menu} variant={variant} />
        </div>
      </div>
      <div
        style={{
          height: 44,
          display: 'flex',
          alignItems: 'center',
          paddingLeft: 16,
          borderTopWidth: 1,
          borderColor: C.border,
        }}
      >
        <text style={{ fontSize: 14, color: C.muted }}>{`Command menu ${menu}`}</text>
      </div>
    </div>
  )
}

function Page({ variant, tick }: { variant: Variant; tick: number }) {
  // The viewport shows the first cards; a card more than a quarter viewport
  // past the page clip counts as far, like the demo's IntersectionObserver.
  const cardHeight = CARD_CONTENT_HEIGHT + 44 + 2 + 16
  const clipBottom = WINDOW.height + WINDOW.height / 4
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: WINDOW.width,
        height: WINDOW.height,
        backgroundColor: C.bg,
      }}
    >
      <div style={{ height: PAGE_HEADER, display: 'flex', alignItems: 'center', paddingLeft: 20 }}>
        <text style={{ fontSize: 20, color: C.fg }}>{`Command Menu ${tick}`}</text>
      </div>
      <div
        testId="page"
        style={{
          display: 'flex',
          flexDirection: 'column',
          flexGrow: 1,
          minHeight: 0,
          overflowY: 'auto',
          padding: 20,
          gap: 16,
        }}
      >
        {Array.from({ length: MENUS }, (_, menu) => (
          <Card
            key={menu}
            menu={menu}
            variant={variant}
            far={PAGE_HEADER + 20 + menu * (cardHeight + 16) > clipBottom}
          />
        ))}
      </div>
    </div>
  )
}

function p(samples: number[], q: number) {
  const sorted = [...samples].sort((a, b) => a - b)
  return sorted[Math.min(sorted.length - 1, Math.ceil(q * sorted.length) - 1)]!
}

interface Sample {
  wallMs: number
  cpuMs: number
}

// CPU time next to wall time: the machine this runs on is often shared, and a
// frame that waited for a core is not a slower frame.
function sample(run: (index: number) => void): Sample[] {
  const out: Sample[] = []
  for (let i = 0; i < WARMUP + SAMPLES; i++) {
    const cpu = process.cpuUsage()
    const start = performance.now()
    run(i)
    const wallMs = performance.now() - start
    const used = process.cpuUsage(cpu)
    if (i >= WARMUP) out.push({ wallMs, cpuMs: (used.user + used.system) / 1000 })
  }
  return out
}

function report(variant: Variant, label: string, samples: Sample[], elements: number) {
  const wall = samples.map((s) => s.wallMs)
  const cpu = samples.map((s) => s.cpuMs)
  console.log(
    `[menus.perf] ${variant} ${label} elements=${elements} n=${samples.length} ` +
      `cpu p50=${p(cpu, 0.5).toFixed(2)}ms min=${Math.min(...cpu).toFixed(2)}ms | ` +
      `wall p50=${p(wall, 0.5).toFixed(2)}ms min=${Math.min(...wall).toFixed(2)}ms`,
  )
}

describeNative('command menu page: clipped and scrolled-out content', () => {
  for (const variant of ['full', 'far-hidden', 'all-hidden'] as const) {
    it(`${variant}: frame and page wheel`, () => {
      const root = createTestRoot(WINDOW)
      root.render(<Page variant={variant} tick={0} />)
      const renderer = root.renderer
      const elements = renderer.getRetainedElementCount()

      // A frame with nothing changed: the whole root view is rebuilt and laid out.
      report(variant, 'idle-flush', sample(() => renderer.flush()), elements)

      // Wheel on the page scroller: the same rebuild plus a scroll offset change.
      const wheel = sample((i) =>
        renderer.dispatchScrollWheel(WINDOW.width / 2, 400, 0, i % 2 === 0 ? 40 : -40),
      )
      report(variant, 'page-wheel', wheel, elements)

      // A wheel over a menu's list scroller.
      const listWheel = sample((i) =>
        renderer.dispatchScrollWheel(WINDOW.width / 2, 200, 0, i % 2 === 0 ? 40 : -40),
      )
      report(variant, 'list-wheel', listWheel, elements)
      expect(elements).toBeGreaterThan(0)
    }, 120_000)
  }
})
