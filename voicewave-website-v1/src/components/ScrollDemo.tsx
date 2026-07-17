import { motion, useReducedMotion } from 'framer-motion'
import { Monitor } from 'lucide-react'
import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { Dashboard } from '../appReplica/components/Dashboard'
import { Layout } from '../appReplica/components/Layout'
import { THEME } from '../appReplica/constants'
import type { DictationState } from '../appReplica/types'
import '../appReplica/styles.css'
import { windowsDownloadUrl } from '../config/download'

type DemoSample = {
  id: string
  label: string
  partial: string
  final: string
}

const DEMO_SAMPLES: DemoSample[] = [
  {
    id: 'release',
    label: 'Release Update',
    partial: 'This week we closed the final release-readiness checks...',
    final: 'This week we closed the final release-readiness checks and verified reliability on the RC build.',
  },
  {
    id: 'coding',
    label: 'Coding Note',
    partial: 'Please keep the demo on the website homepage and use real app components...',
    final: 'Please keep the demo on the website homepage and use real app components so users see truthful behavior.',
  },
  {
    id: 'study',
    label: 'Study Summary',
    partial: 'Key idea one is to gate rollout by reliability and rollback safety...',
    final: 'Key idea one is to gate rollout by reliability and rollback safety before expanding the beta cohort.',
  },
]

type CorePane = 'home' | 'models' | 'dictionary' | 'pro'

function asCorePane(value: string): CorePane {
  if (value === 'models' || value === 'dictionary' || value === 'pro') {
    return value
  }
  return 'home'
}

export default function ScrollDemo() {
  const reducedMotion = useReducedMotion()
  const isReducedMotion = Boolean(reducedMotion)
  const [activeNav, setActiveNav] = useState<CorePane>('home')
  const [status, setStatus] = useState<DictationState>('idle')
  const [sampleIndex, setSampleIndex] = useState(0)
  const [partialTranscript, setPartialTranscript] = useState<string | null>(null)
  const [finalTranscript, setFinalTranscript] = useState<string | null>(null)
  const [recentSentences, setRecentSentences] = useState<
    Array<{
      id: string
      text: string
      createdAtUtcMs: number
    }>
  >([])
  const timeoutsRef = useRef<number[]>([])

  const sample = useMemo(() => DEMO_SAMPLES[sampleIndex] ?? DEMO_SAMPLES[0], [sampleIndex])
  const isRecording = status === 'listening' || status === 'transcribing'

  const clearAllTimeouts = useCallback(() => {
    for (const timeoutId of timeoutsRef.current) {
      window.clearTimeout(timeoutId)
    }
    timeoutsRef.current = []
  }, [])

  useEffect(() => () => clearAllTimeouts(), [clearAllTimeouts])

  const advanceSample = useCallback(() => {
    setSampleIndex((current) => (current + 1) % DEMO_SAMPLES.length)
  }, [])

  const startCapture = useCallback(() => {
    clearAllTimeouts()
    setStatus('listening')
    setPartialTranscript(sample.partial)
    setFinalTranscript(null)
  }, [clearAllTimeouts, sample.partial])

  const releaseCapture = useCallback(() => {
    clearAllTimeouts()
    setStatus('transcribing')
    setPartialTranscript(sample.partial)
    const insertedTimeout = window.setTimeout(() => {
      setStatus('inserted')
      setPartialTranscript(null)
      setFinalTranscript(sample.final)
      setRecentSentences((current) => [
        {
          id: `demo-${Date.now()}`,
          text: sample.final,
          createdAtUtcMs: Date.now(),
        },
        ...current,
      ].slice(0, 5))
    }, 540)

    const idleTimeout = window.setTimeout(() => {
      setStatus('idle')
      setPartialTranscript(null)
      advanceSample()
    }, 1900)

    timeoutsRef.current.push(insertedTimeout, idleTimeout)
  }, [advanceSample, clearAllTimeouts, sample.final, sample.partial])

  useEffect(() => {
    if (isReducedMotion || activeNav !== 'home') {
      return
    }

    let timeoutId: number | null = null
    if (status === 'idle') {
      timeoutId = window.setTimeout(() => startCapture(), 1200)
    } else if (status === 'listening') {
      timeoutId = window.setTimeout(() => releaseCapture(), 1300)
    }

    return () => {
      if (timeoutId !== null) {
        window.clearTimeout(timeoutId)
      }
    }
  }, [isReducedMotion, activeNav, status, startCapture, releaseCapture])

  const renderPane = () => {
    if (activeNav === 'home') {
      return (
        <Dashboard
          theme={THEME}
          status={status}
          onPressStart={startCapture}
          onPressEnd={releaseCapture}
          currentModel="fw-small.en"
          partialTranscript={partialTranscript}
          finalTranscript={finalTranscript}
          pushToTalkHotkey="Ctrl + Space"
          isPro={false}
          recentSentences={recentSentences}
        />
      )
    }

    if (activeNav === 'models') {
      return (
        <section className="mx-auto max-w-5xl space-y-4 pb-12">
          <h2 className="vw-section-heading text-3xl text-[#09090B]">Models</h2>
          <div className="grid gap-4 md:grid-cols-2">
            <article className="vw-surface-base rounded-3xl px-5 py-5">
              <p className="text-sm font-semibold text-[#09090B]">fw-small.en</p>
              <p className="mt-2 text-sm text-[#64748B]">Fast startup and low latency for everyday dictation.</p>
              <span className="vw-chip mt-3">Recommended for most users</span>
            </article>
            <article className="vw-surface-base rounded-3xl px-5 py-5">
              <p className="text-sm font-semibold text-[#09090B]">fw-large-v3</p>
              <p className="mt-2 text-sm text-[#64748B]">Higher quality at higher compute cost.</p>
              <span className="vw-chip mt-3">Best on high-performance hardware</span>
            </article>
          </div>
        </section>
      )
    }

    if (activeNav === 'dictionary') {
      return (
        <section className="mx-auto max-w-5xl space-y-4 pb-12">
          <h2 className="vw-section-heading text-3xl text-[#09090B]">Dictionary</h2>
          <article className="vw-surface-base rounded-3xl px-5 py-5">
            <p className="text-sm font-semibold text-[#09090B]">Approved terms</p>
            <ul className="mt-3 space-y-2 text-sm text-[#334155]">
              <li>Faster-Whisper</li>
              <li>VoiceWave</li>
              <li>TTFSD</li>
            </ul>
          </article>
        </section>
      )
    }

    return (
      <section className="mx-auto max-w-5xl space-y-4 pb-12">
        <h2 className="vw-section-heading text-3xl text-[#09090B]">Pro</h2>
        <article className="vw-surface-elevated rounded-3xl px-5 py-5">
          <p className="text-sm text-[#334155]">Public preview shows customer-facing Pro messaging only.</p>
          <a
            href={windowsDownloadUrl}
            target="_blank"
            rel="noreferrer"
            download
            className="vw-btn-primary mt-4 inline-flex"
          >
            Upgrade / Download
          </a>
        </article>
      </section>
    )
  }

  return (
    <section id="demo" className="px-0 py-10 sm:py-14">
      <div className="site-shell">
        <p className="section-eyebrow font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
          <span aria-hidden="true" className="section-eyebrow-tick" />
          See It In Action
        </p>
        <h2 className="mt-4 text-[clamp(2.1rem,5.2vw,3.85rem)] leading-[1.02] text-[#0a1020]">App-realistic live demo</h2>
        <p className="mt-3 max-w-3xl text-base text-[#475569] sm:text-lg">
          This preview uses the same app UI components with simulated input, so users see truthful VoiceWave behavior.
        </p>

        <motion.div
          initial={{ opacity: 0, y: isReducedMotion ? 0 : 18, scale: isReducedMotion ? 1 : 0.955 }}
          whileInView={{ opacity: 1, y: 0, scale: 1 }}
          viewport={{ once: true, amount: 0.22 }}
          transition={{ duration: isReducedMotion ? 0.01 : 0.62, ease: [0.16, 1, 0.3, 1], delay: isReducedMotion ? 0 : 0.08 }}
          className="panel-card mt-8 overflow-hidden border-[#d6e5f8]"
        >
          <div className="flex items-center justify-between gap-3 border-b border-[#d7e6f8] bg-[linear-gradient(132deg,rgba(247,251,255,0.98),rgba(233,244,255,0.95))] px-4 py-2.5">
            <span className="inline-flex items-center gap-1 rounded-full border border-[#cfe0f4] bg-white/90 px-2.5 py-1 font-mono text-[10px] font-bold uppercase tracking-[0.1em] text-[#2a4261]">
              Simulated UI
            </span>
            <span className="inline-flex items-center gap-1 rounded-full border border-[#cfe0f4] bg-white/90 px-2.5 py-1 font-mono text-[10px] font-bold uppercase tracking-[0.1em] text-[#2a4261]">
              State: {status}
            </span>
          </div>

          {/* Mobile placeholder */}
          <div className="md:hidden rounded-2xl border border-[#d6e5f8] bg-[linear-gradient(135deg,rgba(247,251,255,0.98),rgba(233,244,255,0.95))] p-8 text-center">
            <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full bg-[#e8f1ff]">
              <Monitor className="h-7 w-7 text-[#0a3eb8]" />
            </div>
            <p className="font-semibold text-[#0a1020] text-base">Interactive Demo</p>
            <p className="mt-2 text-sm text-[#475569] leading-relaxed">Best experienced on a larger screen. Open VoiceWave on desktop to see the full app UI in action.</p>
          </div>

          {/* Desktop demo
             * Container height is the VISUAL height (after scale 0.8 is applied).
             * Inner Layout renders at its natural width & height, then gets
             * scaled to 80%. With the updated TODAY list the natural content
             * is roughly 900px tall, so visual height = 900 * 0.8 = 720.
             * Keep overflow hidden so the card doesn't leak. */}
          <div className="hidden md:block" style={{ overflow: 'hidden', height: '720px' }}>
            <div style={{ transform: 'scale(0.8)', transformOrigin: 'top left', width: 'calc(100% / 0.8)' }}>
              <Layout
                theme={THEME}
                activeNav={activeNav}
                setActiveNav={(next) => setActiveNav(asCorePane(next))}
                isRecording={isRecording}
                isPro={false}
                showProTools={false}
                profileDisplayName="VoiceWave Demo"
                profileStatusLabel="Public preview"
                isProfileAuthenticated={false}
              >
                {renderPane()}
              </Layout>
            </div>
          </div>
        </motion.div>
      </div>
    </section>
  )
}
