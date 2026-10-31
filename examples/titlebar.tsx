/** A custom titlebar: a drag region that moves the window, with interactive controls cut out of it. */

import { useState } from 'react'
import { render } from '@ghia/react'

const muted = '#FFFFFF99'

export function TitlebarApp() {
  const [clicks, setClicks] = useState(0)
  const [name, setName] = useState('')
  const [lastEvent, setLastEvent] = useState('none yet')

  return (
    <div
      style={{
        width: '100%',
        height: '100%',
        display: 'flex',
        flexDirection: 'column',
        backgroundColor: '#14181F',
      }}
    >
      <div
        testId="titlebar"
        style={{
          appRegion: 'drag',
          height: 40,
          display: 'flex',
          flexDirection: 'row',
          alignItems: 'center',
          paddingLeft: 80,
          paddingRight: 12,
          backgroundColor: '#1D232C',
          borderBottomWidth: 1,
          borderColor: '#FFFFFF1F',
          borderBottomStyle: 'solid',
        }}
      >
        <div
          testId="title"
          onPointerDown={(event) => setLastEvent(event.type)}
          onPointerMove={(event) => setLastEvent(event.type)}
          onClick={(event) => setLastEvent(event.type)}
          style={{ flexGrow: 1, height: '100%', display: 'flex', alignItems: 'center', justifyContent: 'center' }}
        >
          <text style={{ color: '#FFFFFF', fontSize: 13, fontWeight: 600 }}>Titlebar</text>
        </div>
        <div style={{ appRegion: 'no-drag', display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 8 }}>
          <div
            testId="count-button"
            onClick={() => setClicks((value) => value + 1)}
            style={{
              height: 26,
              paddingLeft: 12,
              paddingRight: 12,
              display: 'flex',
              alignItems: 'center',
              borderRadius: 6,
              backgroundColor: '#FFFFFF14',
              cursor: 'pointer',
              hover: { backgroundColor: '#FFFFFF2B' },
            }}
          >
            <text style={{ color: '#FFFFFFE8', fontSize: 12 }}>{`Clicked ${clicks}`}</text>
          </div>
          <input
            testId="name-input"
            value={name}
            placeholder="Your name"
            onChange={(event) => setName(event.value ?? '')}
            style={{
              width: 140,
              height: 26,
              paddingLeft: 10,
              paddingRight: 10,
              borderRadius: 6,
              backgroundColor: '#0E1217',
              color: '#FFFFFF',
              fontSize: 12,
            }}
          />
        </div>
      </div>

      <div style={{ padding: 24, gap: 8 }}>
        <text style={{ color: muted, fontSize: 12, fontWeight: 600 }}>LAST EVENT ON THE TITLE</text>
        <text testId="last-event" style={{ color: '#FFFFFF', fontSize: 22, fontWeight: 600 }}>
          {lastEvent}
        </text>
        <text style={{ color: muted, fontSize: 13, lineHeight: "20px" }}>
          Drag the bar to move the window. Presses and moves over a drag region never reach the page, so
          this stays put; the button and the input are no-drag and get theirs.
        </text>
      </div>
    </div>
  )
}

if (import.meta.main) {
  render(<TitlebarApp />, {
    title: 'GPUIX Titlebar',
    appName: 'GPUIX Titlebar',
    width: 640,
    height: 360,
    titlebarTransparent: true,
    trafficLightX: 18,
    trafficLightY: 13,
    focus: process.env.GPUIX_BACKGROUND !== '1',
  })
}
