import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useRef, useState } from 'react'

type PillState = 'idle' | 'listening' | 'transcribing' | 'inserted'

const DEMO_TEXTS = ['Local mode active.', 'VoiceWave is ready.', 'Still here, local.']

function clamp01(v: number) {
  return Math.max(0, Math.min(1, v))
}

export default function WebsiteFloatingPill() {
  const [visible, setVisible] = useState(false)
  const [state, setState] = useState<PillState>('idle')
  const [bars, setBars] = useState<number[]>(() => Array(8).fill(0.12))
  const [textIdx, setTextIdx] = useState(0)

  const phaseRef = useRef(0)
  const rafRef = useRef(0)
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const stateRef = useRef<PillState>('idle')
  stateRef.current = state

  useEffect(() => {
    const onScroll = () => setVisible(window.scrollY > window.innerHeight * 0.8)
    window.addEventListener('scroll', onScroll, { passive: true })
    onScroll()
    return () => window.removeEventListener('scroll', onScroll)
  }, [])

  useEffect(() => {
    const loop = (ts: number) => {
      phaseRef.current = ts * 0.003
      const s = stateRef.current
      const amp = s === 'listening' ? 0.85 : s === 'transcribing' ? 0.35 : 0.07
      setBars(
        Array.from({ length: 8 }, (_, i) => {
          const wave = Math.abs(
            Math.sin(phaseRef.current + i * 0.65) *
              Math.cos(phaseRef.current * 0.48 + i * 0.22)
          )
          return clamp01(0.1 + wave * amp)
        })
      )
      rafRef.current = requestAnimationFrame(loop)
    }
    rafRef.current = requestAnimationFrame(loop)
    return () => cancelAnimationFrame(rafRef.current)
  }, [])

  const runDemo = () => {
    if (stateRef.current !== 'idle') return
    if (timerRef.current) clearTimeout(timerRef.current)
    setState('listening')
    timerRef.current = setTimeout(() => {
      setState('transcribing')
      timerRef.current = setTimeout(() => {
        setState('inserted')
        setTextIdx(i => (i + 1) % DEMO_TEXTS.length)
        timerRef.current = setTimeout(() => setState('idle'), 2000)
      }, 900)
    }, 1500)
  }

  useEffect(
    () => () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    },
    []
  )

  const pillBg =
    state === 'inserted'
      ? 'linear-gradient(135deg, #d9f99d, #bef264)'
      : state === 'listening'
        ? 'linear-gradient(135deg, #0a2a8c, #1b8eff)'
        : state === 'transcribing'
          ? 'linear-gradient(135deg, #0032b8, #1b73dc)'
          : '#09090b'

  const pillShadow =
    state === 'listening'
      ? '0 0 22px rgba(126, 216, 255, 0.5), 0 4px 16px rgba(0,0,0,0.3)'
      : state === 'inserted'
        ? '0 0 22px rgba(190, 242, 100, 0.45), 0 4px 16px rgba(0,0,0,0.2)'
        : '0 4px 20px rgba(0,0,0,0.4)'

  const barColor =
    state === 'inserted' ? '#112012' : state === 'idle' ? '#374151' : '#ffffff'

  const label =
    state === 'listening'
      ? 'Listening'
      : state === 'transcribing'
        ? 'Processing'
        : state === 'inserted'
          ? DEMO_TEXTS[textIdx]
          : 'VoiceWave'

  const labelColor =
    state === 'inserted' ? '#112012' : state === 'idle' ? '#6b7f94' : '#ffffff'

  return (
    <AnimatePresence>
      {visible && (
        <motion.div
          initial={{ opacity: 0, y: 18, scale: 0.88 }}
          animate={{ opacity: 1, y: 0, scale: 1 }}
          exit={{ opacity: 0, y: 18, scale: 0.88 }}
          transition={{ duration: 0.42, ease: [0.16, 1, 0.3, 1] }}
          className="fixed bottom-6 right-6 z-50 flex flex-col items-end gap-2"
        >
          {/* Label */}
          <div
            className="rounded-full px-3 py-1.5 font-mono text-[9px] uppercase tracking-[0.14em]"
            style={{
              background: 'rgba(9,9,11,0.82)',
              backdropFilter: 'blur(10px)',
              color: 'rgba(255,255,255,0.55)',
            }}
          >
            app overlay &mdash; tap to demo
          </div>

          {/* Pill */}
          <button
            type="button"
            onClick={runDemo}
            disabled={state !== 'idle'}
            aria-label="Demo VoiceWave pill"
            className="flex select-none items-center gap-2.5 rounded-full px-4 py-2.5 outline-none"
            style={{
              background: pillBg,
              boxShadow: pillShadow,
              transition: 'background 300ms ease, box-shadow 300ms ease',
              cursor: state === 'idle' ? 'pointer' : 'default',
            }}
          >
            <span className="flex items-center gap-[2.5px]" aria-hidden>
              {bars.map((scale, i) => (
                <span
                  key={i}
                  className="block w-[2.5px] rounded-full"
                  style={{
                    height: '13px',
                    transform: `scaleY(${scale.toFixed(3)})`,
                    background: barColor,
                    transition: 'background 200ms ease',
                  }}
                />
              ))}
            </span>
            <span
              className="max-w-[120px] overflow-hidden text-ellipsis whitespace-nowrap font-mono text-[10px] font-bold uppercase tracking-[0.1em]"
              style={{ color: labelColor, transition: 'color 200ms ease' }}
            >
              {label}
            </span>
          </button>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
