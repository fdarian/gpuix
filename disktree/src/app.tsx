/** @jsxImportSource @gpuix/react */
/**
 * disktree: a treemap of what fills a folder, on a frosted macOS window.
 *
 * A GPUIX port of https://github.com/tobi/disktree, in light mode.
 *
 * The window and argv handling live in cli.tsx; this module only exports
 * the store and the view, so tests can mount it on the test renderer.
 *
 * Click a folder to go in; click a file to select it.
 * ⌫ or Esc goes up, [ and ] change the depth, t cycles Size/Files/Age,
 * r scans again.
 *
 * The mosaic is one flat list of absolutely placed tiles. They set
 * `pointerEvents: "none"`, so the container owns every mouse event and
 * hit-tests in JS, like the Rust original does on its canvas.
 */

import { spawn } from 'node:child_process'
import path from 'node:path'
import { memo, useMemo, useState, useSyncExternalStore } from 'react'
import { useWindowSize, type DomEvent } from '@gpuix/react'

import {
  AGE_BUCKETS,
  LAYOUT_DEFAULTS,
  ageBucket,
  ago,
  diskSpace,
  displayPath,
  hit,
  humanBytes,
  humanCount,
  layout,
  resolve,
  resolveChain,
  scan,
  valueOf,
  worthALook,
  type Candidate,
  type Category,
  type DiskSpace,
  type Metric,
  type Rect,
  type ScanProgress,
  type Tile,
  type TreeNode,
} from './core.ts'

// ── Design tokens ──────────────────────────────────────────────────────────

const INK = '#1F1530'
const INK_2 = '#1F15309E'
const INK_3 = '#1F153066'
const HAIRLINE = '#1F153014'
const HIGHLIGHT = '#F75407'
// Every fill is translucent, so the frosted window shows through everywhere.
const CARD = '#FFFFFFE6'
// GPUI's blur is colorless: it only blurs the desktop. A dark wallpaper would
// make the window dark, so this tint carries the light look on its own.
const WINDOW_TINT = '#EFECF7A8'
// Apple-style controls: translucent white pills on the window, gray when
// selected or pressed.
const CONTROL = '#FFFFFFF2'
const CONTROL_HOVER = '#F8F7FBF2'
const CONTROL_SELECTED = '#1F153014'
const CONTROL_PRESSED = '#1F15301F'
const CONTROL_SHADOW = { offsetX: 0, offsetY: 0.5, blurRadius: 3, spreadRadius: 0, color: '#1F153014' }

/** Saturated light hues, from the palette: blue, pink, purple, orange, lavender. */
const CATEGORY: Record<Category, { color: string; label: string }> = {
  code: { color: '#0496E8', label: 'Code' },
  agent: { color: '#F76FE6', label: 'Agent scratch' },
  toolchain: { color: '#8A5BA8', label: 'Toolchains' },
  synced: { color: '#3FC1F4', label: 'Synced' },
  git: { color: '#C04FC8', label: 'Git' },
  media: { color: '#A98BF2', label: 'Media' },
  documents: { color: '#FF9A5C', label: 'Documents' },
  cache: { color: '#6F7CFF', label: 'Cache' },
  other: { color: '#B9AEDC', label: 'Other' },
}
const LEGEND: Category[] = ['code', 'agent', 'toolchain', 'synced', 'git', 'media', 'documents', 'cache']
const AGE_COLORS = ['#F75407', '#FF9A5C', '#F76FE6', '#A98BF2', '#B9AEDC']

// Tiles are translucent so the frosted window shows through. A child sits on
// its parent's fill, so painting the hue again would stack into a dark mud.
// A child of the same kind adds a white wash instead: deeper reads lighter.
const TILE_ALPHA = 0.9
const DEPTH_WASH = '#FFFFFF38'

/** Light, still saturated: the hue pulled halfway to white. */
function pastel(hex: string): string {
  const channel = (offset: number) => parseInt(hex.slice(offset, offset + 2), 16)
  const lift = (value: number) => Math.round(value + (255 - value) * 0.55).toString(16).padStart(2, '0')
  return `#${lift(channel(1))}${lift(channel(3))}${lift(channel(5))}`
}

type Rgb = [number, number, number]

/** `#RRGGBBAA` painted over an opaque colour. */
function over(fill: string, below: Rgb): Rgb {
  const a = fill.length > 7 ? parseInt(fill.slice(7, 9), 16) / 255 : 1
  return [0, 1, 2].map((index) => {
    const channel = parseInt(fill.slice(1 + index * 2, 3 + index * 2), 16)
    return channel * a + below[index]! * (1 - a)
  }) as Rgb
}

/** Black or white, whichever has more WCAG contrast on this background. */
function inkOn([r, g, b]: Rgb): '#000000' | '#FFFFFF' {
  const linear = (value: number) => {
    const c = value / 255
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4
  }
  const luminance = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
  // Contrast with white equals contrast with black at L ≈ 0.179.
  return luminance > 0.179 ? '#000000' : '#FFFFFF'
}

// The tint is what the tiles sit on; the desktop behind it is unknown, so the
// tint is treated as opaque when choosing label ink.
const WINDOW_BASE: Rgb = over(WINDOW_TINT.slice(0, 7), [0, 0, 0])

const TEXT = { caption: 11, body: 12, title: 14, heading: 18, figure: 26, display: 40 }

// Window geometry. The treemap origin is derived from these, so a mouse
// position maps to a tile without measuring anything.
const TOP_BAR = 54
const LEGEND_ROW = 34
const KEY_BAR = 36
const PAD_LEFT = 14
const PAD_RIGHT = 10
const PAD_BOTTOM = 10
const PANEL_WIDTH = 304
const PANEL_GAP = 12
// macOS paints the traffic lights over the app.
const TRAFFIC_LIGHTS = process.platform === 'darwin' ? 84 : 14

function alpha(hex: string, value: number): string {
  const byte = Math.round(Math.max(0, Math.min(1, value)) * 255)
  return `${hex.slice(0, 7)}${byte.toString(16).padStart(2, '0')}`
}

// ── Store ──────────────────────────────────────────────────────────────────

export interface DisktreeState {
  root: string
  includeHidden: boolean
  tree: TreeNode | null
  progress: ScanProgress
  scanning: boolean
  error: string | null
  elapsedMs: number | null
  scannedAt: number
  disk: DiskSpace | null
}

export type DisktreeStore = ReturnType<typeof createDisktreeStore>

/** Scan state lives outside React; the view subscribes with useSyncExternalStore. */
export function createDisktreeStore(root: string) {
  let state: DisktreeState = {
    root: path.resolve(root),
    includeHidden: true,
    tree: null,
    progress: { files: 0, dirs: 0, bytes: 0, errors: 0 },
    scanning: false,
    error: null,
    elapsedMs: null,
    scannedAt: Date.now(),
    disk: null,
  }
  const listeners = new Set<() => void>()
  let current: AbortController | null = null
  const set = (patch: Partial<DisktreeState>) => {
    state = { ...state, ...patch }
    for (const listener of listeners) listener()
  }

  /** Scan again. The old tree stays on screen until the new one lands. */
  async function rescan(includeHidden = state.includeHidden): Promise<void> {
    current?.abort()
    const controller = new AbortController()
    current = controller
    const progress: ScanProgress = { files: 0, dirs: 0, bytes: 0, errors: 0 }
    const started = performance.now()
    set({ includeHidden, scanning: true, error: null, progress: { ...progress } })
    const timer = setInterval(() => set({ progress: { ...progress } }), 100)
    try {
      const tree = await scan(state.root, { includeHidden, progress, signal: controller.signal })
      if (current !== controller) return
      set({
        tree,
        scanning: false,
        progress: { ...progress },
        elapsedMs: performance.now() - started,
        scannedAt: Date.now(),
        disk: diskSpace(state.root),
      })
    } catch (error) {
      if (current !== controller) return
      set({ scanning: false, error: error instanceof Error ? error.message : String(error) })
    } finally {
      clearInterval(timer)
    }
  }

  return {
    get: () => state,
    subscribe(listener: () => void) {
      listeners.add(listener)
      return () => {
        listeners.delete(listener)
      }
    },
    rescan,
  }
}

// ── Small parts ────────────────────────────────────────────────────────────

type Mode = 'size' | 'files' | 'age'

function Eyebrow({ children }: { children: string }) {
  return <text style={{ fontSize: TEXT.caption, fontWeight: 600, color: INK_3 }}>{children.toUpperCase()}</text>
}

function Swatch({ color, size = 10 }: { color: string; size?: number }) {
  return <div style={{ width: size, height: size, borderRadius: 3, backgroundColor: color, flexShrink: 0 }} />
}

function Bar({ fraction, color, height = 4 }: { fraction: number; color: string; height?: number }) {
  return (
    <div style={{ width: '100%', height, borderRadius: height / 2, backgroundColor: HAIRLINE }}>
      <div
        style={{
          width: `${Math.max(0, Math.min(1, fraction)) * 100}%`,
          height: '100%',
          borderRadius: height / 2,
          backgroundColor: color,
        }}
      />
    </div>
  )
}

function PlainButton({
  label,
  onClick,
  primary = false,
  testId,
}: {
  label: string
  onClick: () => void
  primary?: boolean
  testId?: string
}) {
  return (
    <div
      testId={testId}
      onClick={onClick}
      style={{
        display: 'flex',
        flexGrow: 1,
        alignItems: 'center',
        justifyContent: 'center',
        height: 30,
        borderRadius: 8,
        cursor: 'pointer',
        // On the white card a secondary button needs an edge, not a fill.
        backgroundColor: primary ? '#F75407E6' : CONTROL,
        borderWidth: primary ? 0 : 1,
        borderColor: '#1F153014',
        boxShadow: CONTROL_SHADOW,
        hover: { backgroundColor: primary ? '#FF6A20E6' : '#F4F2F8E6' },
        active: { backgroundColor: primary ? '#DE4A05E6' : '#E9E6F0E6' },
      }}
    >
      <text style={{ fontSize: TEXT.body + 1, fontWeight: primary ? 600 : 500, color: primary ? '#FFFFFF' : INK }}>
        {label}
      </text>
    </div>
  )
}

function Checkbox({ label, checked, onToggle, testId }: { label: string; checked: boolean; onToggle: () => void; testId: string }) {
  return (
    <div
      testId={testId}
      onClick={onToggle}
      style={{ display: 'flex', alignItems: 'center', gap: 7, cursor: 'pointer', paddingLeft: 4, paddingRight: 4 }}
    >
      <div
        style={{
          display: 'flex',
          width: 15,
          height: 15,
          borderRadius: 4,
          alignItems: 'center',
          justifyContent: 'center',
          backgroundColor: checked ? '#0496E8E6' : CONTROL,
          borderWidth: checked ? 0 : 1,
          borderColor: '#1F153033',
        }}
      >
        {checked ? <text style={{ fontSize: 10, fontWeight: 700, color: '#FFFFFF' }}>✓</text> : null}
      </div>
      <text style={{ fontSize: TEXT.body + 1, color: INK }}>{label}</text>
    </div>
  )
}

function Segmented({ value, onChange }: { value: Mode; onChange: (mode: Mode) => void }) {
  const items: Array<[Mode, string]> = [
    ['size', 'Size'],
    ['files', 'Files'],
    ['age', 'Age'],
  ]
  return (
    <div style={{ display: 'flex', padding: 2, gap: 2, borderRadius: 9, backgroundColor: CONTROL, boxShadow: CONTROL_SHADOW }}>
      {items.map(([mode, label]) => {
        const active = mode === value
        return (
          <div
            key={mode}
            testId={`mode-${mode}`}
            onClick={() => onChange(mode)}
            style={{
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              width: 58,
              height: 26,
              borderRadius: 7,
              cursor: 'pointer',
              backgroundColor: active ? CONTROL_SELECTED : undefined,
              hover: active ? {} : { backgroundColor: '#1F15300A' },
              active: { backgroundColor: CONTROL_PRESSED },
            }}
          >
            <text style={{ fontSize: TEXT.body + 1, fontWeight: active ? 600 : 400, color: active ? INK : INK_2 }}>{label}</text>
          </div>
        )
      })}
    </div>
  )
}

function Keycap({ keys, label }: { keys: string; label: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
      <div
        style={{
          paddingLeft: 6,
          paddingRight: 6,
          height: 20,
          display: 'flex',
          alignItems: 'center',
          borderRadius: 5,
          backgroundColor: CONTROL,
          boxShadow: CONTROL_SHADOW,
        }}
      >
        <text style={{ fontSize: TEXT.caption, color: INK }}>{keys}</text>
      </div>
      <text style={{ fontSize: TEXT.caption, color: INK_2 }}>{label}</text>
    </div>
  )
}

const CHEVRON_LEFT =
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2.2" ' +
  'stroke-linecap="round" stroke-linejoin="round"><path d="m15 18-6-6 6-6"/></svg>'

/** Up one level, like ⌫ and Esc. */
function BackButton({ disabled, onClick }: { disabled: boolean; onClick: () => void }) {
  return (
    <div
      testId="back"
      onClick={disabled ? undefined : onClick}
      style={{
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        width: 28,
        height: 28,
        borderRadius: 8,
        cursor: disabled ? 'default' : 'pointer',
        backgroundColor: CONTROL,
        boxShadow: CONTROL_SHADOW,
        hover: disabled ? {} : { backgroundColor: CONTROL_HOVER },
        active: disabled ? {} : { backgroundColor: CONTROL_PRESSED },
      }}
    >
      <svg source={CHEVRON_LEFT} style={{ width: 15, height: 15, color: disabled ? INK_3 : INK }} />
    </div>
  )
}

/** Four tiles in category colours, and the name. */
function Logo() {
  const tile = (category: Category) => (
    <div style={{ width: 8, height: 8, borderRadius: 2, backgroundColor: CATEGORY[category].color }} />
  )
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 9, flexShrink: 0 }}>
      <div style={{ display: 'flex', gap: 2 }}>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {tile('code')}
          {tile('synced')}
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
          {tile('agent')}
          {tile('toolchain')}
        </div>
      </div>
      <text style={{ fontSize: TEXT.heading, fontWeight: 600, color: INK }}>disktree</text>
    </div>
  )
}

// ── Mosaic ─────────────────────────────────────────────────────────────────

interface TileDeco {
  key: string
  rect: Rect
  fill: string
  strip: string | null
  hatch: boolean
  label: { name: string; size: string; header: Rect | null; depth: number; ink: string } | null
}

const MAX_LABELS = 150

function hatchSvg(width: number, height: number): string {
  return (
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}" viewBox="0 0 ${width} ${height}">` +
    '<defs><pattern id="h" width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)">' +
    '<rect width="1.4" height="6" fill="#000"/></pattern></defs>' +
    `<rect width="${width}" height="${height}" fill="url(#h)"/></svg>`
  )
}

function decorate(tree: TreeNode, tiles: Tile[], mode: Mode, metric: Metric, now: number): TileDeco[] {
  const labelled = new Set(
    tiles
      .filter((tile) => {
        const band = tile.header ?? tile.rect
        return band.w >= 38 && band.h >= 15
      })
      .sort((left, right) => right.rect.w * right.rect.h - left.rect.w * left.rect.h)
      .slice(0, MAX_LABELS),
  )
  // Tiles arrive parents first, so each one blends over its parent's result.
  const painted = new Map<string, Rgb>()
  return tiles.map((tile) => {
    const node = tile.others ? undefined : resolve(tree, tile.crumbs)
    const hueOf = (entry: TreeNode | undefined) =>
      mode === 'age' && entry?.modified
        ? AGE_COLORS[ageBucket((now - entry.modified) / 86_400_000)]!
        : CATEGORY[entry?.category ?? 'other'].color
    const hue = hueOf(node)
    const parent = resolve(tree, tile.crumbs.slice(0, -1))
    const sameAsParent = tile.depth > 0 && parent && hueOf(parent) === hue
    const fill = sameAsParent ? DEPTH_WASH : alpha(pastel(hue), TILE_ALPHA)
    const parentKey = (tile.others ? tile.crumbs : tile.crumbs.slice(0, -1)).join('/')
    const background = over(fill, painted.get(parentKey) ?? WINDOW_BASE)
    if (!tile.others) painted.set(tile.crumbs.join('/'), background)
    const size =
      metric === 'files'
        ? node?.dir
          ? `${humanCount(node.files)} files`
          : ''
        : node
          ? humanBytes(valueOf(node, metric))
          : ''
    return {
      key: `${tile.crumbs.join('-')}${tile.others ? '+' : ''}`,
      rect: tile.rect,
      fill,
      strip: tile.depth === 0 && mode !== 'age' ? hue : null,
      // Only the outermost reclaimable tile is hatched; its children are
      // translucent, so the hatch shows through them.
      hatch: Boolean(node?.reclaim) && !parent?.reclaim,
      label: labelled.has(tile)
        ? {
            name: tile.others ? `+${tile.others} more` : (node?.name ?? ''),
            size: tile.others ? '' : size,
            header: tile.header,
            depth: tile.depth,
            ink: inkOn(background),
          }
        : null,
    }
  })
}

function TileLabel({ label, rect }: { label: NonNullable<TileDeco['label']>; rect: Rect }) {
  const nameStyle = {
    fontSize: TEXT.body,
    fontWeight: label.depth === 0 && label.header ? 600 : 400,
    color: label.ink,
    whiteSpace: 'nowrap' as const,
    flexShrink: 0,
  }
  const sizeStyle = { fontSize: TEXT.caption, color: alpha(label.ink, 0.62), whiteSpace: 'nowrap' as const, flexShrink: 0 }
  if (label.header) {
    return (
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          gap: 6,
          height: label.header.h,
          paddingLeft: 6,
          paddingRight: 6,
        }}
      >
        <text style={nameStyle}>{label.name}</text>
        {label.depth === 0 ? <div style={{ flexGrow: 1 }} /> : null}
        {label.size && rect.w > 110 ? <text style={sizeStyle}>{label.size}</text> : null}
      </div>
    )
  }
  const stacked = rect.h >= 36
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: stacked ? 'column' : 'row',
        gap: stacked ? 1 : 6,
        paddingLeft: 6,
        paddingTop: 3,
        paddingRight: 6,
      }}
    >
      <text style={nameStyle}>{label.name}</text>
      {label.size && (stacked || rect.w > 110) ? <text style={sizeStyle}>{label.size}</text> : null}
    </div>
  )
}

/** Memoized, so hover and selection only move the two outline boxes. */
const TileLayer = memo(function TileLayer({ tiles }: { tiles: TileDeco[] }) {
  return (
    <>
      {tiles.map((tile) => (
        <div
          key={tile.key}
          testId={`tile-${tile.key}`}
          style={{
            position: 'absolute',
            left: tile.rect.x,
            top: tile.rect.y,
            width: tile.rect.w,
            height: tile.rect.h,
            overflow: 'hidden',
            pointerEvents: 'none',
            backgroundColor: tile.fill,
            borderTopWidth: tile.strip ? 2 : 0,
            borderColor: tile.strip ?? tile.fill,
          }}
        >
          {tile.hatch ? (
            <svg
              source={hatchSvg(Math.round(tile.rect.w), Math.round(tile.rect.h))}
              style={{
                position: 'absolute',
                left: 0,
                top: 0,
                width: Math.round(tile.rect.w),
                height: Math.round(tile.rect.h),
                color: '#1F153026',
                pointerEvents: 'none',
              }}
            />
          ) : null}
          {tile.label ? <TileLabel label={tile.label} rect={tile.rect} /> : null}
        </div>
      ))}
    </>
  )
})

function Outline({ rect, width, color }: { rect: Rect; width: number; color: string }) {
  return (
    <div
      style={{
        position: 'absolute',
        left: rect.x,
        top: rect.y,
        width: rect.w,
        height: rect.h,
        borderWidth: width,
        borderColor: color,
        pointerEvents: 'none',
      }}
    />
  )
}

// ── Panel ──────────────────────────────────────────────────────────────────

function Figure({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 3, flexGrow: 1, flexBasis: 0, minWidth: 0 }}>
      <text style={{ fontSize: TEXT.caption - 0.5, fontWeight: 600, color: INK_3 }}>{label.toUpperCase()}</text>
      <text style={{ fontSize: TEXT.title + 1, color: INK, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{value}</text>
    </div>
  )
}

interface PanelProps {
  state: DisktreeState
  tree: TreeNode
  target: number[]
  current: number[]
  metric: Metric
  insights: Candidate[]
  onOpen: (crumbs: number[]) => void
  /** Go to the parent and select it, so it is on screen. */
  onShow: (crumbs: number[]) => void
}

function SidePanel({ state, tree, target, current, metric, insights, onOpen, onShow }: PanelProps) {
  const node = resolve(tree, target) ?? tree
  const full = path.join(state.root, ...resolveChain(tree, target).slice(1).map((node) => node.name))
  const [revealError, setRevealError] = useState<{ path: string; message: string } | null>(null)
  const revealLabel = process.platform === 'darwin' ? 'Show in Finder'
    : process.platform === 'win32' ? 'Show in Explorer' : 'Show in File Manager'
  const reveal = () => {
    setRevealError(null)
    const command = process.platform === 'darwin' ? 'open'
      : process.platform === 'win32' ? 'explorer.exe' : 'xdg-open'
    const args = process.platform === 'darwin' ? ['-R', full]
      : process.platform === 'win32' ? [`/select,"${full}"`] : [node.dir ? full : path.dirname(full)]
    const child = spawn(command, args, {
      stdio: 'ignore',
      detached: true,
      windowsVerbatimArguments: process.platform === 'win32',
    })
    child.once('error', (error) => setRevealError({ path: full, message: error.message }))
    child.unref()
  }
  const [number, unit] =
    metric === 'files' ? [humanCount(node.files), 'files'] : humanBytes(valueOf(node, metric)).split(' ')
  const share = valueOf(node, metric) / Math.max(1, valueOf(tree, metric))
  const kind = `${CATEGORY[node.category].label}${node.reclaim ? ` · ${node.reclaim}` : ''}`
  const isCurrent = target.join('/') === current.join('/')
  const worthTotal = insights.reduce((sum, candidate) => sum + candidate.bytes, 0)
  const largest = insights[0]?.bytes ?? 1
  const disk = state.disk

  return (
    <div
      testId="panel"
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: PANEL_WIDTH,
        flexShrink: 0,
        gap: 18,
        padding: 18,
        borderRadius: 12,
        backgroundColor: CARD,
        borderWidth: 1,
        borderColor: '#1F15300D',
        boxShadow: { offsetX: 0, offsetY: 1, blurRadius: 14, spreadRadius: 0, color: '#1F15300C' },
      }}
    >
      <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
        <Eyebrow>Selection</Eyebrow>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 3 }}>
          <div style={{ display: 'flex', alignItems: 'center', gap: 8, minWidth: 0 }}>
            <div style={{ width: 3, height: 18, borderRadius: 2, backgroundColor: CATEGORY[node.category].color, flexShrink: 0 }} />
            <text
              testId="selection-name"
              style={{ fontSize: TEXT.heading, fontWeight: 600, color: INK, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}
            >
              {node.name}
            </text>
          </div>
          <text style={{ fontSize: TEXT.caption, color: INK_2, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
            {displayPath(full)}
          </text>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
          <div style={{ display: 'flex', alignItems: 'flex-end', gap: 6 }}>
            <text testId="selection-size" style={{ fontSize: TEXT.display, fontWeight: 300, color: INK, lineHeight: "44px" }}>
              {number}
            </text>
            <text style={{ fontSize: TEXT.title + 2, color: INK_2, paddingBottom: 6 }}>{unit}</text>
          </div>
          <Bar fraction={share} color={HIGHLIGHT} />
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          <div style={{ display: 'flex', gap: 12 }}>
            <Figure label="Of scan" value={`${(share * 100).toFixed(share >= 0.1 ? 0 : 1)}%`} />
            <Figure label="Files" value={humanCount(node.files)} />
          </div>
          <div style={{ display: 'flex', gap: 12 }}>
            <Figure label="Last write" value={ago(state.scannedAt, node.modified)} />
            <Figure label="Kind" value={kind} />
          </div>
        </div>
        <div style={{ display: 'flex', gap: 8 }}>
          {node.dir && !isCurrent ? <PlainButton testId="open" label="Open" primary onClick={() => onOpen(target)} /> : null}
          <PlainButton testId="reveal" label={revealLabel} onClick={reveal} />
        </div>
        {revealError?.path === full ? (
          <text testId="reveal-error" style={{ fontSize: TEXT.caption, color: HIGHLIGHT }}>
            {`Could not open file manager: ${revealError.message}`}
          </text>
        ) : null}
      </div>

      <div style={{ height: 1, backgroundColor: HAIRLINE, flexShrink: 0 }} />

      <div style={{ display: 'flex', flexDirection: 'column', gap: 4, flexGrow: 1, minHeight: 0, overflowY: 'scroll' }}>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'space-between', paddingBottom: 4 }}>
          <Eyebrow>Worth a look</Eyebrow>
          {worthTotal > 0 ? (
            <text style={{ fontSize: TEXT.caption, fontWeight: 600, color: HIGHLIGHT }}>{humanBytes(worthTotal)}</text>
          ) : null}
        </div>
        {insights.length === 0 ? (
          <text style={{ fontSize: TEXT.caption + 1, color: INK_2 }}>Nothing obviously disposable</text>
        ) : null}
        {insights.map((candidate) => {
          const found = resolve(tree, candidate.crumbs)
          if (!found) return null
          const names = resolveChain(tree, candidate.crumbs)
            .slice(1)
            .map((entry) => entry.name)
          const active = target.join('/') === candidate.crumbs.join('/')
          const accent = CATEGORY[found.category].color
          return (
            <div
              key={candidate.crumbs.join('/')}
              testId={`insight-${names.join('/')}`}
              onClick={() => onShow(candidate.crumbs)}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 9,
                paddingLeft: 8,
                paddingRight: 8,
                paddingTop: 6,
                paddingBottom: 6,
                borderRadius: 8,
                cursor: 'pointer',
                backgroundColor: active ? '#1F15300D' : undefined,
                hover: { backgroundColor: '#1F15300A' },
              }}
            >
              <div style={{ width: 3, height: 26, borderRadius: 2, backgroundColor: accent, flexShrink: 0 }} />
              <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0, gap: 1 }}>
                <text style={{ fontSize: TEXT.body + 1, color: INK, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
                  {names.slice(-2).join('/')}
                </text>
                <text style={{ fontSize: TEXT.caption, color: INK_2, whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
                  {candidate.reason}
                </text>
              </div>
              <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'flex-end', gap: 5, width: 80, flexShrink: 0 }}>
                <text style={{ fontSize: TEXT.body + 1, color: INK }}>{humanBytes(candidate.bytes)}</text>
                <Bar fraction={candidate.bytes / largest} color={accent} height={3} />
              </div>
            </div>
          )
        })}
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 8, flexShrink: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <Eyebrow>Disk</Eyebrow>
          <text style={{ fontSize: TEXT.caption, color: INK_3 }}>{path.parse(state.root).root}</text>
        </div>
        {disk ? (
          <>
            <div style={{ display: 'flex', alignItems: 'flex-end', gap: 6 }}>
              <text style={{ fontSize: TEXT.figure, fontWeight: 300, color: INK, lineHeight: "30px" }}>
                {humanBytes(disk.available).split(' ')[0]}
              </text>
              <text style={{ fontSize: TEXT.body + 1, color: INK_2, paddingBottom: 4 }}>
                {`${humanBytes(disk.available).split(' ')[1]} free`}
              </text>
            </div>
            <Bar fraction={1 - disk.available / Math.max(1, disk.total)} color="#8A5BA8" height={6} />
            <div style={{ display: 'flex', justifyContent: 'space-between' }}>
              <text style={{ fontSize: TEXT.caption, color: INK_2 }}>{`${humanBytes(disk.total - disk.available)} used`}</text>
              <text style={{ fontSize: TEXT.caption, color: INK_2 }}>{`${humanBytes(disk.total)} total`}</text>
            </div>
          </>
        ) : (
          <text style={{ fontSize: TEXT.caption, color: INK_2 }}>Free space is not available here</text>
        )}
      </div>
    </div>
  )
}

// ── App ────────────────────────────────────────────────────────────────────

interface Nav {
  tree: TreeNode | null
  crumbs: number[]
  selected: number[] | null
  hovered: number[] | null
}

export interface DisktreeAppProps {
  store: DisktreeStore
  /** Explicit size keeps tests independent of the live window. */
  width?: number
  height?: number
}

export function DisktreeApp({ store, width, height }: DisktreeAppProps) {
  const state = useSyncExternalStore(store.subscribe, store.get)
  const windowSize = useWindowSize()
  const W = width ?? windowSize.width
  const H = height ?? windowSize.height
  const [mode, setMode] = useState<Mode>('size')
  const [apparent, setApparent] = useState(false)
  const [depth, setDepth] = useState(LAYOUT_DEFAULTS.maxDepth)
  const [storedNav, setNav] = useState<Nav>({ tree: null, crumbs: [], selected: null, hovered: null })

  const tree = state.tree
  // A new scan resets where you are: old crumbs index a different tree.
  const nav: Nav = storedNav.tree === tree ? storedNav : { tree, crumbs: [], selected: null, hovered: null }
  const update = (patch: Partial<Nav>) => setNav({ ...nav, ...patch })

  const metric: Metric = mode === 'files' ? 'files' : apparent ? 'apparent' : 'bytes'
  const area: Rect = {
    x: 0,
    y: 0,
    w: Math.max(0, W - PAD_LEFT - PANEL_GAP - PANEL_WIDTH - PAD_RIGHT),
    h: Math.max(0, H - TOP_BAR - LEGEND_ROW - KEY_BAR - PAD_BOTTOM),
  }
  const origin = { x: PAD_LEFT, y: TOP_BAR + LEGEND_ROW }
  const current = tree ? resolve(tree, nav.crumbs) : undefined

  const tiles = useMemo(
    () => (tree && current ? layout(current, nav.crumbs, area, metric, { ...LAYOUT_DEFAULTS, maxDepth: depth }) : []),
    [tree, current, nav.crumbs, area.w, area.h, metric, depth],
  )
  const decorated = useMemo(
    () => (tree ? decorate(tree, tiles, mode, metric, state.scannedAt) : []),
    [tree, tiles, mode, metric, state.scannedAt],
  )
  const insights = useMemo(() => (tree ? worthALook(tree, state.scannedAt, 8) : []), [tree, state.scannedAt])

  const tileAt = (x: number, y: number) => hit(tiles, x - origin.x, y - origin.y)
  const rectOf = (crumbs: number[] | null) =>
    crumbs ? tiles.find((tile) => !tile.others && tile.crumbs.join('/') === crumbs.join('/'))?.rect : undefined

  const open = (crumbs: number[]) => {
    const node = tree && resolve(tree, crumbs)
    if (!node) return
    if (node.dir) update({ crumbs, selected: null, hovered: null })
    else update({ crumbs: crumbs.slice(0, -1), selected: crumbs, hovered: null })
  }
  const show = (crumbs: number[]) => update({ crumbs: crumbs.slice(0, -1), selected: crumbs, hovered: null })
  const up = () => {
    if (!nav.crumbs.length) return
    update({ crumbs: nav.crumbs.slice(0, -1), selected: nav.crumbs, hovered: null })
  }
  const rescan = (includeHidden = state.includeHidden) => void store.rescan(includeHidden)

  const onMouseMove = (event: DomEvent) => {
    const tile = tileAt(event.x ?? -1, event.y ?? -1)
    const crumbs = tile && !tile.others ? tile.crumbs : null
    if ((crumbs?.join('/') ?? null) !== (nav.hovered?.join('/') ?? null)) update({ hovered: crumbs })
  }
  const onMouseDown = (event: DomEvent) => {
    if (event.button !== 0) return
    const tile = tileAt(event.x ?? -1, event.y ?? -1)
    if (!tile || tile.others) return update({ selected: null })
    // A folder opens on the first click; a file is selected inside its folder.
    open(tile.crumbs)
  }
  const onKeyDown = (event: DomEvent) => {
    switch (event.key) {
      case 'Backspace':
      case 'Escape':
        return up()
      case 'Enter':
        if (nav.selected) open(nav.selected)
        return
      case '[':
        return setDepth((value) => Math.max(1, value - 1))
      case ']':
        return setDepth((value) => Math.min(6, value + 1))
      case 't':
        return setMode((value) => (value === 'size' ? 'files' : value === 'files' ? 'age' : 'size'))
      case 'r':
        return rescan()
    }
  }

  // The panel follows the pointer, then the selection, then the view.
  const target = nav.hovered ?? nav.selected ?? nav.crumbs
  const hoveredRect = rectOf(nav.hovered)
  const selectedRect = rectOf(nav.selected)

  // Trail: everything above the scanned root is dim, the tree part is clickable.
  // A deep root keeps its last two steps, like the original's folded trail.
  const parts = state.root.split(path.sep).filter(Boolean)
  const above = parts.length > 3 ? ['…', ...parts.slice(-2)] : parts
  const inTree = tree ? resolveChain(tree, nav.crumbs).slice(1) : []

  const progressText = state.scanning
    ? `scanning · ${humanCount(state.progress.files)} entries · ${humanBytes(state.progress.bytes)}`
    : `scan ${humanCount(state.progress.files + state.progress.dirs)} entries${
        state.elapsedMs == null ? '' : ` · ${(state.elapsedMs / 1000).toFixed(1)} s`
      }`

  return (
    <div
      testId="disktree"
      autoFocus
      tabIndex={-1}
      onKeyDown={onKeyDown}
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        backgroundColor: WINDOW_TINT,
        userSelect: 'none',
      }}
    >
      {/* ── top bar ── */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          height: TOP_BAR,
          flexShrink: 0,
          gap: 16,
          paddingLeft: TRAFFIC_LIGHTS,
          paddingRight: PAD_RIGHT + 4,
        }}
      >
        <div style={{ display: 'flex', alignItems: 'center', gap: 12, flexShrink: 0 }}>
          <BackButton disabled={nav.crumbs.length === 0} onClick={up} />
          <Logo />
        </div>
        <div style={{ display: 'flex', alignItems: 'center', gap: 2, minWidth: 0, flexShrink: 1, overflow: 'hidden' }}>
          {above.map((part, index) => {
            const isRoot = index === above.length - 1
            const active = isRoot && nav.crumbs.length === 0
            return (
              <div key={`above-${index}`} style={{ display: 'flex', alignItems: 'center', gap: 2, flexShrink: 0 }}>
                <text style={{ fontSize: TEXT.title, color: INK_3, paddingLeft: 3, paddingRight: 3 }}>/</text>
                <div
                  testId={isRoot ? 'crumb-root' : undefined}
                  onClick={isRoot ? () => update({ crumbs: [], selected: null }) : undefined}
                  style={{
                    paddingLeft: 7,
                    paddingRight: 7,
                    height: 26,
                    display: 'flex',
                    alignItems: 'center',
                    borderRadius: 7,
                    cursor: isRoot ? 'pointer' : 'default',
                    backgroundColor: active ? CONTROL_SELECTED : undefined,
                    hover: isRoot && !active ? { backgroundColor: CONTROL } : {},
                    active: isRoot ? { backgroundColor: CONTROL_PRESSED } : {},
                  }}
                >
                  <text style={{ fontSize: TEXT.title, color: isRoot ? INK : INK_3, fontWeight: active ? 600 : 400 }}>
                    {part}
                  </text>
                </div>
              </div>
            )
          })}
          {inTree.map((node, index) => {
            const crumbs = nav.crumbs.slice(0, index + 1)
            const active = index === inTree.length - 1
            return (
              <div key={`tree-${index}`} style={{ display: 'flex', alignItems: 'center', gap: 2, flexShrink: 0 }}>
                <text style={{ fontSize: TEXT.title, color: INK_3, paddingLeft: 3, paddingRight: 3 }}>/</text>
                <div
                  testId={`crumb-${index}`}
                  onClick={() => update({ crumbs, selected: null })}
                  style={{
                    paddingLeft: 7,
                    paddingRight: 7,
                    height: 26,
                    display: 'flex',
                    alignItems: 'center',
                    borderRadius: 7,
                    cursor: 'pointer',
                    backgroundColor: active ? CONTROL_SELECTED : undefined,
                    hover: active ? {} : { backgroundColor: CONTROL },
                    active: { backgroundColor: CONTROL_PRESSED },
                  }}
                >
                  <text style={{ fontSize: TEXT.title, color: INK, fontWeight: active ? 600 : 400 }}>{node.name}</text>
                </div>
              </div>
            )
          })}
        </div>
        <div style={{ flexGrow: 1 }} />
        <div style={{ display: 'flex', alignItems: 'center', gap: 14, flexShrink: 0 }}>
          <Segmented value={mode} onChange={setMode} />
          <Checkbox
            testId="hidden"
            label="Hidden files"
            checked={state.includeHidden}
            onToggle={() => rescan(!state.includeHidden)}
          />
          <Checkbox testId="apparent" label="Apparent size" checked={apparent} onToggle={() => setApparent(!apparent)} />
          <div
            style={{
              display: 'flex',
              alignItems: 'center',
              height: 30,
              paddingLeft: 11,
              paddingRight: 3,
              gap: 4,
              borderRadius: 9,
              backgroundColor: CONTROL,
              boxShadow: CONTROL_SHADOW,
            }}
          >
            <text style={{ fontSize: TEXT.body + 1, color: INK, paddingRight: 4 }}>{`Depth ${depth}`}</text>
            {(
              [
                ['depth-less', '−', -1],
                ['depth-more', '+', 1],
              ] as const
            ).map(([id, glyph, step]) => {
              const disabled = step < 0 ? depth <= 1 : depth >= 6
              return (
                <div
                  key={id}
                  testId={id}
                  onClick={disabled ? undefined : () => setDepth(depth + step)}
                  style={{
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center',
                    width: 24,
                    height: 24,
                    borderRadius: 6,
                    cursor: disabled ? 'default' : 'pointer',
                    hover: disabled ? {} : { backgroundColor: '#1F15300F' },
                    active: disabled ? {} : { backgroundColor: CONTROL_PRESSED },
                  }}
                >
                  <text style={{ fontSize: TEXT.title + 1, color: disabled ? INK_3 : INK }}>{glyph}</text>
                </div>
              )
            })}
          </div>
        </div>
      </div>

      {/* ── body: mosaic on the frosted window, panel on a white card ── */}
      <div
        style={{
          display: 'flex',
          flexGrow: 1,
          minHeight: 0,
          gap: PANEL_GAP,
          paddingLeft: PAD_LEFT,
          paddingRight: PAD_RIGHT,
          paddingBottom: PAD_BOTTOM,
        }}
      >
        <div style={{ display: 'flex', flexDirection: 'column', flexGrow: 1, minWidth: 0 }}>
          <div style={{ display: 'flex', alignItems: 'center', height: LEGEND_ROW, gap: 16, flexShrink: 0, overflow: 'hidden' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 6, flexShrink: 0 }}>
              <text testId="scan-total" style={{ fontSize: TEXT.body, fontWeight: 600, color: INK }}>
                {humanBytes(tree?.bytes ?? state.progress.bytes)}
              </text>
              <text style={{ fontSize: TEXT.body, color: INK_2 }}>
                {`· ${humanCount(tree?.files ?? state.progress.files)} files · ${humanCount(tree?.dir ? tree.dirs : state.progress.dirs)} dirs`}
              </text>
              {state.progress.errors > 0 ? (
                <text style={{ fontSize: TEXT.body, color: HIGHLIGHT }}>{`· ${humanCount(state.progress.errors)} unreadable`}</text>
              ) : null}
            </div>
            <div style={{ flexGrow: 1 }} />
            <div style={{ display: 'flex', alignItems: 'center', gap: 12, minWidth: 0, overflow: 'hidden' }}>
              <div style={{ display: 'flex', alignItems: 'center', gap: 5, flexShrink: 0 }}>
                <div style={{ width: 10, height: 10, borderRadius: 3, backgroundColor: alpha(CATEGORY.other.color, 0.5), overflow: 'hidden' }}>
                  <svg source={hatchSvg(10, 10)} style={{ width: 10, height: 10, color: '#1F153070' }} />
                </div>
                <text style={{ fontSize: TEXT.caption, color: INK_2 }}>Reclaimable</text>
              </div>
              {(mode === 'age'
                ? AGE_BUCKETS.map(([, label], index) => [AGE_COLORS[index]!, label] as const)
                : LEGEND.map((category) => [CATEGORY[category].color, CATEGORY[category].label] as const)
              ).map(([color, label]) => (
                <div key={label} style={{ display: 'flex', alignItems: 'center', gap: 5, flexShrink: 0 }}>
                  <Swatch color={color} />
                  <text style={{ fontSize: TEXT.caption, color: INK_2 }}>{label}</text>
                </div>
              ))}
            </div>
          </div>

          <div
            testId="treemap"
            onMouseMove={onMouseMove}
            onMouseDown={onMouseDown}
            onMouseLeave={() => nav.hovered && update({ hovered: null })}
            style={{ position: 'relative', width: area.w, height: area.h, flexShrink: 0 }}
          >
            {tree ? (
              <>
                <TileLayer tiles={decorated} />
                {hoveredRect ? <Outline rect={hoveredRect} width={1} color="#1F153080" /> : null}
                {selectedRect ? <Outline rect={selectedRect} width={2} color={HIGHLIGHT} /> : null}
              </>
            ) : (
              <Scanning state={state} />
            )}
          </div>

          <div style={{ display: 'flex', alignItems: 'center', height: KEY_BAR, gap: 16, flexShrink: 0, overflow: 'hidden' }}>
            <Keycap keys="click" label="open" />
            <Keycap keys="⌫" label="up" />
            <Keycap keys="[ ]" label="depth" />
            <Keycap keys="t" label="mode" />
            <Keycap keys="r" label="rescan" />
            <div style={{ flexGrow: 1 }} />
            <text testId="scan-status" style={{ fontSize: TEXT.caption, color: INK_2, flexShrink: 0 }}>
              {progressText}
            </text>
          </div>
        </div>

        {tree ? (
          <SidePanel
            state={state}
            tree={tree}
            target={target}
            current={nav.crumbs}
            metric={metric}
            insights={insights}
            onOpen={open}
            onShow={show}
          />
        ) : (
          <div style={{ width: PANEL_WIDTH, flexShrink: 0, borderRadius: 12, backgroundColor: CARD }} />
        )}
      </div>
    </div>
  )
}

function Scanning({ state }: { state: DisktreeState }) {
  const { progress } = state
  // A whole volume has a known total: the space in use. A folder does not,
  // so its bar only approaches the end as the count grows.
  const disk = state.root === path.parse(state.root).root ? diskSpace(state.root) : null
  const estimate = disk
    ? progress.bytes / Math.max(1, disk.total - disk.available)
    : 1 - Math.exp(-progress.files / 150_000)
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        alignItems: 'center',
        justifyContent: 'center',
        gap: 16,
        width: '100%',
        height: '100%',
      }}
    >
      <text style={{ fontSize: TEXT.title + 2, fontWeight: 600, color: INK }}>
        {state.error ? `Could not read ${displayPath(state.root)}` : `Reading ${displayPath(state.root)}`}
      </text>
      <div style={{ display: 'flex', gap: 28 }}>
        {(
          [
            ['files', humanCount(progress.files)],
            ['directories', humanCount(progress.dirs)],
            ['measured', humanBytes(progress.bytes)],
            ['unreadable', humanCount(progress.errors)],
          ] as const
        ).map(([label, value]) => (
          <div key={label} style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 2 }}>
            <text style={{ fontSize: TEXT.figure, fontWeight: 300, color: INK }}>{value}</text>
            <text style={{ fontSize: TEXT.caption, color: INK_2 }}>{label}</text>
          </div>
        ))}
      </div>
      <div style={{ width: 420 }}>
        <Bar fraction={estimate} color={HIGHLIGHT} />
      </div>
      {state.error ? <text style={{ fontSize: TEXT.body, color: HIGHLIGHT }}>{state.error}</text> : null}
    </div>
  )
}
