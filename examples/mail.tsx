/**
 * Discord-style mail client. Channel tree, timeline, and reading pane.
 *
 * Run: cd examples && bun --hot mail.tsx
 */

import { useMemo, useState, type ReactNode } from 'react'
import { applyMacCpuThrottleFromEnv, render } from '@gpuix/react'

import iconSidebar from './assets/icons/panel-left.svg' with { type: 'text' }
import iconArrowLeft from './assets/icons/arrow-left.svg' with { type: 'text' }
import iconArrowRight from './assets/icons/arrow-right.svg' with { type: 'text' }
import iconCompose from './assets/icons/compose.svg' with { type: 'text' }
import iconChevronDown from './assets/icons/chevron-down.svg' with { type: 'text' }
import iconFilter from './assets/icons/list-filter.svg' with { type: 'text' }
import iconZap from './assets/icons/zap.svg' with { type: 'text' }
import iconSparkle from './assets/icons/sparkle.svg' with { type: 'text' }
import iconList from './assets/icons/list.svg' with { type: 'text' }

const C = {
  sidebar: '#141416',
  list: '#141416',
  reading: '#161618',
  raised: '#1A1A1D',
  selected: '#2A2A2F',
  pill: '#1A1A1D',
  overlay: '#FFFFFF0F',
  overlayStrong: '#FFFFFF18',
  border: '#242428',
  borderStrong: '#3A3A40',
  divider: '#242428',
  text: '#ECECEE',
  secondary: '#C6C6CA',
  muted: '#8A8A90',
  ghost: '#55555E',
  face: '#3A3A40',
  unread: '#3B82F6',
  mention: '#ED4245',
  tree: '#3F3F46',
}

const FONT = typeof window === 'undefined' ? 'Helvetica' : 'IBM Plex Sans'
const SIDEBAR_WIDTH = 214
const LIST_WIDTH = 328
const TITLEBAR_HEIGHT = 48
const TRAFFIC_LIGHT_CLEARANCE =
  typeof process !== 'undefined' && process.platform === 'darwin' ? 86 : 12

const SVG = {
  clock: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M12 6v6l4 2"/></svg>`,
  x: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`,
  sort: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m3 16 4 4 4-4"/><path d="M7 20V4"/><path d="m21 8-4-4-4 4"/><path d="M17 4v16"/></svg>`,
  star: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11.525 2.295a.53.53 0 0 1 .95 0l2.31 4.679a2.123 2.123 0 0 0 1.595 1.16l5.166.756a.53.53 0 0 1 .294.904l-3.736 3.638a2.123 2.123 0 0 0-.611 1.878l.882 5.14a.53.53 0 0 1-.771.56l-4.618-2.428a2.122 2.122 0 0 0-1.973 0L6.396 21.01a.53.53 0 0 1-.77-.56l.881-5.139a2.122 2.122 0 0 0-.611-1.879L2.16 9.795a.53.53 0 0 1 .294-.906l5.165-.755a2.122 2.122 0 0 0 1.597-1.16z"/></svg>`,
  block: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="m4.9 4.9 14.2 14.2"/></svg>`,
  trash: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 6h18"/><path d="M8 6V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/><path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6"/></svg>`,
  archive: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="20" height="5" x="2" y="3" rx="1"/><path d="M4 8v11a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8"/><path d="M10 12h4"/></svg>`,
  user: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="8" r="4"/><path d="M4 20a8 8 0 0 1 16 0"/></svg>`,
  tag: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12.586 2.586A2 2 0 0 0 11.172 2H4a2 2 0 0 0-2 2v7.172a2 2 0 0 0 .586 1.414l8.704 8.704a2.426 2.426 0 0 0 3.42 0l6.58-6.58a2.426 2.426 0 0 0 0-3.42z"/><circle cx="7.5" cy="7.5" r=".5" fill="currentColor"/></svg>`,
  megaphone: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M11 6a13 13 0 0 0 8.4-2.8A1 1 0 0 1 21 4v12a1 1 0 0 1-1.6.8A13 13 0 0 0 11 14"/><path d="M11 6v8"/><path d="M11 14H8a5 5 0 0 1 0-8h3"/></svg>`,
  message: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M7.9 20A9 9 0 1 0 4 16.1L2 22z"/></svg>`,
  more: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="5" cy="12" r="1"/><circle cx="12" cy="12" r="1"/><circle cx="19" cy="12" r="1"/></svg>`,
  hash: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M4 9h16"/><path d="M4 15h16"/><path d="M10 3 8 21"/><path d="M16 3l-2 18"/></svg>`,
  puzzle: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 1.233-.706 1.704l-1.611 1.611a.98.98 0 0 1-.837.276c-.47-.07-.802-.48-.968-.925a2.54 2.54 0 1 0-3.214 3.214c.446.166.855.497.925.968a.979.979 0 0 1-.276.837l-1.61 1.61a2.404 2.404 0 0 1-1.705.707 2.402 2.402 0 0 1-1.704-.706l-1.568-1.568a1.026 1.026 0 0 0-.877-.29c-.493.074-.84.504-1.02.968a2.54 2.54 0 1 1-3.194-3.194c.463-.18.894-.527.967-1.02a1.026 1.026 0 0 0-.289-.877l-1.568-1.568A2.402 2.402 0 0 1 1.998 12c0-.617.236-1.234.706-1.704L4.23 8.77c.24-.24.581-.353.917-.303.515.077.877.528 1.073 1.01a2.54 2.54 0 1 0 3.188-3.188c-.483-.196-.933-.557-1.01-1.073a1.026 1.026 0 0 1 .303-.917l1.525-1.525A2.402 2.402 0 0 1 12 1.998c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.074.84-.504 1.02-.968a2.54 2.54 0 1 1 3.194 3.194c-.463.18-.894.527-.967 1.02z"/></svg>`,
  at: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="4"/><path d="M16 8v5a3 3 0 0 0 6 0v-1a10 10 0 1 0-4 8"/></svg>`,
  paperclip: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m21.44 11.05-9.19 9.19a6 6 0 0 1-8.49-8.49l8.57-8.57A4 4 0 1 1 18 8.84l-8.59 8.57a2 2 0 0 1-2.83-2.83l8.49-8.48"/></svg>`,
  mic: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2a3 3 0 0 0-3 3v7a3 3 0 0 0 6 0V5a3 3 0 0 0-3-3Z"/><path d="M19 10v2a7 7 0 0 1-14 0v-2"/><line x1="12" x2="12" y1="19" y2="22"/></svg>`,
  maximize: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="m21 3-7 7"/><path d="m3 21 7-7"/><path d="M9 21H3v-6"/></svg>`,
  minimize: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m14 10 7-7"/><path d="M20 10h-6V4"/><path d="m3 21 7-7"/><path d="M4 14h6v6"/></svg>`,
  snooze: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"/><path d="M8 12h8"/></svg>`,
  framer: `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24"><path fill="currentColor" d="M4 0h16v8h-8zm0 8h8l8 8H4zm8 8h8v8z"/></svg>`,
} as const

const ICONS = {
  sidebar: iconSidebar,
  arrowLeft: iconArrowLeft,
  arrowRight: iconArrowRight,
  compose: iconCompose,
  chevronDown: iconChevronDown,
  filter: iconFilter,
  zap: iconZap,
  sparkle: iconSparkle,
  list: iconList,
  ...SVG,
} as const

type IconName = keyof typeof ICONS

function Icon({ name, size = 14, color }: { name: IconName; size?: number; color: string }) {
  return <svg source={ICONS[name]} style={{ width: size, height: size, flexShrink: 0, color }} />
}

function IconButton({
  icon,
  onClick,
  size = 14,
  dimmed,
  pad = 26,
  testId,
}: {
  icon: IconName
  onClick?: () => void
  size?: number
  dimmed?: boolean
  pad?: number
  testId?: string
}) {
  return (
    <div
      testId={testId}
      onClick={onClick}
      style={{
        width: pad,
        height: pad,
        flexShrink: 0,
        borderRadius: pad / 2,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        appRegion: 'no-drag',
        cursor: 'pointer',
        opacity: dimmed ? 0.35 : 1,
        hover: dimmed ? undefined : { backgroundColor: C.overlay },
        active: dimmed ? undefined : { backgroundColor: C.overlayStrong },
      }}
    >
      <Icon name={icon} size={size} color={C.muted} />
    </div>
  )
}

function Pill({ children }: { children: ReactNode }) {
  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        height: 28,
        paddingLeft: 2,
        paddingRight: 2,
        flexShrink: 0,
        appRegion: 'no-drag',
        borderRadius: 14,
        backgroundColor: C.pill,
        borderWidth: 1,
        borderColor: C.border,
        borderStyle: 'solid',
      }}
    >
      {children}
    </div>
  )
}

type FaceSpec = { src?: string; letter?: string; logo?: boolean }

function Face({ src, letter, logo, size }: FaceSpec & { size: number }) {
  return (
    <div
      style={{
        width: size,
        height: size,
        flexShrink: 0,
        borderRadius: logo ? Math.max(4, size * 0.22) : size / 2,
        backgroundColor: logo ? '#111111' : C.face,
        overflow: 'hidden',
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
      }}
    >
      {src ? (
        <img
          src={src}
          objectFit="cover"
          style={{
            width: size,
            height: size,
            borderRadius: logo ? Math.max(4, size * 0.22) : size / 2,
          }}
        />
      ) : logo ? (
        <Icon name="framer" size={size * 0.55} color="#FFFFFF" />
      ) : (
        <text style={{ fontSize: size * 0.38, fontWeight: 600, color: C.text, fontFamily: FONT }}>
          {letter}
        </text>
      )}
    </div>
  )
}

function FaceStack({ faces }: { faces: FaceSpec[] }) {
  if (faces.length === 1) {
    return (
      <div style={{ width: 34, height: 34, flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        <Face {...faces[0]!} size={faces[0]!.logo ? 34 : 32} />
      </div>
    )
  }
  return (
    <div
      style={{
        width: 34,
        height: 34,
        flexShrink: 0,
        display: 'flex',
        flexDirection: 'row',
        flexWrap: 'wrap',
        gap: 1.5,
      }}
    >
      {faces.slice(0, 4).map((face, index) => (
        <Face key={index} {...face} size={16} />
      ))}
    </div>
  )
}

function MentionBadge({ count }: { count: number }) {
  if (count <= 0) return null
  return (
    <div
      style={{
        height: 13,
        minWidth: 13,
        paddingLeft: 4,
        paddingRight: 4,
        borderRadius: 7,
        backgroundColor: C.mention,
        display: 'flex',
        alignItems: 'center',
        justifyContent: 'center',
        flexShrink: 0,
      }}
    >
      <text style={{ fontSize: 9, fontWeight: 700, color: '#FFFFFF', fontFamily: FONT }}>{count}</text>
    </div>
  )
}

type Message = {
  id: string
  from: string
  to?: string
  face: FaceSpec
  time: string
  day?: string
  body: string
  image?: string
}

type MailThread = {
  id: string
  channelId: string
  senders: string
  subject: string
  snippet: string
  date: string
  lastReplyAt: number
  unread: boolean
  mentionCount?: number
  faces: FaceSpec[]
  messages: Message[]
}

type Channel = {
  id: string
  label: string
  icon: IconName
}

const maraFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1494790108377-be9c29b29330?w=128&h=128&fit=crop' }
const noraFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1544005313-94ddf0286df2?w=128&h=128&fit=crop' }
const julesFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1500648767791-00dcc994a43e?w=128&h=128&fit=crop' }
const kenjiFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1507003211169-0a1dd7228f2d?w=128&h=128&fit=crop' }
const leaFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1524504388940-b1c1722653e1?w=128&h=128&fit=crop' }
const miraFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1438761681033-6461ffad8d80?w=128&h=128&fit=crop' }
const atlasFace: FaceSpec = { src: 'https://images.unsplash.com/photo-1500530855697-b586d89ba3ee?w=128&h=128&fit=crop' }
const bannerSrc = 'https://images.unsplash.com/photo-1618005182384-a83a8bd57fbe?w=1200&q=80'

const CHANNELS: Channel[] = [
  { id: 'primary', label: 'Primary', icon: 'user' },
  { id: 'promotions', label: 'Promotions', icon: 'tag' },
  { id: 'social', label: 'Social', icon: 'megaphone' },
  { id: 'updates', label: 'Updates', icon: 'message' },
  { id: 'forums', label: 'Forums', icon: 'list' },
  { id: 'notifications', label: 'Notifications', icon: 'sparkle' },
]

const THREADS: MailThread[] = [
  {
    id: 'nora',
    channelId: 'primary',
    senders: 'Nora Hale',
    subject: 'Desk notes',
    snippet: 'I parked the draft in the shared folder. Ping me when you want a pass.',
    date: 'Today',
    lastReplyAt: 1756900000000,
    unread: true,
    mentionCount: 1,
    faces: [noraFace],
    messages: [
      { id: 'no-1', from: 'Nora Hale', face: noraFace, time: 'Today', body: 'I parked the draft in the shared folder. Ping me when you want a pass.' },
    ],
  },
  {
    id: 'jules',
    channelId: 'primary',
    senders: 'Jules Park',
    subject: 'Re: Thursday',
    snippet: 'Re: Thursday',
    date: '12/08/2024',
    lastReplyAt: 1723420800000,
    unread: true,
    mentionCount: 1,
    faces: [julesFace],
    messages: [
      { id: 'ju-1', from: 'Jules Park', face: julesFace, time: '12/08/2024', body: 'Can we move Thursday to 4pm?' },
      { id: 'ju-2', from: 'You', face: maraFace, time: '12/08/2024', day: '13 Aug 2024', body: 'Re: Thursday' },
    ],
  },
  {
    id: 'kenji',
    channelId: 'primary',
    senders: 'Kenji Ito',
    subject: 'Morning',
    snippet: 'Morning',
    date: 'Yesterday',
    lastReplyAt: 1723334400000,
    unread: false,
    faces: [kenjiFace],
    messages: [      { id: 'ke-1', from: 'Kenji Ito', face: kenjiFace, time: 'Yesterday', body: 'Morning' }],
  },
  {
    id: 'atlas-weekly',
    channelId: 'primary',
    senders: 'Atlas, Mira, Kenji...',
    subject: 'Atlas Weekly',
    snippet: 'Atlas cut idle spend and shipped the routing board',
    date: 'Jul 30',
    lastReplyAt: 1722297600000,
    unread: true,
    mentionCount: 1,
    faces: [atlasFace, miraFace, kenjiFace, { letter: '8' }],
    messages: [
      {
        id: 'aw-1',
        from: 'Lea from Atlas',
        to: 'Mara',
        face: leaFace,
        time: '4w',
        body: 'Lea sent the weekly recap: routing board is live, idle spend is down 18%, and the studio sale runs through August 31.',
      },
      {
        id: 'aw-2',
        from: 'Mira Cole',
        to: 'Mara',
        face: miraFace,
        time: '4w',
        body: 'Seat usage dropped after the routing change. The full recap is in the thread.',
      },
      {
        id: 'aw-3',
        from: 'Lea from Atlas',
        to: 'Mara',
        face: leaFace,
        time: '4w',
        day: '2 Aug',
        body: 'Northlight 4 and Harbor 2 are in the studio now. We also shipped a live grain shader, form spam filters, and nested folders on design pages.\n\nNorthlight 4\nThis is the most reliable model we have run in-house. First drafts keep style, reuse parts, and land closer to the brief. In our bench it hits 81%, ahead of the previous run at 74%.',
        image: bannerSrc,
      },
    ],
  },
  {
    id: 'lea-july',
    channelId: 'primary',
    senders: 'Lea From Atlas, Mi...',
    subject: 'Atlas Weekly',
    snippet: 'New studio models and design notes from Atlas',
    date: 'Jul 8',
    lastReplyAt: 1720396800000,
    unread: true,
    faces: [leaFace, miraFace, { letter: 'T' }, { letter: '8' }],
    messages: [
      { id: 'lj-1', from: 'Lea from Atlas', face: leaFace, time: 'Jul 8', body: 'New studio models and design notes from Atlas. Northlight 4, Harbor 2, and the grain shader.' },
      { id: 'lj-2', from: 'Mira Cole', face: miraFace, time: 'Jul 8', body: 'Start with the grain shader. It is the one worth a demo.' },
    ],
  },
  {
    id: 'lighthouse',
    channelId: 'promotions',
    senders: 'Atlas, Lighthouse, ...',
    subject: 'Studio drop',
    snippet: 'Atlas is hosting a short studio drop next week...',
    date: 'Aug 5',
    lastReplyAt: 1722816000000,
    unread: false,
    faces: [atlasFace, { letter: 'L' }, { letter: 'T' }, { letter: '8' }],
    messages: [{ id: 'lh-1', from: 'Atlas', face: atlasFace, time: 'Aug 5', body: 'Atlas is hosting a short studio drop next week.' }],
  },
  {
    id: 'welcome',
    channelId: 'promotions',
    senders: 'Atlas',
    subject: 'Welcome to Atlas',
    snippet: 'Welcome to Atlas. Here is how to get started',
    date: '01/10/2023',
    lastReplyAt: 1696118400000,
    unread: false,
    faces: [atlasFace],
    messages: [{ id: 'we-1', from: 'Atlas', face: atlasFace, time: '01/10/2023', body: 'Welcome to Atlas. Here is how to get started.' }],
  },
  {
    id: 'invite',
    channelId: 'social',
    senders: 'Mira, Atlas',
    subject: 'Workspace invite',
    snippet: 'You were invited to the Harbor workspace in Atlas',
    date: '03/11/2023',
    lastReplyAt: 1698969600000,
    unread: false,
    faces: [miraFace, atlasFace],
    messages: [{ id: 'iv-1', from: 'Mira Cole', face: miraFace, time: '03/11/2023', body: 'You were invited to the Harbor workspace in Atlas.' }],
  },
  {
    id: 'year-review',
    channelId: 'updates',
    senders: 'Atlas, Northlight...',
    subject: '2024 year in review',
    snippet: 'Atlas 2024 year in review and highlights',
    date: '15/01/2025',
    lastReplyAt: 1736899200000,
    unread: false,
    faces: [atlasFace, { letter: 'N' }, { letter: 'T' }, { letter: '9' }],
    messages: [{ id: 'yr-1', from: 'Atlas', face: atlasFace, time: '15/01/2025', body: 'Atlas 2024 year in review and highlights.' }],
  },
  {
    id: 'lea-30',
    channelId: 'updates',
    senders: 'Lea From Atlas...',
    subject: 'Atlas weekly update',
    snippet: 'Atlas weekly update brings shared boards and design notes...',
    date: 'Aug 30',
    lastReplyAt: 1724976000000,
    unread: true,
    faces: [leaFace, { letter: 'J' }, { letter: 'J' }, { letter: '8' }],
    messages: [{ id: 'la-1', from: 'Lea from Atlas', face: leaFace, time: 'Aug 30', body: 'Atlas weekly update brings shared boards and design notes.' }],
  },
  {
    id: 'lea-29',
    channelId: 'updates',
    senders: 'Lea From Atlas, No...',
    subject: 'Atlas weekly notes',
    snippet: 'Atlas weekly notes cover boards and new design tools',
    date: 'Aug 29',
    lastReplyAt: 1724889600000,
    unread: false,
    faces: [kenjiFace, leaFace],
    messages: [{ id: 'lb-1', from: 'Lea from Atlas', face: leaFace, time: 'Aug 29', body: 'Atlas weekly notes cover boards and new design tools.' }],
  },
  {
    id: 'beta',
    channelId: 'updates',
    senders: 'Atlas Preview',
    subject: 'Preview access',
    snippet: 'You are on the Atlas preview list. Here is what ships next',
    date: '14/07/2023',
    lastReplyAt: 1689292800000,
    unread: false,
    faces: [atlasFace],
    messages: [{ id: 'be-1', from: 'Atlas Preview', face: atlasFace, time: '14/07/2023', body: 'You are on the Atlas preview list. Here is what ships next.' }],
  },
  {
    id: 'harbor',
    channelId: 'forums',
    senders: 'Atlas',
    subject: 'Notes on Harbor',
    snippet: 'Notes on Harbor project parts in Atlas',
    date: '04/05/2025',
    lastReplyAt: 1746316800000,
    unread: true,
    faces: [atlasFace],
    messages: [
      { id: 'ha-1', from: 'Atlas', face: atlasFace, time: '04/05/2025', body: 'Notes on Harbor project parts in Atlas.' },
      { id: 'ha-2', from: 'Jules Park', face: julesFace, time: '04/05/2025', body: 'Noted. I will check the part set tomorrow.' },
    ],
  },
  {
    id: 'parts',
    channelId: 'forums',
    senders: 'Lea From Atlas',
    subject: 'Part library',
    snippet: 'New part library updates for the Harbor project',
    date: '02/08/2023',
    lastReplyAt: 1690934400000,
    unread: false,
    faces: [leaFace],
    messages: [{ id: 'pa-1', from: 'Lea from Atlas', face: leaFace, time: '02/08/2023', body: 'New part library updates for the Harbor project.' }],
  },
  {
    id: 'invoice',
    channelId: 'notifications',
    senders: 'Atlas Billing',
    subject: 'December invoice',
    snippet: 'Your Atlas invoice for December is ready',
    date: '12/12/2023',
    lastReplyAt: 1702339200000,
    unread: true,
    faces: [atlasFace],
    messages: [{ id: 'in-1', from: 'Atlas Billing', face: atlasFace, time: '12/12/2023', body: 'Your Atlas invoice for December is ready.' }],
  },
  {
    id: 'sites',
    channelId: 'notifications',
    senders: 'Atlas, Sites',
    subject: 'Site published',
    snippet: 'Your published site is live. Share it with your team',
    date: '18/09/2023',
    lastReplyAt: 1694995200000,
    unread: false,
    faces: [atlasFace, { letter: 'S' }],
    messages: [{ id: 'si-1', from: 'Atlas', face: atlasFace, time: '18/09/2023', body: 'Your published site is live. Share it with your team.' }],
  },
  {
    id: 'security',
    channelId: 'notifications',
    senders: 'Atlas Security',
    subject: 'New login',
    snippet: 'A new login was detected on your Atlas account',
    date: '09/06/2023',
    lastReplyAt: 1686268800000,
    unread: true,
    faces: [{ letter: 'S' }],
    messages: [{ id: 'se-1', from: 'Atlas Security', face: { letter: 'S' }, time: '09/06/2023', body: 'A new login was detected on your Atlas account.' }],
  },
]

function messageDay(thread: MailThread, message: Message) {
  return message.day ?? thread.date ?? 'Today'
}

function ChannelRow({
  channel,
  active,
  unread,
  mentionCount,
  onSelect,
}: {
  channel: Channel
  active: boolean
  unread: boolean
  mentionCount: number
  onSelect: () => void
}) {
  const color = active || unread ? C.text : C.muted
  return (
    <div
      testId={`channel-${channel.id}`}
      onClick={onSelect}
      style={{
        height: 26,
        paddingLeft: 8,
        paddingRight: 8,
        gap: 6,
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        borderRadius: 6,
        cursor: 'pointer',
        backgroundColor: active ? C.selected : undefined,
        hover: { backgroundColor: C.overlay },
      }}
    >
      <Icon name={channel.icon} size={13} color={color} />
      <text
        style={{
          flexGrow: 1,
          minWidth: 0,
          fontSize: 13,
          fontWeight: unread ? 600 : 400,
          color,
          fontFamily: FONT,
          whiteSpace: 'nowrap',
          textOverflow: 'ellipsis',
        }}
      >
        {channel.label}
      </text>
      <MentionBadge count={mentionCount} />
    </div>
  )
}

function SidebarThreadRow({
  thread,
  selected,
  last,
  onSelect,
}: {
  thread: MailThread
  selected: boolean
  last: boolean
  onSelect: () => void
}) {
  const color = selected || thread.unread ? C.text : C.muted
  return (
    <div
      testId={`nav-thread-${thread.id}`}
      onClick={onSelect}
      style={{
        position: 'relative',
        height: 26,
        paddingLeft: 30,
        paddingRight: 8,
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        borderRadius: 6,
        cursor: 'pointer',
        backgroundColor: selected ? C.selected : undefined,
        hover: { backgroundColor: C.overlay },
      }}
    >
      <div
        style={{
          position: 'absolute',
          left: 18,
          top: 0,
          width: 1,
          height: last ? 13 : 26,
          backgroundColor: C.tree,
        }}
      />
      <div
        style={{
          position: 'absolute',
          left: 18,
          top: 12,
          width: 8,
          height: 1,
          backgroundColor: C.tree,
        }}
      />
      <text
        style={{
          flexGrow: 1,
          minWidth: 0,
          fontSize: 13,
          fontWeight: thread.unread ? 600 : 400,
          color,
          fontFamily: FONT,
          whiteSpace: 'nowrap',
          textOverflow: 'ellipsis',
        }}
      >
        {thread.senders}
      </text>
      <MentionBadge count={thread.mentionCount ?? 0} />
    </div>
  )
}

function TimelineRow({
  thread,
  selected,
  onSelect,
}: {
  thread: MailThread
  selected: boolean
  onSelect: () => void
}) {
  return (
    <div
      testId={`thread-${thread.id}`}
      onClick={onSelect}
      style={{
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
        gap: 9,
        paddingLeft: 9,
        paddingRight: 9,
        paddingTop: 10,
        paddingBottom: 10,
        borderRadius: 11,
        cursor: 'pointer',
        backgroundColor: selected ? C.selected : undefined,
        hover: { backgroundColor: C.overlay },
      }}
    >
      <div style={{ width: 7, height: 34, flexShrink: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
        {thread.unread ? (
          <div style={{ width: 7, height: 7, borderRadius: 4, backgroundColor: C.unread }} />
        ) : (
          <div style={{ width: 7, height: 7 }} />
        )}
      </div>
      <FaceStack faces={thread.faces} />
      <div style={{ flexGrow: 1, minWidth: 0, overflow: 'hidden', gap: 2 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 6 }}>
          <text
            style={{
              flexGrow: 1,
              minWidth: 0,
              fontSize: 13,
              fontWeight: 600,
              color: C.text,
              fontFamily: FONT,
              whiteSpace: 'nowrap',
              textOverflow: 'ellipsis',
            }}
          >
            {thread.senders}
          </text>
          <text style={{ fontSize: 12, color: C.muted, fontFamily: FONT, flexShrink: 0 }}>
            {thread.date}
          </text>
        </div>
        <text
          style={{
            fontSize: 12.5,
            lineHeight: "16px",
            color: C.muted,
            fontFamily: FONT,
            whiteSpace: 'nowrap',
            textOverflow: 'ellipsis',
          }}
        >
          {thread.snippet}
        </text>
      </div>
    </div>
  )
}

function DayDivider({ label }: { label: string }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 12, paddingTop: 12, paddingBottom: 12 }}>
      <div style={{ flexGrow: 1, height: 1, backgroundColor: C.divider }} />
      <div
        style={{
          paddingLeft: 10,
          paddingRight: 10,
          paddingTop: 2,
          paddingBottom: 2,
          borderRadius: 12,
          borderWidth: 1,
          borderColor: C.border,
          borderStyle: 'solid',
        }}
      >
        <text style={{ fontSize: 12, color: C.muted, fontFamily: FONT }}>{label}</text>
      </div>
      <div style={{ flexGrow: 1, height: 1, backgroundColor: C.divider }} />
    </div>
  )
}

function DirectHeader({ thread }: { thread: MailThread }) {
  const person = thread.faces[0]!
  return (
    <div style={{ paddingTop: 8, paddingBottom: 16, gap: 16 }}>
      <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 12 }}>
        <Face {...person} size={72} />
        <div style={{ minWidth: 0, gap: 2 }}>
          <text style={{ fontSize: 18, fontWeight: 600, color: C.text, fontFamily: FONT }}>{thread.senders}</text>
          <text style={{ fontSize: 13, color: C.muted, fontFamily: FONT }}>{thread.subject}</text>
        </div>
      </div>
      <text style={{ fontSize: 13.5, lineHeight: "20px", color: C.secondary, fontFamily: FONT }}>
        This conversation is only between you and {thread.senders}.
      </text>
    </div>
  )
}

function MessageRow({ message }: { message: Message }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'flex-start', gap: 10, paddingTop: 12, paddingBottom: 12 }}>
      <Face {...message.face} size={28} />
      <div style={{ flexGrow: 1, minWidth: 0, gap: 2 }}>
        <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'baseline', gap: 8 }}>
          <text style={{ fontSize: 13.5, fontWeight: 600, color: C.text, fontFamily: FONT }}>{message.from}</text>
          {message.time ? (
            <text style={{ fontSize: 12.5, color: C.muted, fontFamily: FONT }}>{message.time}</text>
          ) : null}
        </div>
        {message.to ? (
          <text style={{ fontSize: 12.5, color: C.muted, fontFamily: FONT, whiteSpace: 'nowrap' }}>
            {`To ${message.to}`}
          </text>
        ) : null}
        <text style={{ fontSize: 13.5, lineHeight: "20px", color: C.secondary, fontFamily: FONT }}>{message.body}</text>
        {message.image ? (
          <img
            src={message.image}
            objectFit="cover"
            style={{ width: '100%', height: 280, borderRadius: 12, backgroundColor: '#000000', marginTop: 12 }}
          />
        ) : null}
      </div>
    </div>
  )
}

function SearchField({
  value,
  onChange,
  placeholder,
  testId,
}: {
  value: string
  onChange: (value: string) => void
  placeholder: string
  testId: string
}) {
  return (
    <div
      style={{
        height: 28,
        flexGrow: 1,
        minWidth: 0,
        appRegion: 'no-drag',
        paddingLeft: 10,
        paddingRight: 8,
        borderRadius: 14,
        backgroundColor: C.raised,
        display: 'flex',
        flexDirection: 'row',
        alignItems: 'center',
      }}
    >
      <input
        testId={testId}
        value={value}
        placeholder={placeholder}
        onChange={(event) => onChange(event.value ?? '')}
        style={{ flexGrow: 1, minWidth: 0, fontSize: 13, fontFamily: FONT, color: C.text }}
      />
      {value ? <IconButton icon="x" size={11} pad={18} onClick={() => onChange('')} /> : null}
    </div>
  )
}

export function MailApp() {
  const [query, setQuery] = useState('')
  const [channelQuery, setChannelQuery] = useState('')
  const [draft, setDraft] = useState('')
  const [active, setActive] = useState('atlas-weekly')
  const [activeChannel, setActiveChannel] = useState('primary')
  const [sidebarFocus, setSidebarFocus] = useState<'channel' | 'thread'>('thread')
  const [threadPane, setThreadPane] = useState<'closed' | 'split' | 'full'>('split')

  const visibleChannels = useMemo(() => {
    const q = channelQuery.trim().toLowerCase()
    if (!q) return CHANNELS
    return CHANNELS.filter((channel) => channel.label.toLowerCase().includes(q))
  }, [channelQuery])

  const visibleThreads = useMemo(() => {
    const q = query.trim().toLowerCase()
    return THREADS
      .filter((thread) => {
        if (thread.channelId !== activeChannel) return false
        if (!q) return true
        return (
          thread.senders.toLowerCase().includes(q) ||
          thread.snippet.toLowerCase().includes(q) ||
          thread.subject.toLowerCase().includes(q)
        )
      })
      .sort((a, b) => b.lastReplyAt - a.lastReplyAt)
  }, [query, activeChannel])

  const thread = THREADS.find((item) => item.id === active) ?? THREADS[0]!

  function selectChannel(id: string) {
    setActiveChannel(id)
    setSidebarFocus('channel')
    setThreadPane('closed')
  }

  function selectThread(id: string, pane: 'split' | 'full') {
    const next = THREADS.find((item) => item.id === id)
    if (!next) return
    setActive(id)
    setActiveChannel(next.channelId)
    setSidebarFocus('thread')
    setThreadPane(pane)
  }

  return (
    <div
      style={{
        display: 'flex',
        flexDirection: 'column',
        width: '100%',
        height: '100%',
        backgroundColor: C.sidebar,
        fontFamily: FONT,
        color: C.text,
      }}
    >
      <div
        style={{
          appRegion: 'drag',
          display: 'flex',
          flexDirection: 'row',
          height: TITLEBAR_HEIGHT,
          flexShrink: 0,
          borderBottomWidth: 1,
          borderColor: C.divider,
          borderBottomStyle: 'solid',
        }}
      >
        <div
          style={{
            width: SIDEBAR_WIDTH,
            flexShrink: 0,
            display: 'flex',
            flexDirection: 'row',
            alignItems: 'center',
            paddingLeft: 8,
            paddingRight: 12,
          }}
        >
          <div style={{ width: TRAFFIC_LIGHT_CLEARANCE, height: '100%', flexShrink: 0 }} />
          <Pill>
            <IconButton icon="sidebar" size={15} testId="sidebar-toggle" />
          </Pill>
        </div>

        {threadPane !== 'full' ? (
          <div
            style={{
              width: threadPane === 'split' ? LIST_WIDTH : undefined,
              flexGrow: threadPane === 'split' ? 0 : 1,
              flexShrink: 0,
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              paddingLeft: 8,
              paddingRight: 8,
              gap: 6,
              borderLeftWidth: 1,
              borderRightWidth: threadPane === 'split' ? 1 : 0,
              borderColor: C.divider,
              borderLeftStyle: 'solid',
              borderRightStyle: 'solid',
            }}
          >
            <Pill>
              <IconButton icon="arrowLeft" dimmed size={13} pad={24} />
              <IconButton icon="arrowRight" dimmed size={13} pad={24} />
              <IconButton icon="clock" size={13} pad={24} />
            </Pill>
            <SearchField value={query} onChange={setQuery} placeholder="Search" testId="search" />
            <Pill>
              <IconButton icon="filter" size={13} pad={24} />
              <IconButton icon="sort" size={13} pad={24} />
            </Pill>
          </div>
        ) : null}

        {threadPane !== 'closed' ? (
          <div
            style={{
              flexGrow: 1,
              minWidth: 0,
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              paddingLeft: 16,
              paddingRight: 12,
              gap: 8,
            }}
          >
            <text
              style={{
                flexGrow: 1,
                minWidth: 0,
                fontSize: 15,
                fontWeight: 600,
                color: C.text,
                fontFamily: FONT,
                whiteSpace: 'nowrap',
                textOverflow: 'ellipsis',
              }}
            >
              {thread.subject}
            </text>
            <Pill>
              <IconButton icon="snooze" size={14} pad={24} />
            </Pill>
            <Pill>
              <IconButton icon="zap" size={14} pad={24} />
            </Pill>
            <Pill>
              <IconButton icon="archive" size={13} pad={24} />
              <IconButton icon="star" size={13} pad={24} />
              <IconButton icon="clock" size={13} pad={24} />
            </Pill>
            <Pill>
              <IconButton icon="block" size={13} pad={24} />
              <IconButton icon="trash" size={13} pad={24} />
              <IconButton icon="more" size={13} pad={24} />
            </Pill>
            <Pill>
              {threadPane === 'full' ? (
                <IconButton icon="minimize" size={13} pad={24} onClick={() => setThreadPane('split')} />
              ) : (
                <IconButton icon="maximize" size={13} pad={24} onClick={() => setThreadPane('full')} testId="thread-full" />
              )}
              <IconButton icon="x" size={13} pad={24} onClick={() => setThreadPane('closed')} testId="thread-close" />
            </Pill>
          </div>
        ) : null}
      </div>

      <div style={{ display: 'flex', flexDirection: 'row', flexGrow: 1, minHeight: 0 }}>
        <div
          style={{
            width: SIDEBAR_WIDTH,
            flexShrink: 0,
            height: '100%',
            display: 'flex',
            flexDirection: 'column',
            borderRightWidth: 1,
            borderColor: C.divider,
            borderRightStyle: 'solid',
            userSelect: 'none',
          }}
        >
          <div
            style={{
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              paddingLeft: 12,
              paddingRight: 8,
              paddingTop: 10,
              paddingBottom: 10,
              gap: 8,
              flexShrink: 0,
            }}
          >
            <Face {...maraFace} size={28} />
            <div style={{ flexGrow: 1, minWidth: 0, gap: 1 }}>
              <div style={{ display: 'flex', flexDirection: 'row', alignItems: 'center', gap: 4 }}>
                <text style={{ fontSize: 13.5, fontWeight: 600, color: C.text, fontFamily: FONT }}>Mara Lin</text>
                <Icon name="chevronDown" size={12} color={C.muted} />
              </div>
              <text
                style={{
                  fontSize: 11.5,
                  color: C.muted,
                  fontFamily: FONT,
                  whiteSpace: 'nowrap',
                  textOverflow: 'ellipsis',
                }}
              >
                mara@northlight...
              </text>
            </div>
            <IconButton icon="compose" size={15} pad={28} />
          </div>

          <div style={{ paddingLeft: 8, paddingRight: 8, paddingBottom: 8, flexShrink: 0 }}>
            <SearchField
              value={channelQuery}
              onChange={setChannelQuery}
              placeholder="Find a channel"
              testId="find-channel"
            />
          </div>

          <div
            style={{
              flexGrow: 1,
              minHeight: 0,
              overflowY: 'scroll',
              paddingLeft: 8,
              paddingRight: 8,
              paddingBottom: 8,
            }}
          >
            {visibleChannels.map((channel) => {
              const children = THREADS
                .filter((item) => item.channelId === channel.id)
                .sort((a, b) => b.lastReplyAt - a.lastReplyAt)
                .slice(0, 4)
              const unread = children.some((item) => item.unread)
              return (
                <div key={channel.id} style={{ paddingBottom: 2 }}>
                  <ChannelRow
                    channel={channel}
                    active={activeChannel === channel.id && sidebarFocus === 'channel'}
                    unread={unread}
                    mentionCount={0}
                    onSelect={() => selectChannel(channel.id)}
                  />
                  {children.map((item, index) => (
                    <SidebarThreadRow
                      key={item.id}
                      thread={item}
                      last={index === children.length - 1}
                      selected={item.id === active && sidebarFocus === 'thread'}
                      onSelect={() => selectThread(item.id, 'full')}
                    />
                  ))}
                </div>
              )
            })}
          </div>

          <div
            style={{
              height: 44,
              paddingLeft: 14,
              paddingRight: 10,
              display: 'flex',
              flexDirection: 'row',
              alignItems: 'center',
              flexShrink: 0,
            }}
          >
            <Icon name="hash" size={13} color={C.muted} />
            <text style={{ flexGrow: 1, marginLeft: 8, fontSize: 13, color: C.muted, fontFamily: FONT }}>
              Feedback
            </text>
            <Icon name="puzzle" size={14} color={C.muted} />
          </div>
        </div>

        {threadPane !== 'full' ? (
          <div
            style={{
              width: threadPane === 'split' ? LIST_WIDTH : undefined,
              flexGrow: threadPane === 'split' ? 0 : 1,
              flexShrink: 0,
              height: '100%',
              borderRightWidth: threadPane === 'split' ? 1 : 0,
              borderColor: C.divider,
              borderRightStyle: 'solid',
              overflowY: 'scroll',
              paddingLeft: 8,
              paddingRight: 8,
              paddingBottom: 12,
            }}
          >
            {visibleThreads.map((item) => (
              <TimelineRow
                key={item.id}
                thread={item}
                selected={item.id === active}
                onSelect={() => selectThread(item.id, 'split')}
              />
            ))}
          </div>
        ) : null}

        {threadPane !== 'closed' ? (
          <div
            style={{
              flexGrow: 1,
              minWidth: 0,
              height: '100%',
              backgroundColor: C.reading,
              position: 'relative',
            }}
          >
            <div
              style={{
                height: '100%',
                overflowY: 'scroll',
                paddingLeft: 22,
                paddingRight: 22,
                paddingBottom: 88,
              }}
            >
              {thread.faces.length === 1 ? <DirectHeader thread={thread} /> : null}
              {thread.messages.map((message, index) => {
                const day = messageDay(thread, message)
                const previous = index > 0 ? messageDay(thread, thread.messages[index - 1]!) : null
                return (
                  <div key={message.id}>
                    {day !== previous ? <DayDivider label={day} /> : null}
                    <MessageRow message={message} />
                  </div>
                )
              })}
            </div>

            <div
              style={{
                position: 'absolute',
                left: 36,
                right: 36,
                bottom: 16,
                height: 40,
                display: 'flex',
                flexDirection: 'row',
                alignItems: 'center',
                paddingLeft: 14,
                paddingRight: 8,
                gap: 4,
                borderRadius: 22,
                backgroundColor: C.raised,
                borderWidth: 1,
                borderColor: C.borderStrong,
                borderStyle: 'solid',
                pointerEvents: 'auto',
              }}
            >
              <input
                testId="composer"
                value={draft}
                placeholder="Ask anything, @ for more..."
                onChange={(event) => setDraft(event.value ?? '')}
                style={{ flexGrow: 1, fontSize: 13, fontFamily: FONT, color: C.text }}
              />
              <IconButton icon="at" size={14} pad={26} />
              <IconButton icon="paperclip" size={14} pad={26} />
              <div style={{ width: 1, height: 14, backgroundColor: C.border, marginLeft: 4, marginRight: 4 }} />
              <IconButton icon="mic" size={14} pad={26} />
            </div>
          </div>
        ) : null}
      </div>
    </div>
  )
}

const isEntryPoint =
  typeof Bun !== 'undefined'
    ? Bun.isStandaloneExecutable || Bun.main === import.meta.path
    : typeof process !== 'undefined' && process.argv[1]?.endsWith('mail.tsx')

if (isEntryPoint) {
  applyMacCpuThrottleFromEnv()
  render(<MailApp />, {
    title: 'Mail',
    appName: 'Mail',
    width: 1280,
    height: 860,
    titlebarTransparent: true,
    windowBackground: C.sidebar,
    trafficLightX: 16,
    trafficLightY: 17,
    focus: process.env.GPUIX_BACKGROUND !== '1',
  })
}
