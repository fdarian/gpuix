import React, { useLayoutEffect, useRef, useState } from 'react'
import { render } from '@ghia/react'
import type { ImgInstance, PublicInstance } from '@ghia/react'

const WIDTH = 720
const HEIGHT = 96
const PIXEL_RATIO = 2

// BGRA is GPUI's native byte order, so the upload skips a per-pixel swizzle.
function bgraWaveform(phase: number): Buffer {
  const width = WIDTH * PIXEL_RATIO
  const height = HEIGHT * PIXEL_RATIO
  const bytes = Buffer.alloc(width * height * 4)
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const i = (y * width + x) * 4
      bytes[i] = 38
      bytes[i + 1] = 22
      bytes[i + 2] = 18
      bytes[i + 3] = 255
    }
  }
  const mid = (height - 1) / 2
  for (let x = 0; x < width; x++) {
    const sample = Math.sin((x / width) * Math.PI * 6 + phase) * 0.72
    const y = Math.round(mid - sample * mid)
    const i = (y * width + x) * 4
    bytes[i] = 255
    bytes[i + 1] = 169
    bytes[i + 2] = 92
    bytes[i + 3] = 255
  }
  return bytes
}

function Waveform({ phase }: { phase: number }) {
  const img = useRef<ImgInstance>(null)

  useLayoutEffect(() => {
    img.current?.setImagePixels(
      WIDTH * PIXEL_RATIO,
      HEIGHT * PIXEL_RATIO,
      bgraWaveform(phase),
      { format: 'bgra' },
    )
  }, [phase])

  return (
    <img
      ref={img}
      testId="waveform"
      objectFit="fill"
      style={{ width: WIDTH, height: HEIGHT, borderRadius: 10 }}
    />
  )
}

const CLIPS = ['Intro', 'Verse', 'Chorus', 'Bridge', 'Outro']

export function WaveformApp({
  phase = 0,
}: {
  phase?: number
} = {}) {
  const lastClip = useRef<PublicInstance>(null)

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        gap: 16,
        padding: 24,
        width: '100%',
        height: '100%',
        backgroundColor: '#11111b',
      }}
    >
      <text style={{ color: '#cdd6f4', fontSize: 18, fontWeight: 600 }}>
        Waveform
      </text>
      <Waveform phase={phase} />
      <div
        testId="clip-list"
        style={{
          height: 120,
          overflow: 'scroll',
          borderRadius: 10,
          backgroundColor: '#1e1e2e',
        }}
      >
        {CLIPS.map((name, index) => (
          <div
            key={name}
            ref={index === CLIPS.length - 1 ? lastClip : undefined}
            testId={`clip-${name.toLowerCase()}`}
            style={{
              height: 48,
              padding: 14,
              color: '#cdd6f4',
              backgroundColor: index % 2 === 0 ? '#1e1e2e' : '#181825',
            }}
          >
            {name}
          </div>
        ))}
      </div>
      <div
        testId="jump-outro"
        style={{
          padding: 10,
          borderRadius: 8,
          backgroundColor: '#89b4fa',
          color: '#1e1e2e',
          width: 140,
        }}
        onClick={() => lastClip.current?.scrollIntoView?.()}
      >
        Jump to outro
      </div>
    </div>
  )
}

function App() {
  const [phase, setPhase] = useState(0)
  return (
    <div
      style={{ width: '100%', height: '100%' }}
      onClick={() => setPhase((value) => value + 0.6)}
    >
      <WaveformApp phase={phase} />
    </div>
  )
}

if (import.meta.main) {
  render(<App />, {
    title: 'GPUIX Waveform',
    width: 800,
    height: 420,
    focus: process.env.GPUIX_BACKGROUND !== '1',
  })
}
