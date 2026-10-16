/**
 * Drives the disktree example end to end: a real scan of a small fixture
 * folder, the real layout, and clicks through the GPU test renderer.
 *
 * Screenshots go to `disktree/screenshots/*.png` for inspection.
 */

import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'
import React from 'react'
import { afterAll, describe, expect, it } from 'vitest'
import { connectTest } from '@gpuix/react/automation'
import { createTestRoot, hasNativeTestRenderer } from '@gpuix/react/testing'

import { DisktreeApp, createDisktreeStore } from './app.tsx'
import { displayPath } from './core.ts'

const describeNative = hasNativeTestRenderer ? describe : describe.skip
const SHOTS = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', 'screenshots')
fs.mkdirSync(SHOTS, { recursive: true })
const KIB = 1024

const fixture = fs.mkdtempSync(path.join(os.tmpdir(), 'disktree-'))
const root = path.join(fixture, 'project')

function write(relative: string, bytes: number) {
  const full = path.join(root, relative)
  fs.mkdirSync(path.dirname(full), { recursive: true })
  fs.writeFileSync(full, Buffer.alloc(bytes, 1))
}

write('package.json', 200)
write('node_modules/react/index.js', 2048 * KIB)
write('node_modules/react/cjs/react.js', 1024 * KIB)
write('node_modules/zod/index.js', 512 * KIB)
write('.cache/blob', 1024 * KIB)
write('src/app.ts', 400 * KIB)
write('src/lib/scan.ts', 200 * KIB)
write('docs/readme.md', 96 * KIB)

afterAll(() => fs.rmSync(fixture, { recursive: true, force: true }))

describeNative('disktree', () => {
  it('scans, opens on one click, and goes back up', async () => {
    const store = createDisktreeStore(root)
    await store.rescan()
    const tree = store.get().tree!
    if (!tree.dir) throw new Error('Expected scanned folder')
    expect(tree.children.map((child) => `${child.name} ${child.category} ${child.reclaim}`)).toMatchInlineSnapshot(`
      [
        "node_modules cache reinstallable",
        ".cache cache regenerable",
        "src code null",
        "docs documents null",
        "package.json other null",
      ]
    `)

    const { render, renderer } = createTestRoot({ width: 1320, height: 840 })
    const size = renderer.getWindowSize()
    render(<DisktreeApp store={store} width={size.width} height={size.height} />)
    const app = await connectTest(renderer)
    await app.screenshot({ path: path.join(SHOTS, 'root.png') })

    const name = () => app.getByTestId('selection-name').textContent()
    expect(await name()).toBe('project')

    // node_modules is the largest child, crumbs [0]. Its centre is a child
    // tile, so press on its name band.
    const band = await app.getByTestId('tile-0').bounds()
    const onBand = { x: band.x + 30, y: band.y + 10 }
    // One click opens the folder.
    await app.mouse.click(onBand)
    expect(await name()).toBe('node_modules')
    // Path and free space differ per machine; the rest is fixed by the fixture.
    const panel = (await app.getByTestId('panel').textContent())
      .replace(displayPath(path.join(root, 'node_modules')), '…')
      .replace(/Show in (Finder|Explorer)/, 'Show in File Manager')
      .replace(/DISK.*/, '')
    expect(panel).toMatchInlineSnapshot(`"SELECTIONnode_modules…3.5MiBOF SCAN68%FILES3LAST WRITEjust nowKINDCache · reinstallableShow in File ManagerWORTH A LOOK4.5 MiBnode_modulesreinstallable3.5 MiB.cacheregenerable1.0 MiB"`)
    expect(await app.getByTestId('crumb-0').textContent()).toBe('node_modules')
    await app.screenshot({ path: path.join(SHOTS, 'node-modules.png') })

    await app.getByTestId('disktree').press('Backspace')
    expect(await name()).toBe('node_modules')

    // The back button does the same as ⌫.
    await app.mouse.click(onBand)
    expect(await app.getByTestId('crumb-0').textContent()).toBe('node_modules')
    await app.getByTestId('back').click()
    expect(await name()).toBe('node_modules')

    await app.close()
  })
})
