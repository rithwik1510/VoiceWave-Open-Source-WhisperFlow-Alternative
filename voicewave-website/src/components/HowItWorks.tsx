import { useCallback, useEffect, useRef, useState } from 'react'
import { Code2, Keyboard, Mail, MessageSquare, MessageSquareText, TerminalSquare, Type } from 'lucide-react'

// The centerpiece: a floating pill dictating into real app windows.
// VoiceWave's whole product is "text lands wherever you are" — so the
// demo shows exactly that, cycling through an editor, chat, mail, and
// a terminal. Hold the pill to drive it yourself.

type DictateState = 'idle' | 'listening' | 'transcribing' | 'typing' | 'inserted'

const BAR_COUNT = 8

type AppId = 'vscode' | 'slack' | 'mail' | 'terminal'

const APPS: Array<{ id: AppId; label: string; icon: typeof Code2; sample: string }> = [
  {
    id: 'vscode',
    label: 'VS Code',
    icon: Code2,
    sample: '// Debounce the resize handler before the Friday release.'
  },
  {
    id: 'slack',
    label: 'Slack',
    icon: MessageSquare,
    sample: 'Just pushed the fix — can you rerun the pipeline?'
  },
  {
    id: 'mail',
    label: 'Mail',
    icon: Mail,
    sample: 'Hi Sarah, the revised proposal is attached. Let me know by Thursday.'
  },
  {
    id: 'terminal',
    label: 'Terminal',
    icon: TerminalSquare,
    sample: 'git commit -m "fix: stop double insert on release"'
  }
]

const STEPS = [
  { icon: Keyboard, title: 'Hold', body: 'Ctrl + Win from any app.' },
  { icon: MessageSquareText, title: 'Speak', body: 'Whisper runs on your hardware.' },
  { icon: Type, title: 'Release', body: 'Text lands at your cursor.' }
]

function clamp01(v: number) {
  return Math.max(0, Math.min(1, v))
}

const Caret = ({ color = '#1b8eff' }: { color?: string }) => (
  <span className="ml-0.5 inline-block h-[1.05em] w-[2px] animate-pulse align-text-bottom" style={{ background: color }} />
)

function AppWindow({ app, typed, active }: { app: AppId; typed: string; active: boolean }) {
  if (app === 'vscode') {
    return (
      <div className="overflow-hidden rounded-xl border border-[#20263a] bg-[#12141f] text-left shadow-[0_18px_50px_-22px_rgba(9,15,40,0.65)]">
        <div className="flex items-center gap-2 border-b border-[#20263a] bg-[#171a28] px-3.5 py-2">
          <span className="flex gap-1.5" aria-hidden="true">
            <span className="h-2 w-2 rounded-full bg-[#2b3149]" />
            <span className="h-2 w-2 rounded-full bg-[#2b3149]" />
            <span className="h-2 w-2 rounded-full bg-[#2b3149]" />
          </span>
          <span className="font-mono text-[10px] text-[#7a86ad]">resize.ts — voicewave</span>
        </div>
        <div className="grid grid-cols-[2rem_1fr] gap-x-3 px-3.5 py-3 font-mono text-[11px] leading-[1.7] sm:text-xs">
          <div className="select-none text-right text-[#3b4262]">{'1\n2\n3\n4'.split('\n').map((n) => <div key={n}>{n}</div>)}</div>
          <div className="min-w-0">
            <div><span className="text-[#c792ea]">const</span> <span className="text-[#82aaff]">onResize</span> <span className="text-[#89ddff]">=</span> <span className="text-[#c792ea]">()</span> <span className="text-[#c792ea]">=&gt;</span> <span className="text-[#89ddff]">{'{'}</span></div>
            <div className="pl-4"><span className="text-[#82aaff]">field</span>.<span className="text-[#82aaff]">updateDrawResolution</span><span className="text-[#89ddff]">()</span></div>
            <div><span className="text-[#89ddff]">{'}'}</span></div>
            <div className="text-[#5d6a94] italic">
              {typed}
              {active && <Caret color="#7ed8ff" />}
            </div>
          </div>
        </div>
      </div>
    )
  }

  if (app === 'terminal') {
    return (
      <div className="overflow-hidden rounded-xl border border-[#1c2030] bg-[#0b0d14] text-left shadow-[0_18px_50px_-22px_rgba(9,15,40,0.65)]">
        <div className="flex items-center gap-2 border-b border-[#1c2030] bg-[#10131d] px-3.5 py-2">
          <TerminalSquare className="h-3 w-3 text-[#5d6a94]" aria-hidden="true" />
          <span className="font-mono text-[10px] text-[#7a86ad]">powershell — voicewave</span>
        </div>
        <div className="px-3.5 py-3 font-mono text-[11px] leading-[1.8] sm:text-xs">
          <div className="text-[#5d6a94]">PS C:\dev\voicewave&gt; git status</div>
          <div className="text-[#4c9f70]">modified: src/insertion.rs</div>
          <div className="text-[#e6edf3]">
            <span className="text-[#5d6a94]">PS C:\dev\voicewave&gt; </span>
            {typed}
            {active && <Caret color="#7ed8ff" />}
          </div>
        </div>
      </div>
    )
  }

  if (app === 'slack') {
    return (
      <div className="overflow-hidden rounded-xl border border-[#e3ebf5] bg-white text-left shadow-[0_18px_50px_-22px_rgba(30,60,110,0.28)]">
        <div className="flex items-center gap-2 border-b border-[#edf2f8] px-3.5 py-2">
          <span className="text-sm font-bold text-[#0f172a]"># release</span>
          <span className="text-[10px] text-[#94a3b8]">3 members</span>
        </div>
        <div className="space-y-2.5 px-3.5 py-3">
          <div className="flex items-start gap-2">
            <span className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-md bg-[#ffd7ba] text-[10px] font-bold text-[#8a4b1f]">S</span>
            <div>
              <p className="text-[11px] font-bold text-[#0f172a]">Sam <span className="ml-1 font-normal text-[#94a3b8]">10:52</span></p>
              <p className="text-xs text-[#334155]">CI is red on the insertion test again 😩</p>
            </div>
          </div>
          <div className="rounded-lg border border-[#dce6f2] bg-[#fbfdff] px-3 py-2">
            <p className="min-h-[1.2rem] text-xs text-[#0f172a]">
              {typed || <span className="text-[#a5b3c6]">Message #release</span>}
              {active && <Caret />}
            </p>
          </div>
        </div>
      </div>
    )
  }

  return (
    <div className="overflow-hidden rounded-xl border border-[#e3ebf5] bg-white text-left shadow-[0_18px_50px_-22px_rgba(30,60,110,0.28)]">
      <div className="border-b border-[#edf2f8] px-3.5 py-2">
        <p className="text-[11px] text-[#64748b]">To: <span className="text-[#0f172a]">sarah@studio.co</span></p>
        <p className="text-[11px] text-[#64748b]">Subject: <span className="font-medium text-[#0f172a]">Revised proposal</span></p>
      </div>
      <div className="px-3.5 py-3">
        <p className="min-h-[3rem] text-xs leading-relaxed text-[#0f172a]">
          {typed}
          {active && <Caret />}
        </p>
      </div>
    </div>
  )
}

function InsertionStage() {
  const [state, setState] = useState<DictateState>('idle')
  const [bars, setBars] = useState<number[]>(() => Array(BAR_COUNT).fill(0.15))
  const [typed, setTyped] = useState('')
  const [appIdx, setAppIdx] = useState(0)

  const phaseRef = useRef(0)
  const rafRef = useRef(0)
  const timersRef = useRef<number[]>([])
  const isHoldingRef = useRef(false)
  const stateRef = useRef<DictateState>('idle')
  stateRef.current = state

  const app = APPS[appIdx % APPS.length]

  const clearTimers = useCallback(() => {
    for (const id of timersRef.current) window.clearTimeout(id)
    timersRef.current = []
  }, [])

  useEffect(() => () => clearTimers(), [clearTimers])

  useEffect(() => {
    const loop = (ts: number) => {
      phaseRef.current = ts * 0.004
      const s = stateRef.current
      setBars(
        Array.from({ length: BAR_COUNT }, (_, i) => {
          if (s === 'listening') {
            const wave = Math.abs(Math.sin(phaseRef.current + i * 0.72) * Math.cos(phaseRef.current * 0.45 + i * 0.3))
            return clamp01(0.22 + wave * 0.9)
          }
          if (s === 'transcribing' || s === 'typing') {
            const wave = Math.abs(Math.sin(phaseRef.current * 2 + i * 0.5))
            return clamp01(0.2 + wave * 0.4)
          }
          return clamp01(0.1 + Math.abs(Math.sin(phaseRef.current * 0.35 + i * 0.6)) * 0.06)
        })
      )
      rafRef.current = requestAnimationFrame(loop)
    }
    rafRef.current = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(rafRef.current)
  }, [])

  const runInsertion = useCallback(() => {
    setState('transcribing')
    const sample = APPS[appIdx % APPS.length].sample
    const t1 = window.setTimeout(() => {
      setState('typing')
      let charIdx = 0
      const typeNext = () => {
        charIdx += 1
        setTyped(sample.slice(0, charIdx))
        if (charIdx < sample.length) {
          timersRef.current.push(window.setTimeout(typeNext, 16))
        } else {
          setState('inserted')
          timersRef.current.push(
            window.setTimeout(() => {
              setTyped('')
              setState('idle')
              setAppIdx((idx) => (idx + 1) % APPS.length)
            }, 2100)
          )
        }
      }
      typeNext()
    }, 780)
    timersRef.current.push(t1)
  }, [appIdx])

  // Autoplay: idle → listening → insertion, forever.
  useEffect(() => {
    if (isHoldingRef.current) return
    let timeoutId: number | null = null
    if (state === 'idle') {
      timeoutId = window.setTimeout(() => setState('listening'), 900)
    } else if (state === 'listening') {
      timeoutId = window.setTimeout(runInsertion, 1700)
    }
    return () => {
      if (timeoutId !== null) window.clearTimeout(timeoutId)
    }
  }, [state, runInsertion])

  const startHold = () => {
    if (stateRef.current === 'typing' || stateRef.current === 'transcribing') return
    clearTimers()
    isHoldingRef.current = true
    setTyped('')
    setState('listening')
  }

  const endHold = () => {
    if (!isHoldingRef.current) return
    isHoldingRef.current = false
    if (stateRef.current === 'listening') {
      runInsertion()
    }
  }

  const pillBg =
    state === 'inserted'
      ? 'linear-gradient(135deg, #0050d2, #1b8eff)'
      : state === 'listening'
        ? 'linear-gradient(135deg, #0a2a8c, #1b8eff)'
        : state === 'transcribing' || state === 'typing'
          ? 'linear-gradient(135deg, #0032b8, #1b73dc)'
          : '#09090b'

  const labelText =
    state === 'idle'
      ? 'Hold to dictate'
      : state === 'listening'
        ? 'Listening…'
        : state === 'transcribing'
          ? 'Transcribing…'
          : state === 'typing'
            ? 'Inserting…'
            : 'Inserted'

  return (
    <div className="mx-auto mt-10 w-full max-w-2xl">
      {/* App switcher */}
      <div className="flex items-center justify-center gap-1.5" role="tablist" aria-label="Demo target app">
        {APPS.map((item, idx) => {
          const isActive = idx === appIdx % APPS.length
          return (
            <button
              key={item.id}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => {
                clearTimers()
                setTyped('')
                setState('idle')
                setAppIdx(idx)
              }}
              className={`flex items-center gap-1.5 rounded-full px-3 py-1.5 font-mono text-[10px] font-bold uppercase tracking-[0.1em] transition-colors ${
                isActive ? 'bg-[#09090B] text-white' : 'text-[#61758f] hover:bg-[#eef4fb] hover:text-[#0f172a]'
              }`}
            >
              <item.icon className="h-3 w-3" aria-hidden="true" />
              {item.label}
            </button>
          )
        })}
      </div>

      {/* The focused app */}
      <div className="relative mt-4">
        <AppWindow app={app.id} typed={typed} active={state === 'typing' || state === 'inserted'} />

        {/* Floating pill, docked at the bottom edge like the real one */}
        <div className="pointer-events-none absolute inset-x-0 -bottom-7 flex justify-center">
          <button
            type="button"
            className="pointer-events-auto relative select-none outline-none"
            style={{ touchAction: 'none', WebkitUserSelect: 'none' }}
            onMouseDown={startHold}
            onMouseUp={endHold}
            onMouseLeave={endHold}
            onTouchStart={(e) => {
              e.preventDefault()
              startHold()
            }}
            onTouchEnd={(e) => {
              e.preventDefault()
              endHold()
            }}
            aria-label="Hold to dictate"
          >
            {state === 'listening' && (
              <span
                className="absolute inset-0 animate-ping rounded-full"
                style={{ background: '#7ed8ff', opacity: 0.22, animationDuration: '1.1s' }}
              />
            )}
            <span
              className="relative flex items-center gap-3 rounded-full px-6 py-3"
              style={{
                background: pillBg,
                boxShadow:
                  state === 'listening'
                    ? '0 0 36px rgba(126, 216, 255, 0.55), 0 8px 24px rgba(0,0,0,0.3)'
                    : '0 8px 28px rgba(0,0,0,0.32)',
                transform: state === 'listening' ? 'scale(1.05)' : 'scale(1)',
                transition: 'transform 200ms ease, box-shadow 300ms ease, background 300ms ease'
              }}
            >
              <span className="flex items-center gap-[3px]" aria-hidden>
                {bars.map((scale, i) => (
                  <span
                    key={i}
                    className="block w-[3px] rounded-full"
                    style={{
                      height: '18px',
                      transform: `scaleY(${scale.toFixed(3)})`,
                      background: state === 'idle' ? '#5a6c84' : '#ffffff',
                      transition: 'background 300ms ease'
                    }}
                  />
                ))}
              </span>
              <span
                className="font-mono text-[11px] font-bold uppercase tracking-[0.12em]"
                style={{ color: state === 'idle' ? '#8ea3bc' : '#ffffff', transition: 'color 300ms ease' }}
              >
                {labelText}
              </span>
            </span>
          </button>
        </div>
      </div>

      <p className="mt-12 text-center font-mono text-[9px] uppercase tracking-[0.16em] text-[#94a3b8]">
        Hold the pill to drive it yourself
      </p>
    </div>
  )
}

export default function HowItWorks() {
  return (
    <section id="how" className="scroll-mt-28 py-16 sm:py-20">
      <div className="site-shell">
        <div className="mx-auto max-w-3xl text-center">
          <p className="section-eyebrow justify-center font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
            <span aria-hidden="true" className="section-eyebrow-tick" />
            How it works
          </p>
          <h2 className="mt-4 font-display text-[clamp(2rem,5vw,3.4rem)] font-bold leading-[1.02] tracking-tight text-[#09090B]">
            Hold. Speak. Release.
          </h2>
          <p className="mt-4 text-pretty text-base leading-relaxed text-[#475569] sm:text-lg">
            It types wherever you are. No account, no setup wizard, no Python.
          </p>
        </div>

        <div className="mx-auto mt-10 flex max-w-2xl flex-wrap items-start justify-center gap-x-10 gap-y-4">
          {STEPS.map((step, idx) => (
            <div key={step.title} className="flex items-center gap-3">
              <span className="flex h-9 w-9 items-center justify-center rounded-full bg-[#09090B] text-white">
                <step.icon className="h-4 w-4" aria-hidden="true" />
              </span>
              <div className="text-left">
                <p className="font-mono text-[9px] font-bold uppercase tracking-[0.14em] text-[#8ba2bb]">0{idx + 1}</p>
                <p className="text-sm font-bold leading-tight text-[#09090B]">
                  {step.title} <span className="font-normal text-[#475569]">— {step.body}</span>
                </p>
              </div>
            </div>
          ))}
        </div>

        <InsertionStage />
      </div>
    </section>
  )
}
