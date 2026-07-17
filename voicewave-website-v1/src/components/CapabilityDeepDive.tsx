import { motion } from 'framer-motion'
import { Bot, Gauge, Server } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'
import VoiceWaveLogo from './VoiceWaveLogo'

type FlowPhase = 'idle' | 'capture' | 'decode' | 'inserted'

function PrivacyFlowDiagram() {
  const [animKey, setAnimKey] = useState(0)
  const [phase, setPhase] = useState<FlowPhase>('idle')
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  useEffect(() => {
    const sequence: Array<[FlowPhase, number]> = [
      ['capture', 900],
      ['decode', 1000],
      ['inserted', 1400],
      ['idle', 1300],
    ]
    let step = 0
    const next = () => {
      const [ph, delay] = sequence[step % sequence.length]
      setPhase(ph)
      if (ph === 'idle') setAnimKey(k => k + 1)
      step++
      timerRef.current = setTimeout(next, delay)
    }
    timerRef.current = setTimeout(next, 800)
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [])

  const micActive = phase === 'capture' || phase === 'decode' || phase === 'inserted'
  const chipActive = phase === 'decode' || phase === 'inserted'
  const textActive = phase === 'inserted'

  return (
    <div className="relative w-full overflow-hidden rounded-2xl border border-[#1e3557] bg-[#080f1e] p-4">
      <style>{`
        @keyframes pvd-flow-mic {
          0% { stroke-dashoffset: 112; }
          100% { stroke-dashoffset: 0; }
        }
        @keyframes pvd-flow-chip {
          0% { stroke-dashoffset: 90; }
          100% { stroke-dashoffset: 0; }
        }
      `}</style>

      <svg viewBox="0 0 300 100" className="w-full" aria-hidden="true">
        {/* Cloud (always visible, always blocked) */}
        <ellipse cx="150" cy="14" rx="15" ry="8" fill="#0a1528" stroke="#1c3356" strokeWidth="1" />
        <ellipse cx="139" cy="18" rx="9" ry="7" fill="#0a1528" stroke="#1c3356" strokeWidth="1" />
        <ellipse cx="161" cy="18" rx="9" ry="7" fill="#0a1528" stroke="#1c3356" strokeWidth="1" />
        <text x="150" y="20" textAnchor="middle" fill="#233d64" fontSize="5" fontFamily="monospace" letterSpacing="0.5">CLOUD</text>

        {/* Red X over cloud */}
        <line x1="139" y1="8" x2="161" y2="26" stroke="#ef4444" strokeWidth="1.8" strokeLinecap="round" />
        <line x1="161" y1="8" x2="139" y2="26" stroke="#ef4444" strokeWidth="1.8" strokeLinecap="round" />

        {/* Dotted blocked path chip→cloud */}
        <line x1="150" y1="42" x2="150" y2="27" stroke="#1c3356" strokeWidth="1.5" strokeDasharray="2.5 2" />

        {/* MIC icon */}
        <rect x="23" y="38" width="16" height="20" rx="8" fill="#0a1528"
          stroke={micActive ? '#7ed8ff' : '#1c3356'} strokeWidth={micActive ? 1.8 : 1.2}
          style={{ transition: 'stroke 400ms ease' }}
        />
        <line x1="31" y1="58" x2="31" y2="65" stroke={micActive ? '#7ed8ff' : '#1c3356'} strokeWidth="1.5" style={{ transition: 'stroke 400ms ease' }} />
        <line x1="24" y1="65" x2="38" y2="65" stroke={micActive ? '#7ed8ff' : '#1c3356'} strokeWidth="1.5" style={{ transition: 'stroke 400ms ease' }} />

        {/* CHIP icon */}
        <rect x="138" y="38" width="24" height="24" rx="5" fill="#0a1528"
          stroke={chipActive ? '#7ed8ff' : '#1c3356'} strokeWidth={chipActive ? 1.8 : 1.2}
          style={{ transition: 'stroke 400ms ease' }}
        />
        <rect x="143" y="43" width="4" height="4" rx="1" fill={chipActive ? '#7ed8ff' : '#1c3356'} style={{ transition: 'fill 400ms ease' }} />
        <rect x="151" y="43" width="4" height="4" rx="1" fill={chipActive ? '#7ed8ff' : '#1c3356'} style={{ transition: 'fill 400ms ease' }} />
        <rect x="143" y="51" width="4" height="4" rx="1" fill={chipActive ? '#7ed8ff' : '#1c3356'} style={{ transition: 'fill 400ms ease' }} />
        <rect x="151" y="51" width="4" height="4" rx="1" fill={chipActive ? '#7ed8ff' : '#1c3356'} style={{ transition: 'fill 400ms ease' }} />

        {/* TEXT OUTPUT box */}
        <rect x="248" y="40" width="34" height="20" rx="5" fill="#0a1528"
          stroke={textActive ? '#bef264' : '#1c3356'} strokeWidth={textActive ? 1.8 : 1.2}
          style={{ transition: 'stroke 400ms ease' }}
        />
        <line x1="253" y1="47" x2="278" y2="47" stroke={textActive ? '#bef264' : '#1c3356'} strokeWidth="1.5" strokeLinecap="round" style={{ transition: 'stroke 300ms ease' }} />
        <line x1="253" y1="53" x2="271" y2="53" stroke={textActive ? '#9dd87a' : '#1c3356'} strokeWidth="1.5" strokeLinecap="round" style={{ transition: 'stroke 300ms 100ms ease' }} />

        {/* Rail lines */}
        <line x1="39" y1="50" x2="138" y2="50" stroke="#1c3356" strokeWidth="1.5" />
        <line x1="162" y1="50" x2="248" y2="50" stroke="#1c3356" strokeWidth="1.5" />

        {/* Animated flow: mic → chip */}
        {phase === 'capture' && (
          <line
            key={`mic-${animKey}`}
            x1="39" y1="50" x2="138" y2="50"
            stroke="#7ed8ff" strokeWidth="2" strokeLinecap="round"
            strokeDasharray="112"
            style={{ animation: 'pvd-flow-mic 0.85s ease-out forwards' }}
          />
        )}
        {/* Keep mic→chip lit during later phases */}
        {(phase === 'decode' || phase === 'inserted') && (
          <line x1="39" y1="50" x2="138" y2="50" stroke="#7ed8ff" strokeWidth="2" strokeLinecap="round" strokeOpacity="0.7" />
        )}

        {/* Animated flow: chip → text */}
        {phase === 'decode' && (
          <line
            key={`chip-${animKey}`}
            x1="162" y1="50" x2="248" y2="50"
            stroke="#bef264" strokeWidth="2" strokeLinecap="round"
            strokeDasharray="90"
            style={{ animation: 'pvd-flow-chip 0.85s ease-out forwards' }}
          />
        )}
        {phase === 'inserted' && (
          <line x1="162" y1="50" x2="248" y2="50" stroke="#bef264" strokeWidth="2" strokeLinecap="round" strokeOpacity="0.8" />
        )}

        {/* Labels */}
        <text x="31" y="80" textAnchor="middle" fill="#2d4d76" fontSize="5.5" fontFamily="monospace" letterSpacing="0.5">MIC</text>
        <text x="150" y="80" textAnchor="middle" fill="#2d4d76" fontSize="5.5" fontFamily="monospace" letterSpacing="0.5">WHISPER</text>
        <text x="265" y="72" textAnchor="middle" fill="#2d4d76" fontSize="5.5" fontFamily="monospace" letterSpacing="0.5">APP</text>
      </svg>

      <p className="mt-1 text-center font-mono text-[9px] uppercase tracking-[0.15em] text-[#2d4d76]">
        {phase === 'idle' || phase === 'capture'
          ? 'audio path \u2014 local only'
          : phase === 'decode'
            ? 'decoding on-device\u2026'
            : 'text inserted \u2014 no cloud'}
      </p>
    </div>
  )
}

const LATENCY_BAR_LEVELS = Array.from({ length: 40 }, (_, i) => {
  if (i > 15 && i < 25) {
    return `${82 + (i % 5) * 4}%`
  }
  return `${24 + (i % 4) * 6}%`
})

function DeepDiveD() {
  const sectionRef = useRef<HTMLElement | null>(null)
  const [activeGradientIndex, setActiveGradientIndex] = useState(0)

  useEffect(() => {
    let rafId: number | null = null

    const commitActiveGradient = () => {
      rafId = null
      const section = sectionRef.current
      if (!section) {
        return
      }

      const rect = section.getBoundingClientRect()
      const viewportHeight = window.innerHeight || 1
      const travel = Math.max(rect.height - viewportHeight * 0.45, 1)
      const progress = Math.min(Math.max((viewportHeight * 0.26 - rect.top) / travel, 0), 1)

      const nextIndex = progress < 0.34 ? 0 : progress < 0.68 ? 1 : 2
      setActiveGradientIndex((current) => (current === nextIndex ? current : nextIndex))
    }

    const onScrollOrResize = () => {
      if (rafId !== null) {
        return
      }
      rafId = window.requestAnimationFrame(commitActiveGradient)
    }

    commitActiveGradient()
    window.addEventListener('scroll', onScrollOrResize, { passive: true })
    window.addEventListener('resize', onScrollOrResize, { passive: true })

    return () => {
      window.removeEventListener('scroll', onScrollOrResize)
      window.removeEventListener('resize', onScrollOrResize)
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId)
      }
    }
  }, [])

  return (
    <section ref={sectionRef} id="modules" className="section-pad relative scroll-mt-28 bg-transparent text-[#09090B] lg:min-h-[175vh]">
      <div className="pointer-events-none absolute inset-0 z-0 overflow-hidden" aria-hidden>
        <div className={`modules-gradient-layer is-latency ${activeGradientIndex === 0 ? 'is-active' : ''}`} />
        <div className={`modules-gradient-layer is-models ${activeGradientIndex === 1 ? 'is-active' : ''}`} />
        <div className={`modules-gradient-layer is-privacy ${activeGradientIndex === 2 ? 'is-active' : ''}`} />
      </div>

      <div className="site-shell px-0 grid grid-cols-1 lg:grid-cols-12 gap-10 md:gap-14 relative z-10">
        <div className="lg:col-span-5 relative">
          <div className="lg:sticky top-32">
            <div className="section-title-row mb-8">
              <span className="section-motif">
                <VoiceWaveLogo size={9} strokeWidth={2.6} tone="adaptive" adaptiveOn="light" />
              </span>
              <span className="font-mono text-sm uppercase tracking-widest font-bold text-[#4b5e76]">Deep Dive Analysis</span>
            </div>
            <motion.h2
              initial={{ opacity: 0, y: 30 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true, margin: '-100px' }}
              transition={{ duration: 0.6, ease: [0.16, 1, 0.3, 1] }}
              className="font-display text-4xl sm:text-5xl md:text-6xl lg:text-7xl font-bold tracking-tighter text-[#09090B] mb-6 sm:mb-8 leading-[1.05]"
            >
              Power <br />
              <span className="text-[#5b7392]">stacked.</span>
            </motion.h2>
            <motion.p
              initial={{ opacity: 0, y: 30 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true, margin: '-100px' }}
              transition={{ duration: 0.6, delay: 0.1, ease: [0.16, 1, 0.3, 1] }}
              className="text-base sm:text-xl text-[#475569] leading-relaxed font-medium max-w-sm mb-10 sm:mb-12"
            >
              We stripped away the cloud to deliver a predictable on-device dictation loop directly from your machine&apos;s hardware.
            </motion.p>

            <motion.div
              initial={{ opacity: 0 }}
              whileInView={{ opacity: 1 }}
              viewport={{ once: true }}
              transition={{ duration: 0.6, delay: 0.3 }}
              className="hidden lg:flex flex-col gap-4"
            >
              <div className="bg-[#FFFFFF]/92 border border-[#d7e5f7] p-4 vw-radius-tab shadow-sm max-w-xs">
                <span className="block font-mono text-xs text-[#64748B] uppercase mb-1">Compute Environment</span>
                <span className="block font-bold text-sm">Desktop app + local model runtime</span>
              </div>
            </motion.div>
          </div>
        </div>

        <div className="lg:col-span-7 flex flex-col gap-10 sm:gap-14 mt-8 lg:mt-0 relative pb-6 sm:pb-10">
          <motion.div
            initial={{ opacity: 0 }}
            whileInView={{ opacity: 1 }}
            viewport={{ once: true, margin: '-80px' }}
            transition={{ duration: 0.55, ease: [0.16, 1, 0.3, 1] }}
            className="group relative z-10 w-full bg-[#FFFFFF] vw-radius-shell p-6 sm:p-8 lg:p-10 shadow-[0_10px_40px_-18px_rgba(10,30,90,0.18)] border border-[#d8e5f4] transform-gpu transition-[border-color,box-shadow,transform] duration-300 hover:border-[#1b8eff]/58 hover:shadow-[0_18px_48px_-16px_rgba(27,142,255,0.24)]"
          >
            <div className="flex justify-between items-start mb-8 sm:mb-12">
              <div className="w-14 h-14 sm:w-16 sm:h-16 vw-radius-tab bg-[#FAFCFF] border border-[#d8e5f5] flex items-center justify-center shadow-inner transition-colors duration-300 group-hover:border-[#7ed8ff] group-hover:bg-[#ebf6ff]">
                <Gauge className="w-7 h-7 sm:w-8 sm:h-8 text-[#1b8eff]" />
              </div>
              <span className="font-mono text-xs sm:text-sm font-bold bg-[#e5f3ff] text-[#0b3f98] px-3 py-1 rounded-full">Pipeline / Local</span>
            </div>
            <h3 className="font-display text-3xl sm:text-4xl font-bold text-[#09090B] mb-4">Fast Release-to-Text</h3>
            <p className="text-base sm:text-lg text-[#475569] font-medium leading-relaxed max-w-sm mb-8">
              Capture, decode, and insertion run on-device for a tight dictation loop.
            </p>
            <div className="w-full h-16 bg-[#f7fbff] vw-radius-tab border border-[#d8e4f2] p-2 flex items-end gap-[2px] overflow-hidden relative">
              {LATENCY_BAR_LEVELS.map((height, i) => {
                return <div key={i} className="flex-1 bg-[#c6d6e8] rounded-t-sm" style={{ height }} />
              })}
              <div className="absolute right-12 bottom-[4.5rem] bg-[#0b1224] text-white text-[10px] font-mono px-2 py-1 vw-radius-tab shadow-lg">LOCAL FLOW</div>
              <div className="absolute right-12 bottom-10 w-[1px] h-8 bg-[#0b1224]" />
            </div>
            <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-[#61758f]">
              Illustrative pipeline profile (not live telemetry).
            </p>
          </motion.div>

          <motion.div
            initial={{ opacity: 0 }}
            whileInView={{ opacity: 1 }}
            viewport={{ once: true, margin: '-80px' }}
            transition={{ duration: 0.55, delay: 0.08, ease: [0.16, 1, 0.3, 1] }}
            className="group relative z-20 w-full bg-[#f7fbff] vw-radius-shell p-6 sm:p-8 lg:p-10 shadow-[0_12px_44px_-18px_rgba(10,30,90,0.22)] border border-[#d4e2f2] transform-gpu transition-[border-color,box-shadow,transform] duration-300 hover:border-[#1b8eff]/58 hover:shadow-[0_20px_52px_-16px_rgba(27,142,255,0.26)]"
          >
            <div className="flex justify-between items-start mb-8 sm:mb-12">
              <div className="w-14 h-14 sm:w-16 sm:h-16 vw-radius-tab bg-[#FFFFFF] border border-[#d8e5f5] flex items-center justify-center shadow-sm transition-colors duration-300 group-hover:border-[#7ed8ff] group-hover:bg-[#ebf6ff]">
                <Bot className="w-7 h-7 sm:w-8 sm:h-8 text-[#1b8eff]" />
              </div>
              <span className="font-mono text-xs sm:text-sm font-bold bg-[#e5f3ff] text-[#0b3f98] px-3 py-1 rounded-full">Models / Verified</span>
            </div>
            <h3 className="font-display text-3xl sm:text-4xl font-bold text-[#09090B] mb-4">Local Models</h3>
            <p className="text-base sm:text-lg text-[#475569] font-medium leading-relaxed max-w-sm mb-8">
              Local models install with verification, then run directly in the desktop runtime.
            </p>
            <div className="space-y-3 font-mono text-sm border-t border-[#d7e4f3] pt-6">
              <div className="flex justify-between items-center bg-[#FFFFFF] p-3 vw-radius-tab border border-[#d8e5f5]"><span>fw-small.en</span><span className="font-bold">LOADED</span></div>
              <div className="flex justify-between items-center bg-[#e7f0f9] p-3 vw-radius-tab text-[#6c7f96]"><span>fw-large-v3</span><span>STANDBY</span></div>
            </div>
          </motion.div>

          <motion.div
            initial={{ opacity: 0 }}
            whileInView={{ opacity: 1 }}
            viewport={{ once: true, margin: '-80px' }}
            transition={{ duration: 0.55, delay: 0.16, ease: [0.16, 1, 0.3, 1] }}
            className="group relative z-30 w-full bg-[#09090B] vw-radius-shell p-6 sm:p-8 lg:p-10 shadow-[0_18px_56px_-20px_rgba(6,14,36,0.55)] border border-[#20345f] transform-gpu text-[#FAFAFA] transition-[border-color,box-shadow,transform] duration-300 hover:border-[#3f8dff] hover:shadow-[0_26px_70px_-20px_rgba(27,142,255,0.36)]"
          >
            <div className="flex justify-between items-start mb-8 sm:mb-12">
              <div className="w-14 h-14 sm:w-16 sm:h-16 vw-radius-tab bg-[#111a2b] border border-[#223b6f] flex items-center justify-center shadow-inner transition-colors duration-300 group-hover:border-[#7ed8ff]/55 group-hover:bg-[#0f2448]">
                <Server className="w-7 h-7 sm:w-8 sm:h-8 text-[#FAFAFA]" />
              </div>
              <span className="font-mono text-xs sm:text-sm font-bold bg-[#101c32] border border-[#28457f] text-[#bcd6ff] px-3 py-1 rounded-full">Privacy / Local-Only</span>
            </div>
            <h3 className="font-display text-3xl sm:text-4xl font-bold text-[#FFFFFF] mb-4">No Cloud Transcription</h3>
            <p className="text-base sm:text-lg text-[#b8c9de] font-medium leading-relaxed max-w-sm mb-10 sm:mb-12">
              Voice stays on-device in v1. Optional diagnostics export is user-triggered.
            </p>

            <PrivacyFlowDiagram />
          </motion.div>
        </div>
      </div>
    </section>
  )
}

export default function CapabilityDeepDive() {
  return <DeepDiveD />
}

