/**
 * GPUIX counter for hermes-node. Bundled to CJS; the .node stays external.
 */
import React, { useState } from 'react'
import { render } from '@ghia/react'

function App() {
  const [count, setCount] = useState(0)
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        width: '100%',
        height: '100%',
        backgroundColor: '#11111b',
        gap: 16,
      }}
    >
      <text style={{ fontSize: 28, color: '#cdd6f4', fontWeight: 700 }}>
        GPUIX on Hermes
      </text>
      <div
        testId="count"
        onClick={() => setCount((c) => c + 1)}
        style={{
          padding: 16,
          backgroundColor: '#a6e3a1',
          borderRadius: 8,
          cursor: 'pointer',
        }}
      >
        <text style={{ color: '#1e1e2e', fontWeight: 700, fontSize: 20 }}>
          {String(count)}
        </text>
      </div>
    </div>
  )
}

render(<App />, {
  title: 'GPUIX Hermes',
  width: 480,
  height: 320,
  focus: process.env.GPUIX_BACKGROUND !== '1',
})

const screenshotPath = process.env.GPUIX_SCREENSHOT
if (screenshotPath) {
  setTimeout(() => {
    const slot = globalThis.__gpuixRenderHost as
      | { renderer?: { captureScreenshot?: (path: string) => void; getPaintedText?: () => string[] } }
      | undefined
    const renderer = slot?.renderer
    if (!renderer?.captureScreenshot) {
      console.error('no renderer for screenshot')
      process.exit(1)
    }
    console.log('painted', renderer.getPaintedText?.())
    renderer.captureScreenshot(screenshotPath)
    console.log('screenshot', screenshotPath)
    process.exit(0)
  }, 400)
}
