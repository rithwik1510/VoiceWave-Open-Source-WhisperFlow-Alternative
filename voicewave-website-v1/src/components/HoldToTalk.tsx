import { useCallback, useEffect, useRef, useState } from 'react'

type DictateState = 'idle' | 'listening' | 'transcribing' | 'inserted'

const SAMPLES = [
  "Ship the release — we're good to go.",
  'Add unit tests for the auth module.',
  'Schedule team sync for Thursday at 3.',
  'Draft specs for the new onboarding flow.',
]

const BAR_COUNT = 8

function clamp01(v: number) {
  return Math.max(0, Math.min(1, v))
}

export default function HoldToTalk() {
  const [state, setState] = useState<DictateState>('idle')
  const [bars, setBars] = useState<number[]>(() => Array(BAR_COUNT).fill(0.15))
  const [displayText, setDisplayText] = useState('')
  const [sampleIdx, setSampleIdx] = useState(0)

  const phaseRef = useRef(0)
  const rafRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const typeTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const isHoldingRef = useRef(false)
  const stateRef = useRef<DictateState>('idle')
  stateRef.current = state

  const clearTimers = useCallback(() => {
    if (timerRef.current) clearTimeout(timerRef.current)
    if (typeTimerRef.current) clearTimeout(typeTimerRef.current)
  }, [])

  useEffect(() => {
    const loop = (ts: number) => {
      phaseRef.current = ts * 0.004
      const s = stateRef.current
      setBars(
        Array.from({ length: BAR_COUNT }, (_, i) => {
          if (s === 'listening') {
            const wave = Math.abs(
              Math.sin(phaseRef.current + i * 0.72) *
                Math.cos(phaseRef.current * 0.45 + i * 0.3)
            )
            return clamp01(0.22 + wave * 0.9)
          }
          if (s === 'transcribing') {
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

  const startHold = useCallback(() => {
    if (stateRef.current === 'inserted' || stateRef.current === 'transcribing') return
    clearTimers()
    isHoldingRef.current = true
    setDisplayText('')
    setState('listening')
  }, [clearTimers])

  const endHold = useCallback(() => {
    if (!isHoldingRef.current) return
    isHoldingRef.current = false
    if (stateRef.current !== 'listening') return
    setState('transcribing')
    const sample = SAMPLES[sampleIdx]
    timerRef.current = setTimeout(() => {
      setState('inserted')
      let charIdx = 0
      const typeNext = () => {
        charIdx++
        setDisplayText(sample.slice(0, charIdx))
        if (charIdx < sample.length) {
          typeTimerRef.current = setTimeout(typeNext, 20)
        }
      }
      typeNext()
      timerRef.current = setTimeout(() => {
        setState('idle')
        setDisplayText('')
        setSampleIdx(idx => (idx + 1) % SAMPLES.length)
      }, 3500)
    }, 1000)
  }, [sampleIdx])

  useEffect(() => () => clearTimers(), [clearTimers])

  const pillBg =
    state === 'inserted'
      ? 'linear-gradient(180deg, #d9f99d, #bef264)'
      : state === 'listening'
        ? 'linear-gradient(135deg, #0a2a8c, #1b8eff)'
        : state === 'transcribing'
          ? 'linear-gradient(135deg, #0032b8, #1b73dc)'
          : '#09090b'

  const pillShadow =
    state === 'listening'
      ? '0 0 36px rgba(126, 216, 255, 0.55), 0 8px 24px rgba(0,0,0,0.3)'
      : state === 'inserted'
        ? '0 0 36px rgba(190, 242, 100, 0.5), 0 8px 24px rgba(0,0,0,0.2)'
        : '0 8px 28px rgba(0,0,0,0.32)'

  const barColor =
    state === 'inserted' ? '#112012' : state === 'idle' ? '#3f5168' : '#ffffff'

  const labelColor =
    state === 'inserted' ? '#112012' : state === 'idle' ? '#6b7f97' : '#ffffff'

  const labelText =
    state === 'idle'
      ? 'Hold to dictate'
      : state === 'listening'
        ? 'Listening\u2026'
        : state === 'transcribing'
          ? 'Transcribing\u2026'
          : 'Inserted'

  return (
    <section className="py-16 sm:py-20">
      <div className="site-shell-tight flex flex-col items-center gap-6 text-center">
        <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-[#64748b]">
          Interactive Demo
        </p>
        <h2 className="font-display text-3xl font-bold tracking-tight text-[#0a1020] sm:text-4xl">
          Feel the dictation loop.
        </h2>
        <p className="max-w-xs text-base text-[#475569]">
          Press and hold below — just like the real app.
        </p>

        <div className="mt-2 flex w-full flex-col items-center gap-5">
          {/* Hold button */}
          <button
            type="button"
            className="relative select-none outline-none"
            style={{ touchAction: 'none', WebkitUserSelect: 'none' }}
            onMouseDown={startHold}
            onMouseUp={endHold}
            onMouseLeave={endHold}
            onTouchStart={e => {
              e.preventDefault()
              startHold()
            }}
            onTouchEnd={e => {
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
              className="relative flex items-center gap-3 rounded-full px-7 py-3.5"
              style={{
                background: pillBg,
                boxShadow: pillShadow,
                transform: state === 'listening' ? 'scale(1.05)' : 'scale(1)',
                transition: 'transform 200ms ease, box-shadow 300ms ease, background 300ms ease',
              }}
            >
              <span className="flex items-center gap-[3px]" aria-hidden>
                {bars.map((scale, i) => (
                  <span
                    key={i}
                    className="block w-[3px] rounded-full"
                    style={{
                      height: '20px',
                      transform: `scaleY(${scale.toFixed(3)})`,
                      background: barColor,
                      transition: 'background 300ms ease',
                    }}
                  />
                ))}
              </span>
              <span
                className="font-mono text-xs font-bold uppercase tracking-[0.12em]"
                style={{ color: labelColor, transition: 'color 300ms ease' }}
              >
                {labelText}
              </span>
            </span>
          </button>

          {/* Output area */}
          <div className="flex min-h-[3.5rem] w-full max-w-md items-center justify-center px-4">
            {state === 'transcribing' && (
              <span className="flex items-center gap-2">
                {[0, 1, 2].map(i => (
                  <span
                    key={i}
                    className="h-2 w-2 animate-bounce rounded-full bg-[#1b8eff]"
                    style={{ animationDelay: `${i * 0.14}s`, animationDuration: '0.65s' }}
                  />
                ))}
              </span>
            )}
            {state === 'inserted' && displayText && (
              <div className="flex flex-col items-center gap-2.5">
                <p
                  className="rounded-2xl px-5 py-2.5 text-base font-medium text-[#0a1020] sm:text-lg"
                  style={{
                    background: 'rgba(190, 242, 100, 0.13)',
                    border: '1px solid rgba(190, 242, 100, 0.36)',
                  }}
                >
                  &ldquo;{displayText}
                  <span
                    className="ml-0.5 inline-block h-4 w-0.5 animate-pulse align-middle"
                    style={{ background: '#bef264' }}
                />
                  &rdquo;
                </p>
                {displayText === SAMPLES[sampleIdx] && (
                  <span
                    key={`saved-${sampleIdx}`}
                    className="hold-saved-badge inline-flex items-center gap-2 font-mono text-[10px] uppercase tracking-[0.18em] text-[#355478] sm:text-[11px]"
                  >
                    <span aria-hidden="true" className="hold-saved-tick" />
                    Saved ~{Math.max(4, Math.round(SAMPLES[sampleIdx].length / 4.5 - 2))}s vs typing
                    <span aria-hidden="true" className="hold-saved-tick" />
                  </span>
                )}
              </div>
            )}
          </div>

          {state === 'idle' && (
            <p className="font-mono text-[9px] uppercase tracking-[0.16em] text-[#94a3b8]">
              sample {sampleIdx + 1} of {SAMPLES.length} &middot; release to insert
            </p>
          )}
        </div>
      </div>
    </section>
  )
}
