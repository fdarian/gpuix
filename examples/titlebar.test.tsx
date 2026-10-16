import fs from 'fs'
import path from 'path'
import { fileURLToPath } from 'url'
import { describe, expect, it } from 'vitest'
import React from 'react'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'
import { TitlebarApp } from './titlebar'

const describeNative =
  hasNativeTestRenderer && process.platform !== 'win32' ? describe : describe.skip
const SHOTS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), 'screenshots')

fs.mkdirSync(SHOTS, { recursive: true })

function center(root: ReturnType<typeof createTestRoot>, testId: string) {
  const element = root.renderer.findByTestId(testId)!
  const bounds = root.renderer.getElementBounds(element.id)!
  return { x: bounds.x + bounds.width / 2, y: bounds.y + bounds.height / 2 }
}

describeNative('titlebar example', () => {
  it('moves the window from the bar and shows the page no event', () => {
    const root = createTestRoot({ width: 640, height: 360, titlebarTransparent: true })
    root.render(<TitlebarApp />)
    const title = center(root, 'title')

    root.renderer.nativeSimulateMouseDown(title.x, title.y)
    root.renderer.nativeSimulateMouseUp(title.x, title.y)

    expect(root.renderer.takeSimulatedWindowActions()).toEqual(['move'])
    expect(root.renderer.findByText('none yet')).toBeDefined()
  })

  it('counts clicks on the no-drag button without moving the window', async () => {
    const root = createTestRoot({ width: 640, height: 360, titlebarTransparent: true })
    root.render(<TitlebarApp />)
    const point = center(root, 'count-button')

    root.renderer.nativeSimulateClick(point.x, point.y)
    root.renderer.nativeSimulateClick(point.x, point.y)
    await new Promise((resolve) => setTimeout(resolve, 50))
    root.renderer.flush()

    expect(root.renderer.takeSimulatedWindowActions()).toEqual([])
    expect(root.renderer.findByText('Clicked 2')).toBeDefined()
  })

  it('paints the bar', () => {
    const root = createTestRoot({ width: 640, height: 360, titlebarTransparent: true })
    const shot = path.join(SHOTS, 'titlebar.png')
    if (fs.existsSync(shot)) fs.unlinkSync(shot)
    root.render(<TitlebarApp />)
    root.renderer.flush()
    root.renderer.captureScreenshot(shot)
    expect(fs.statSync(shot).size).toBeGreaterThan(0)
  })
})
