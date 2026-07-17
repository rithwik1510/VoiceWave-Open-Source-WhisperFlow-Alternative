import { motion, useReducedMotion } from 'framer-motion'
import {
  BarChart3,
  Cpu,
  Crown,
  FileText,
  HelpCircle,
  History,
  Home,
  Mic,
  Monitor,
  Palette,
  PanelLeftClose,
  PanelLeftOpen,
  Pause,
  Settings,
  UserCircle,
  Zap
} from 'lucide-react'
import { useCallback, useEffect, useRef, useState } from 'react'
import '../vwapp.css'

// The shell, nav, home dashboard, and state machine below are ported from the
// desktop app's Layout.tsx + Dashboard.tsx (Quiet Ink design system) so the
// demo matches what users actually install. Data is simulated.

type DemoState = 'idle' | 'listening' | 'transcribing' | 'inserted'
type Pane =
  | 'home'
  | 'models'
  | 'dictionary'
  | 'sessions'
  | 'stats'
  | 'pro'
  | 'style'
  | 'settings'
  | 'help'

type HistoryRecord = { id: string; text: string; at: string }

const PUSH_TO_TALK_HOTKEY = 'Ctrl+Windows'
const WAVE_BARS = [18, 34, 26, 44, 30, 50, 22, 42, 28, 36, 24, 40]

const STATUS_META: Record<DemoState, { title: string; hint: string; badge: string; modeLabel: string }> = {
  idle: {
    title: 'Start Dictation',
    hint: 'Press and hold to talk. Release to transcribe.',
    badge: 'Ready',
    modeLabel: 'PUSH TO TALK'
  },
  listening: {
    title: 'Listening...',
    hint: 'Live capture active.',
    badge: 'Live',
    modeLabel: 'PUSH TO TALK'
  },
  transcribing: {
    title: 'Transcribing...',
    hint: 'Local decode in progress.',
    badge: 'Decoding',
    modeLabel: 'AUTO'
  },
  inserted: {
    title: 'Inserted',
    hint: 'Delivered to active app.',
    badge: 'Inserted',
    modeLabel: 'AUTO'
  }
}

const SAMPLES = [
  'This week we closed the release checks and verified the RC build.',
  "Remind me to review Sam's pull request after lunch.",
  'Groceries: oat milk, coffee beans, and something green.',
  'Fix the flaky insertion test before the Friday demo.'
]

const SEED_HISTORY: HistoryRecord[] = [
  { id: 'seed-1', text: 'Merged the mic-guard branch — shipping in the next release.', at: '9:41 AM' },
  { id: 'seed-2', text: 'VoiceWave keeps every word on your machine.', at: '9:12 AM' }
]

const NAV_ITEMS_TOP: Array<{ id: Pane; label: string; icon: typeof Home }> = [
  { id: 'home', label: 'Home', icon: Home },
  { id: 'models', label: 'Models', icon: Cpu },
  { id: 'dictionary', label: 'Dictionary', icon: FileText },
  { id: 'sessions', label: 'History', icon: History },
  { id: 'stats', label: 'Stats', icon: BarChart3 },
  { id: 'pro', label: 'Pro', icon: Crown }
]

const NAV_ITEMS_BOTTOM: Array<{ id: Pane; label: string; icon: typeof Home }> = [
  { id: 'style', label: 'Style', icon: Palette },
  { id: 'settings', label: 'Settings', icon: Settings },
  { id: 'help', label: 'Help', icon: HelpCircle }
]

const WaveLogo = ({ size = 20 }: { size?: number }) => (
  <svg
    width={size}
    height={size}
    viewBox="0 0 24 24"
    fill="none"
    strokeWidth="3"
    strokeLinecap="round"
    strokeLinejoin="round"
    stroke="currentColor"
    aria-hidden="true"
  >
    <path d="M2 10v4" />
    <path d="M6 7v10" />
    <path d="M10 3v18" />
    <path d="M14 3v18" />
    <path d="M18 7v10" />
    <path d="M22 10v4" />
  </svg>
)

const greeting = () => {
  const hour = new Date().getHours()
  if (hour < 12) return 'Good morning.'
  if (hour < 18) return 'Good afternoon.'
  return 'Good evening.'
}

export default function QuietInkDemo() {
  const reducedMotion = Boolean(useReducedMotion())
  const [pane, setPane] = useState<Pane>('home')
  const [collapsed, setCollapsed] = useState(false)
  const [state, setState] = useState<DemoState>('idle')
  const [sampleIdx, setSampleIdx] = useState(0)
  const [lastFinal, setLastFinal] = useState<string | null>(null)
  const [history, setHistory] = useState<HistoryRecord[]>(SEED_HISTORY)
  const [inView, setInView] = useState(false)

  const sectionRef = useRef<HTMLDivElement | null>(null)
  const timeoutsRef = useRef<number[]>([])
  const holdingRef = useRef(false)

  const sample = SAMPLES[sampleIdx % SAMPLES.length]

  const clearTimers = useCallback(() => {
    for (const id of timeoutsRef.current) {
      window.clearTimeout(id)
    }
    timeoutsRef.current = []
  }, [])

  useEffect(() => () => clearTimers(), [clearTimers])

  useEffect(() => {
    const node = sectionRef.current
    if (!node || !('IntersectionObserver' in window)) {
      setInView(true)
      return
    }
    const observer = new IntersectionObserver(
      ([entry]) => setInView(entry?.isIntersecting ?? false),
      { threshold: 0.25 }
    )
    observer.observe(node)
    return () => observer.disconnect()
  }, [])

  const beginListening = useCallback(() => {
    clearTimers()
    setState('listening')
  }, [clearTimers])

  const finishDictation = useCallback(() => {
    clearTimers()
    setState('transcribing')
    const insertedTimeout = window.setTimeout(() => {
      setState('inserted')
      setLastFinal(sample)
      setHistory((current) =>
        [
          {
            id: `demo-${Date.now()}`,
            text: sample,
            at: new Date().toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })
          },
          ...current
        ].slice(0, 6)
      )
    }, 620)
    const idleTimeout = window.setTimeout(() => {
      setState('idle')
      setSampleIdx((idx) => (idx + 1) % SAMPLES.length)
    }, 2400)
    timeoutsRef.current.push(insertedTimeout, idleTimeout)
  }, [clearTimers, sample])

  // Autoplay the loop while visible on the Home pane.
  useEffect(() => {
    if (reducedMotion || !inView || pane !== 'home' || holdingRef.current) {
      return
    }
    let timeoutId: number | null = null
    if (state === 'idle') {
      timeoutId = window.setTimeout(beginListening, 1600)
    } else if (state === 'listening') {
      timeoutId = window.setTimeout(finishDictation, 1500)
    }
    return () => {
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId)
      }
    }
  }, [reducedMotion, inView, pane, state, beginListening, finishDictation])

  const holdStart = () => {
    if (state === 'transcribing') return
    holdingRef.current = true
    beginListening()
  }

  const holdEnd = () => {
    if (!holdingRef.current) return
    holdingRef.current = false
    if (state === 'listening') {
      finishDictation()
    }
  }

  const isRecording = state === 'listening' || state === 'transcribing'
  const statusMeta = STATUS_META[state]
  const stateClass = `vw-home-state-${state}`
  const idleHint = lastFinal ?? `Hold ${PUSH_TO_TALK_HOTKEY} to start capturing`

  const NavButton = ({ item }: { item: { id: Pane; label: string; icon: typeof Home } }) => {
    const isActive = pane === item.id
    const Icon = item.icon
    return (
      <button
        type="button"
        onClick={() => setPane(item.id)}
        aria-current={isActive ? 'page' : undefined}
        title={collapsed ? item.label : undefined}
        className={`vw-nav-button group flex w-full cursor-pointer select-none items-center ${
          collapsed ? 'justify-center px-2' : 'justify-between px-4'
        } rounded-full py-2.5 text-sm transition-colors duration-150 ${
          isActive
            ? 'border border-[#E4E4E7] bg-white font-semibold text-[#09090B] shadow-[0_1px_2px_rgba(9,9,11,0.05)]'
            : 'border border-transparent text-[#52525B] hover:bg-white/55 hover:text-[#09090B]'
        }`}
      >
        <div className={`flex items-center ${collapsed ? 'w-full justify-center gap-0' : 'gap-3'}`}>
          <Icon
            size={17}
            className={`vw-nav-icon ${isActive ? 'text-[#09090B]' : 'text-[#71717A] group-hover:text-[#09090B]'}`}
          />
          <span className="vw-nav-label">{item.label}</span>
        </div>
        {!collapsed && <div className={`vw-nav-active-dot ${isActive ? 'opacity-100' : 'opacity-0'}`} />}
      </button>
    )
  }

  const renderHome = () => (
    <div className="mx-auto max-w-5xl space-y-6 pb-6">
      <section className="flex flex-col justify-between gap-4 lg:flex-row lg:items-end">
        <div className="pt-1">
          <h1 className="font-['Fraunces'] text-4xl tracking-tight text-[#09090B]">{greeting()}</h1>
          <p className="mt-1 text-base font-light text-[#475569] opacity-80">
            System is local and secure. Ready to transcribe.
          </p>
        </div>
        <div className="flex items-center gap-2 pb-1 text-sm text-[#71717A]">
          <span>Hold</span>
          <kbd className="rounded-lg border border-[#E4E4E7] bg-white px-2 py-1 font-sans text-xs font-semibold text-[#09090B] shadow-[0_1px_2px_rgba(9,9,11,0.05)]">
            {PUSH_TO_TALK_HOTKEY}
          </kbd>
          <span>to dictate anywhere</span>
        </div>
      </section>

      <section className="grid gap-4 lg:grid-cols-[1fr_300px]">
        <div className="vw-ring-shell rounded-3xl">
          <div
            className={`vw-ring-inner vw-home-state-card ${stateClass} flex flex-col items-start justify-between gap-5 rounded-3xl bg-white p-7 md:flex-row md:items-center`}
          >
            <div>
              <h3 className="font-['Fraunces'] text-2xl text-[#09090B]">{statusMeta.title}</h3>
              <p className="mt-1 text-sm text-[#475569]">{statusMeta.hint}</p>
              <div className="mt-2 flex flex-wrap items-center gap-2">
                <p className="text-xs uppercase tracking-[0.16em] text-[#71717A]">Model: fw-small.en</p>
                <span className={`vw-home-state-badge ${stateClass}`}>{statusMeta.badge}</span>
              </div>
            </div>
            <button
              type="button"
              aria-label="Hold to dictate (demo)"
              style={{ touchAction: 'none', WebkitUserSelect: 'none' }}
              onMouseDown={holdStart}
              onMouseUp={holdEnd}
              onMouseLeave={holdEnd}
              onTouchStart={(e) => {
                e.preventDefault()
                holdStart()
              }}
              onTouchEnd={(e) => {
                e.preventDefault()
                holdEnd()
              }}
              className={`vw-home-mic-button vw-home-mic-state ${stateClass} ${
                isRecording ? 'vw-home-mic-button-active bg-black' : 'vw-brand-accent'
              } flex h-20 w-20 shrink-0 items-center justify-center rounded-full text-white transition-all duration-300`}
            >
              {isRecording ? <Pause size={28} fill="currentColor" /> : <Mic size={28} />}
            </button>
          </div>
        </div>

        <div className="space-y-3">
          <div className="vw-ring-shell vw-ring-shell-sm rounded-3xl">
            <div className="vw-ring-inner vw-home-secondary-card rounded-3xl bg-white px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-3">
                  <div
                    className="flex h-9 w-9 items-center justify-center rounded-xl"
                    style={{ backgroundImage: 'var(--vw-accent-soft-gradient)' }}
                  >
                    <Cpu size={16} style={{ color: 'var(--vw-accent-blue-600)' }} />
                  </div>
                  <div>
                    <p className="vw-section-heading text-sm font-semibold leading-none text-[#09090B]">Model</p>
                    <p className="mt-1 text-[11px] tracking-[0.14em] text-[#71717A]">FW SMALL.EN</p>
                  </div>
                </div>
                <div className={`vw-status-dot ${stateClass} h-2.5 w-2.5 rounded-full`} />
              </div>
            </div>
          </div>

          <div className="vw-ring-shell vw-ring-shell-sm rounded-3xl">
            <div className="vw-ring-inner vw-home-secondary-card rounded-3xl bg-white px-4 py-3">
              <div className="flex items-center justify-between gap-3">
                <div className="flex items-center gap-3">
                  <div
                    className="flex h-9 w-9 items-center justify-center rounded-xl"
                    style={{ backgroundImage: 'var(--vw-accent-soft-gradient)' }}
                  >
                    <Zap size={16} style={{ color: 'var(--vw-accent-cyan-500)' }} />
                  </div>
                  <div>
                    <p className="vw-section-heading text-sm font-semibold leading-none text-[#09090B]">Mode</p>
                    <p className="mt-1 text-[11px] tracking-[0.14em] text-[#71717A]">{statusMeta.modeLabel}</p>
                  </div>
                </div>
                <span className={`vw-home-mode-chip ${stateClass} rounded-xl border px-2 py-0.5 text-[10px] font-semibold`}>
                  {statusMeta.badge}
                </span>
              </div>
            </div>
          </div>
        </div>
      </section>

      <section>
        <div
          className={`vw-home-state-output ${stateClass} relative flex min-h-20 w-full items-center overflow-hidden rounded-3xl px-8 py-5 transition-colors duration-200 ${
            isRecording
              ? 'vw-output-recording bg-black'
              : 'border border-[#E4E4E7] bg-white shadow-[0_1px_2px_rgba(9,9,11,0.04)]'
          }`}
        >
          <div className="flex flex-1 items-center justify-center">
            {isRecording ? (
              <div className="flex h-8 items-center justify-center gap-1">
                {WAVE_BARS.map((height, index) => (
                  <div
                    key={index}
                    className="w-1 animate-pulse rounded-full bg-white"
                    style={{ height: `${height}%`, animationDelay: `${index * 0.05}s`, animationDuration: '0.8s' }}
                  />
                ))}
              </div>
            ) : (
              <p className="max-w-[56rem] text-center text-sm leading-relaxed text-[#52525B] md:text-base">
                {idleHint}
              </p>
            )}
          </div>
        </div>
      </section>

      <section>
        <p className="vw-section-heading mb-3 text-xs font-semibold tracking-[0.18em] text-[#71717A]">TODAY</p>
        <div className="vw-home-transcript-card overflow-hidden rounded-3xl border border-[#E4E4E7] bg-white">
          {history.map((row, index) => (
            <div
              key={row.id}
              className={`grid grid-cols-[96px_1fr] ${
                index !== history.length - 1 ? 'border-b border-[#F1F1F3]' : ''
              } ${index === 0 ? 'vw-home-row-latest' : ''}`}
            >
              <div className="px-5 py-3.5 text-sm text-[#71717A]">{row.at}</div>
              <div
                className={`px-5 py-3.5 text-[15px] leading-relaxed ${
                  index === 0 ? 'font-medium text-[#09090B]' : 'text-[#27272A]'
                }`}
              >
                {row.text}
              </div>
            </div>
          ))}
        </div>
      </section>
    </div>
  )

  const QuietPane = ({
    kicker,
    heading,
    sub,
    children
  }: {
    kicker: string
    heading: string
    sub?: string
    children: React.ReactNode
  }) => (
    <section className="vw-panel vw-panel-soft mx-auto max-w-5xl">
      <p className="vw-kicker">{kicker}</p>
      <h3 className="vw-section-heading mt-1 text-2xl font-semibold text-[#09090B]">{heading}</h3>
      {sub && <p className="mt-1 max-w-2xl text-sm text-[#71717A]">{sub}</p>}
      <div className="mt-5">{children}</div>
    </section>
  )

  const renderModels = () => (
    <QuietPane kicker="On-device Models" heading="Model Manager" sub="Checksum-verified downloads. Everything runs on this machine.">
      <div className="vw-row-list">
        {[
          { name: 'fw-small.en', desc: 'Fast on any CPU', size: '488 MB', chip: 'Active', active: true },
          { name: 'fw-large-v3-turbo', desc: 'Highest accuracy, GPU recommended', size: '1.6 GB', chip: 'Download', active: false }
        ].map((model) => (
          <div key={model.name} className="flex items-center justify-between gap-4 px-5 py-4">
            <div>
              <p className="text-sm font-semibold text-[#09090B]">{model.name}</p>
              <p className="mt-0.5 text-xs text-[#71717A]">
                {model.desc} · {model.size}
              </p>
            </div>
            {model.active ? (
              <span className="vw-chip vw-chip-accent">Active</span>
            ) : (
              <span className="vw-chip">Download</span>
            )}
          </div>
        ))}
      </div>
    </QuietPane>
  )

  const renderDictionary = () => (
    <QuietPane kicker="Personal Dictionary" heading="Your words" sub="Teach it your terms once. Export and import anytime.">
      <div className="vw-row-list">
        {['faster-whisper', 'VoiceWave', 'Tauri', 'Rithwik'].map((term) => (
          <div key={term} className="flex items-center justify-between px-5 py-3.5">
            <p className="text-sm font-medium text-[#09090B]">{term}</p>
            <span className="vw-chip">Custom</span>
          </div>
        ))}
      </div>
    </QuietPane>
  )

  const renderSessions = () => (
    <QuietPane kicker="Transcription History" heading="History" sub="Stored in a local database. Search it, copy it, or wipe it.">
      <div className="vw-row-list">
        {history.map((row) => (
          <div key={row.id} className="grid grid-cols-[96px_1fr] items-baseline px-5 py-3.5">
            <span className="text-xs tabular-nums text-[#71717A]">{row.at}</span>
            <p className="text-sm leading-relaxed text-[#27272A]">{row.text}</p>
          </div>
        ))}
      </div>
    </QuietPane>
  )

  const renderStats = () => (
    <QuietPane kicker="Usage" heading="Stats" sub="Computed locally. Never uploaded.">
      <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
        {[
          { value: (184 + history.length * 9).toLocaleString(), label: 'Words today' },
          { value: '12,480', label: 'Words this month' },
          { value: '38 min', label: 'Saved this week' },
          { value: '6 days', label: 'Streak' }
        ].map((stat) => (
          <div key={stat.label} className="vw-stat-card">
            <p className="font-['Fraunces'] text-xl font-semibold tabular-nums text-[#09090B]">{stat.value}</p>
            <p className="mt-1 text-[11px] text-[#71717A]">{stat.label}</p>
          </div>
        ))}
      </div>
      <div className="mt-4 flex h-20 items-end gap-1.5 rounded-2xl border border-[#E4E4E7] bg-white p-4" aria-hidden="true">
        {[38, 52, 30, 66, 48, 84, 58, 72, 44, 90, 62, 76, 55, 68].map((height, i) => (
          <div
            key={i}
            className="flex-1 rounded-t-sm"
            style={{ height: `${height}%`, background: 'var(--vw-ai-gradient)', opacity: 0.45 + (i / 34) }}
          />
        ))}
      </div>
    </QuietPane>
  )

  const renderSimple = (kicker: string, heading: string, rows: Array<[string, string]>) => (
    <QuietPane kicker={kicker} heading={heading}>
      <div className="vw-row-list">
        {rows.map(([label, value]) => (
          <div key={label} className="flex items-center justify-between px-5 py-3.5">
            <p className="text-sm font-medium text-[#09090B]">{label}</p>
            <span className="text-sm text-[#71717A]">{value}</span>
          </div>
        ))}
      </div>
    </QuietPane>
  )

  const renderPane = () => {
    switch (pane) {
      case 'home':
        return renderHome()
      case 'models':
        return renderModels()
      case 'dictionary':
        return renderDictionary()
      case 'sessions':
        return renderSessions()
      case 'stats':
        return renderStats()
      case 'pro':
        return renderSimple('VoiceWave Pro', 'Power features', [
          ['Format profiles', 'Included'],
          ['Code mode', 'Included'],
          ['Power history tools', 'Included']
        ])
      case 'style':
        return renderSimple('Style', 'Formatting', [
          ['AI polish', 'Opt-in, on-device'],
          ['Filler removal', 'On'],
          ['Punctuation', 'Auto']
        ])
      case 'settings':
        return renderSimple('Settings', 'Preferences', [
          ['Push-to-talk', PUSH_TO_TALK_HOTKEY],
          ['Insertion', 'Type into active app'],
          ['Updates', 'Signed, via GitHub']
        ])
      case 'help':
        return renderSimple('Help', 'Support', [
          ['Guide', 'Built into the app'],
          ['Issues', 'GitHub'],
          ['Diagnostics', 'Local, encrypted']
        ])
    }
  }

  return (
    <section id="demo" className="scroll-mt-28 px-0 py-10 sm:py-14">
      <div className="site-shell">
        <p className="section-eyebrow font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
          <span aria-hidden="true" className="section-eyebrow-tick" />
          The app itself
        </p>
        <h2 className="mt-4 font-display text-[clamp(2.1rem,5.2vw,3.85rem)] font-bold leading-[1.02] tracking-tight text-[#0a1020]">
          This is what you're installing.
        </h2>
        <p className="mt-3 max-w-3xl text-base text-[#475569] sm:text-lg">
          The real interface, running on simulated input. Click around.
        </p>

        <motion.div
          ref={sectionRef}
          initial={{ opacity: 0, y: reducedMotion ? 0 : 18, scale: reducedMotion ? 1 : 0.965 }}
          whileInView={{ opacity: 1, y: 0, scale: 1 }}
          viewport={{ once: true, amount: 0.22 }}
          transition={{ duration: reducedMotion ? 0.01 : 0.6, ease: [0.16, 1, 0.3, 1] }}
          className="panel-card mt-8 overflow-hidden border-[#d6e5f8]"
        >
          {/* Window chrome */}
          <div className="flex items-center justify-between gap-3 border-b border-[#e3ebf5] bg-[#fbfdff] px-4 py-2.5">
            <div className="flex items-center gap-3">
              <span className="flex items-center gap-1.5" aria-hidden="true">
                <span className="h-2.5 w-2.5 rounded-full bg-[#e4ebf3]" />
                <span className="h-2.5 w-2.5 rounded-full bg-[#e4ebf3]" />
                <span className="h-2.5 w-2.5 rounded-full bg-[#e4ebf3]" />
              </span>
              <span className="font-display text-sm font-bold tracking-tight text-[#0f172a]">VoiceWave</span>
            </div>
            <span className="rounded-full bg-[#f0f5fb] px-2.5 py-1 font-mono text-[9px] font-bold uppercase tracking-[0.1em] text-[#5b7392]">
              Simulated
            </span>
          </div>

          {/* Mobile placeholder */}
          <div className="p-8 text-center md:hidden">
            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-[#e8f1ff]">
              <Monitor className="h-7 w-7 text-[#0a3eb8]" />
            </div>
            <p className="text-base font-semibold text-[#0a1020]">Interactive app demo</p>
            <p className="mt-2 text-sm leading-relaxed text-[#475569]">
              Best on a bigger screen — or install VoiceWave and try the real thing.
            </p>
          </div>

          {/* App shell — ported from the desktop app's Layout */}
          <div className="vwapp hidden md:block">
            <div className="flex h-[640px] w-full overflow-hidden bg-[#EFEFF3] text-[#09090B]">
              <aside
                data-sidebar-collapsed={collapsed ? 'true' : 'false'}
                className={`vw-sidebar-shell relative z-40 ${collapsed ? 'w-20' : 'w-52'} flex flex-shrink-0 flex-col bg-[#EFEFF3]`}
              >
                <div className={`${collapsed ? 'flex-col gap-2 px-3 pb-4 pt-4' : 'gap-3 p-6 pb-4'} flex flex-shrink-0 items-center`}>
                  <div className="vw-brand-accent flex h-10 w-10 shrink-0 items-center justify-center rounded-full text-white shadow-lg shadow-black/5">
                    <WaveLogo size={20} />
                  </div>
                  {!collapsed && (
                    <>
                      <div>
                        <span className="block font-['Fraunces'] text-xl font-semibold leading-none tracking-tight">
                          VoiceWave
                        </span>
                        <span className="mt-1 block text-[10px] font-medium uppercase tracking-widest opacity-60">
                          Harmonic v1.0
                        </span>
                      </div>
                      <button
                        type="button"
                        className="ml-auto h-8 w-8 text-[#52525B] transition hover:text-[#18181B]"
                        onClick={() => setCollapsed(true)}
                        aria-label="Collapse sidebar"
                      >
                        <PanelLeftClose size={17} className="mx-auto" />
                      </button>
                    </>
                  )}
                  {collapsed && (
                    <button
                      type="button"
                      className="h-8 w-8 text-[#52525B] transition hover:text-[#18181B]"
                      onClick={() => setCollapsed(false)}
                      aria-label="Expand sidebar"
                    >
                      <PanelLeftOpen size={17} className="mx-auto" />
                    </button>
                  )}
                </div>

                <nav className={`relative z-50 ${collapsed ? 'px-2' : 'px-4'} flex-shrink-0 space-y-1`}>
                  {NAV_ITEMS_TOP.map((item) => (
                    <NavButton key={item.id} item={item} />
                  ))}
                </nav>

                <div className="flex-1" />

                <div className={`vw-sidebar-pro-wrap ${collapsed ? 'is-collapsed' : ''}`}>
                  <div className="vw-sidebar-pro-wrap-inner">
                    <div className="vw-sidebar-pro-panel p-4">
                      <span className="vw-pro-badge rounded px-2 py-0.5 text-[10px] font-bold uppercase">Pro</span>
                      <p className="mb-3 mt-2.5 text-xs leading-relaxed text-[#52525B]">
                        The release offer unlocks every Pro tool for coders and students.
                      </p>
                      <button className="vw-btn-primary vw-btn-sm w-full" type="button" onClick={() => setPane('pro')}>
                        View Release Offer
                      </button>
                    </div>
                  </div>
                </div>

                <div className="vw-sidebar-divider-wrap">
                  <div className="mx-1 h-px border-t border-[#D4D4D8]" />
                </div>

                <nav className={`relative z-50 ${collapsed ? 'px-2' : 'px-4'} flex-shrink-0 space-y-1 pb-7`}>
                  {NAV_ITEMS_BOTTOM.map((item) => (
                    <NavButton key={item.id} item={item} />
                  ))}
                </nav>
              </aside>

              <main className="relative z-10 flex min-w-0 flex-1 flex-col">
                <header className="z-20 flex h-14 flex-shrink-0 items-center justify-between bg-[#EFEFF3] px-6">
                  <div className="flex items-center gap-3 text-sm">
                    {isRecording && (
                      <div className="flex items-center gap-2 rounded-full bg-black px-3 py-1 text-xs font-medium text-white">
                        <div className="h-2 w-2 animate-pulse rounded-full bg-white" />
                        Recording
                      </div>
                    )}
                  </div>
                  <div className="flex items-center gap-3 rounded-full px-1.5 py-1">
                    <div className="flex h-9 w-9 items-center justify-center rounded-full bg-[#FAFAFA] shadow-sm">
                      <UserCircle size={20} className="opacity-70" />
                    </div>
                    <div className="hidden text-left text-sm sm:block">
                      <p className="font-medium leading-none text-[#09090B]">Workspace</p>
                      <p className="mt-1 text-[11px] leading-none text-[#71717A]">Guest mode</p>
                    </div>
                  </div>
                </header>

                <div className="relative flex-1 overflow-hidden pb-2 pr-2">
                  <div className="relative h-full w-full overflow-y-auto rounded-3xl border border-[#E4E4E7] bg-white shadow-[0_1px_3px_rgba(9,9,11,0.04)]">
                    <div className="min-h-full px-6 py-6">
                      <div key={pane} className="vw-page-shell">
                        {renderPane()}
                      </div>
                    </div>
                  </div>
                </div>
              </main>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  )
}
