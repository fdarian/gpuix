/**
 * How much of a page a retained replay skips (#37 steps 4a and 5b), per page and per kind of
 * frame. Each page is run twice. Judged (`setRetainPlanning(false)`): every frame builds every
 * element, and the share of them in a subtree nothing in changed and nothing on the exclusion list
 * is in is what a replay could skip, with the rest counted under the reason they are not.
 * Skipping: the frames leave unbuilt what they judge unchanged and gpui replays it; the share of
 * elements left unbuilt, what gpui's retained subtrees did and what a frame costs are printed.
 *
 * Frames:
 *   idle    nothing changes, the clock moves
 *   hover   the pointer visits a grid of points over the window, one frame at each
 *   type    for a page with a text input: focus it and type, one frame per key
 *   animate for a page with a node that moves: the node moves
 *
 * It prints and guards one thing: with `setRetainVerify`, no element the key judged unchanged is
 * built differently. Run it through `bun run test:perf:retain` (NODE_ENV=production).
 */

import React, { useState } from 'react'
import { describe, expect, it } from 'vitest'
import { motion } from '@gpuix/react'
import { createTestRoot, hasNativeTestRenderer, type TestRoot } from '@gpuix/react/testing'
import { countedReasons, formatRetain, formatRetained, retainedDelta, type RetainCounters } from './perf-harness'
import { C, CardGrid, LONG_TWEEN } from './perf-fixtures'
import { ChatApp } from './chat'
import { MailApp } from './mail'
import { TimelineApp } from './timeline'

const describeNative = hasNativeTestRenderer ? describe : describe.skip

const WINDOW = { width: 1240, height: 780 }
const FRAME_MS = 1000 / 75
const IDLE_FRAMES = 40
const GRID = { columns: 10, rows: 6 }
const KEYS = 12

function CardsPage({ moving }: { moving: boolean }) {
  return (
    <div style={{ position: 'relative', width: WINDOW.width, height: WINDOW.height, backgroundColor: C.bg, overflow: 'hidden' }}>
      <CardGrid cards={60} />
      {moving ? (
        <motion.div
          testId="highlight"
          style={{ position: 'absolute', left: 20, top: 20, width: 240, height: 32, borderRadius: 8, backgroundColor: '#ffffff', opacity: 0.1 }}
          initial={{ opacity: 0.05 }}
          animate={{ opacity: 0.9 }}
          transition={LONG_TWEEN}
        />
      ) : null}
    </div>
  )
}

const FIELDS = ['Name', 'Email', 'Company', 'Role', 'Phone', 'City', 'Country', 'Website']

function Field({ label, value, onChange }: { label: string; value: string; onChange: (value: string) => void }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 4, width: 280 }}>
      <label style={{ fontSize: 12, color: C.muted }}>{label}</label>
      <input
        value={value}
        placeholder={label}
        onChange={(event) => onChange(event.value ?? '')}
        style={{ height: 32, paddingLeft: 8, borderRadius: 6, borderWidth: 1, borderStyle: 'solid', borderColor: C.border, backgroundColor: C.surface, color: C.fg, fontSize: 13 }}
      />
    </div>
  )
}

function Choice({ label }: { label: string }) {
  return (
    <div
      tabIndex={0}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 8,
        height: 28,
        paddingLeft: 8,
        borderRadius: 6,
        hover: { backgroundColor: C.chip },
        focusVisible: { outlineWidth: 2, outlineColor: '#3b82f6' },
      }}
    >
      <div style={{ width: 14, height: 14, borderRadius: 3, borderWidth: 1, borderStyle: 'solid', borderColor: C.muted }} />
      <text style={{ fontSize: 13, color: C.fg }}>{label}</text>
    </div>
  )
}

function FormPage() {
  const [values, setValues] = useState<Record<string, string>>({})
  return (
    <div style={{ display: 'flex', flexDirection: 'column', width: WINDOW.width, height: WINDOW.height, padding: 24, gap: 20, backgroundColor: C.bg }}>
      <text style={{ fontSize: 20, color: C.fg }}>Account settings</text>
      <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', gap: 16 }}>
        {FIELDS.map((field) => (
          <Field key={field} label={field} value={values[field] ?? ''} onChange={(value) => setValues((all) => ({ ...all, [field]: value }))} />
        ))}
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, width: 280 }}>
        {['Product updates', 'Security alerts', 'Weekly digest', 'Billing', 'Mentions', 'Comments'].map((label) => (
          <Choice key={label} label={label} />
        ))}
      </div>
      <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
        {['Save', 'Cancel'].map((label) => (
          <div key={label} tabIndex={0} style={{ height: 32, paddingLeft: 14, paddingRight: 14, borderRadius: 6, backgroundColor: C.chip, justifyContent: 'center', hover: { backgroundColor: C.border } }}>
            <text style={{ fontSize: 13, color: C.fg }}>{label}</text>
          </div>
        ))}
      </div>
    </div>
  )
}

const ROWS = 30
const COLUMNS = ['Name', 'Status', 'Owner', 'Updated', 'Size', 'Region']

function TablePage() {
  const cell = { padding: 8, borderBottomWidth: 1, borderBottomStyle: 'solid', borderColor: C.border } as const
  return (
    <div style={{ width: WINDOW.width, height: WINDOW.height, backgroundColor: C.bg, padding: 24, overflow: 'hidden' }}>
      <div style={{ display: 'table', width: '100%', borderCollapse: 'separate' }}>
        <div style={{ display: 'table-header-group' }}>
          <div style={{ display: 'table-row' }}>
            {COLUMNS.map((column) => (
              <div key={column} style={{ ...cell, display: 'table-cell' }}>
                <text style={{ fontSize: 12, color: C.muted }}>{column}</text>
              </div>
            ))}
          </div>
        </div>
        <div style={{ display: 'table-row-group' }}>
          {Array.from({ length: ROWS }, (_, row) => (
            <div key={row} style={{ display: 'table-row', hover: { backgroundColor: C.surface } }}>
              {COLUMNS.map((column) => (
                <div key={column} style={{ ...cell, display: 'table-cell' }}>
                  <text style={{ fontSize: 13, color: C.fg }}>{`${column} ${row}`}</text>
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>
    </div>
  )
}

type Page = {
  name: string
  element: React.ReactElement
  size?: { width: number; height: number }
  /** A page with a text input to type into. */
  typeInto?: string
  /** The same page with something moving. */
  animated?: React.ReactElement
}

const PAGES: Page[] = [
  { name: 'cards', element: <CardsPage moving={false} />, animated: <CardsPage moving /> },
  { name: 'form', element: <FormPage />, typeInto: 'input' },
  { name: 'table', element: <TablePage /> },
  { name: 'chat', element: <ChatApp turnCount={120} />, typeInto: 'textarea' },
  { name: 'mail', element: <MailApp />, typeInto: 'input' },
  { name: 'timeline', element: <TimelineApp viewportWidth={1240} viewportHeight={780} /> },
]

function frames(root: TestRoot, name: string, run: () => number): RetainCounters {
  const { renderer } = root
  renderer.resetBuildCounters()
  const retainedBefore = renderer.getRetainedCounters()
  const cpuBefore = process.cpuUsage()
  const drawn = run()
  const cpu = process.cpuUsage(cpuBefore)
  const built = renderer.getBuildCounters()
  const cpuPerFrame = (cpu.user + cpu.system) / 1000 / drawn
  console.log(
    `[retain.perf] ${name} frames=${drawn} cpu/frame=${cpuPerFrame.toFixed(2)}ms built/frame=${(built.elementsBuilt / drawn).toFixed(0)} ` +
      `${formatRetain(built)} | ${formatRetained(retainedDelta(renderer.getRetainedCounters(), retainedBefore), drawn)}`,
  )
  expect(built.retainMismatches, renderer.getRetainMismatches().join('\n')).toBe(0)
  expect(renderer.getRetainedCounters().mismatches, renderer.getRetentionMismatches().join('\n')).toBe(0)
  expect(countedReasons(built)).toBe(built.elementsBuilt + built.elementsSkipped)
  return built
}

/** Exclusions lifted one step after another, to see what each is worth. */
const LIFTS: [string, string[]][] = [
  ['+custom-element', ['custom-element']],
  ['+scroller +display-contents +paint-reporter', ['scroller', 'display-contents', 'paint-reporter']],
]

describeNative('retainable share of a frame, by page', () => {
  for (const page of PAGES) {
    for (const skipping of [false, true]) {
    it(`${page.name} ${skipping ? 'skipping' : 'judged'}`, () => {
      const size = page.size ?? WINDOW
      const root = createTestRoot(size)
      try {
        root.render(page.element)
        const { renderer } = root
        renderer.clockPause()
        renderer.setRetainPlanning(skipping)
        // Judged frames hash every element they build and check what was judged unchanged.
        renderer.setRetainVerify(!skipping)
        // The first builds see every element for the first time.
        for (let i = 0; i < 3; i++) renderer.clockFastForward(FRAME_MS)
        console.log(`[retain.perf] ${page.name}: ${renderer.getRetainedElementCount()} retained elements`)

        const idle = () => {
          for (let i = 0; i < IDLE_FRAMES; i++) renderer.clockFastForward(FRAME_MS)
          return IDLE_FRAMES
        }
        frames(root, `${page.name} idle ${skipping ? 'skipping' : 'judged'}`, idle)

        frames(root, `${page.name} hover ${skipping ? 'skipping' : 'judged'}`, () => {
          let drawn = 0
          for (let row = 0; row < GRID.rows; row++) {
            for (let column = 0; column < GRID.columns; column++) {
              const x = ((column + 0.5) * size.width) / GRID.columns
              const y = ((row + 0.5) * size.height) / GRID.rows
              renderer.dispatchMouseMove(x, y)
              renderer.clockFastForward(FRAME_MS)
              drawn += 1
            }
          }
          return drawn
        })

        if (page.typeInto) {
          const input = renderer.findByType(page.typeInto)[0]
          if (input) {
            frames(root, `${page.name} type ${skipping ? 'skipping' : 'judged'}`, () => {
              renderer.focusElement(input.id)
              for (let i = 0; i < KEYS; i++) renderer.simulateKeystrokes('a')
              return KEYS
            })
          }
        }

        if (page.animated) {
          root.render(page.animated)
          renderer.clockPause()
          for (let i = 0; i < 3; i++) renderer.clockFastForward(FRAME_MS)
          frames(root, `${page.name} animate ${skipping ? 'skipping' : 'judged'}`, idle)
        }

        if (!skipping) {
          // What an idle frame would be with exclusions lifted, cumulatively.
          renderer.setRetainVerify(false)
          const lifted: string[] = []
          for (const [label, names] of LIFTS) {
            lifted.push(...names)
            renderer.setRetainLift(lifted)
            for (let i = 0; i < 3; i++) renderer.clockFastForward(FRAME_MS)
            frames(root, `${page.name} idle lifting ${label}`, idle)
          }
        }
      } finally {
        root.unmount()
      }
    }, 300_000)
    }
  }
})
