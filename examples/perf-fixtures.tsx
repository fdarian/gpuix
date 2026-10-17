/** A page of cards, the static content of the animating-node and draws-vs-presents benchmarks. */

import React from 'react'

export const C = {
  bg: '#0b0b0c',
  surface: '#161618',
  border: '#2a2a2e',
  fg: '#f2f2f3',
  muted: '#9a9aa1',
  chip: '#24242a',
}

/** A tween long enough that the motion node is mid-animation for every sampled frame. */
export const LONG_TWEEN = { duration: 600, ease: 'linear' } as const

function Chip({ label }: { label: string }) {
  return (
    <div style={{ paddingLeft: 6, paddingRight: 6, height: 20, borderRadius: 5, backgroundColor: C.chip, justifyContent: 'center' }}>
      <text style={{ fontSize: 11, color: C.muted }}>{label}</text>
    </div>
  )
}

function Action({ label }: { label: string }) {
  return (
    <div style={{ height: 28, paddingLeft: 10, paddingRight: 10, borderRadius: 6, backgroundColor: C.chip, justifyContent: 'center' }}>
      <text style={{ fontSize: 12, color: C.fg }}>{label}</text>
    </div>
  )
}

/** About 34 retained elements: a header with avatar and two lines, three paragraphs, four chips, two actions. */
export function Card({ index }: { index: number }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: 280,
        gap: 8,
        padding: 14,
        borderRadius: 12,
        borderWidth: 1,
        borderColor: C.border,
        borderStyle: 'solid',
        backgroundColor: C.surface,
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 10 }}>
        <div style={{ width: 32, height: 32, borderRadius: 16, backgroundColor: C.chip }} />
        <div style={{ display: 'flex', flexDirection: 'column' }}>
          <text style={{ fontSize: 14, color: C.fg }}>{`Release ${index}`}</text>
          <text style={{ fontSize: 12, color: C.muted }}>{`Month ${index % 12}`}</text>
        </div>
      </div>
      <text style={{ fontSize: 13, color: C.fg }}>Every change, month by month, with what moved.</text>
      <text style={{ fontSize: 13, color: C.muted }}>A breaking change carries its diff and a prompt.</text>
      <text style={{ fontSize: 13, color: C.muted }}>Nothing here is silent.</text>
      <div style={{ display: 'flex', flexDirection: 'row', gap: 4 }}>
        <Chip label="breaking" />
        <Chip label="layout" />
        <Chip label="paint" />
        <Chip label="text" />
      </div>
      <div style={{ display: 'flex', flexDirection: 'row', gap: 8 }}>
        <Action label="Diff" />
        <Action label="Prompt" />
      </div>
    </div>
  )
}

export function CardGrid({ cards }: { cards: number }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', flexWrap: 'wrap', gap: 16, padding: 20, alignContent: 'flex-start' }}>
      {Array.from({ length: cards }, (_, index) => (
        <Card key={index} index={index} />
      ))}
    </div>
  )
}
