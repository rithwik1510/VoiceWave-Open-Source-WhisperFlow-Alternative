import { AnimatePresence, motion } from 'framer-motion'
import { useEffect, useRef, useState } from 'react'
import { ArrowRight, Check, Copy } from 'lucide-react'
import { windowsDownloadUrl } from '../config/download'
import HeroDottedField, { type HeroSafeZone } from './HeroDottedField'

const WINGET_COMMAND = 'winget install VoiceWave.LocalCore'

const HERO_SUBTEXTS = [
  'Built for fast on-device dictation. No cloud path. Everything stays local.',
  'Press. Speak. Release. Text appears in your active app in under 2 seconds.',
  'Whisper runs on your CPU or GPU. Your audio never leaves this machine.',
  'Hold to talk. Release to insert. Local by design, fast by default.',
]

// Short positioning markers under the subtext. Keeping this brand-forward
// (not metric-forward) so the hero reads as a product, not a spec sheet.
const HERO_POINTS = ['Windows', 'Free preview', 'Offline-first']

export default function Hero() {
  const [subtextIdx, setSubtextIdx] = useState(0)
  const heroSectionRef = useRef<HTMLElement | null>(null)
  const copyStackRef = useRef<HTMLDivElement | null>(null)
  const [safeZone, setSafeZone] = useState<HeroSafeZone | null>(null)
  const [topCutoffPx, setTopCutoffPx] = useState(0)
  const [wingetCopied, setWingetCopied] = useState(false)

  const copyWingetCommand = async () => {
    try {
      await navigator.clipboard.writeText(WINGET_COMMAND)
      setWingetCopied(true)
      window.setTimeout(() => setWingetCopied(false), 1800)
    } catch {
      // Clipboard API unavailable (very old browsers / iframe). Silent fall-through;
      // user can still select-and-copy from the visible text.
    }
  }

  useEffect(() => {
    const heroSection = heroSectionRef.current
    const copyStack = copyStackRef.current

    if (!heroSection || !copyStack) {
      return
    }

    const header = heroSection.parentElement?.querySelector('header')
    let frameId = 0

    const computeLayout = () => {
      frameId = 0
      const heroRect = heroSection.getBoundingClientRect()
      const copyRect = copyStack.getBoundingClientRect()
      if (heroRect.width <= 0 || heroRect.height <= 0) {
        return
      }

      const centerX = copyRect.left - heroRect.left + copyRect.width * 0.5
      const centerY = copyRect.top - heroRect.top + copyRect.height * 0.5
      const radiusFromWidth = copyRect.width * 0.34
      const radiusFromHeight = copyRect.height * 0.62
      const unclampedRadius = Math.max(radiusFromWidth, radiusFromHeight) + 8
      const radius = Math.min(unclampedRadius, heroRect.width * 0.24)

      const nextSafeZone: HeroSafeZone = {
        centerX,
        centerY,
        radius
      }

      setSafeZone((previous) => {
        const previousRadius = previous?.radius ?? 0
        const nextRadius = nextSafeZone.radius ?? 0

        if (
          previous &&
          Math.abs(previous.centerX - nextSafeZone.centerX) < 0.5 &&
          Math.abs(previous.centerY - nextSafeZone.centerY) < 0.5 &&
          Math.abs(previousRadius - nextRadius) < 0.5
        ) {
          return previous
        }
        return nextSafeZone
      })

      const navBottom = header?.getBoundingClientRect().bottom ?? heroRect.top
      const cutoff = Math.max(0, navBottom - heroRect.top + 2)
      setTopCutoffPx((previous) => (Math.abs(previous - cutoff) < 0.5 ? previous : cutoff))
    }

    const scheduleCompute = () => {
      if (frameId !== 0) {
        return
      }
      frameId = window.requestAnimationFrame(computeLayout)
    }

    scheduleCompute()

    const prefersReduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    let subtextInterval = 0
    if (!prefersReduced) {
      subtextInterval = window.setInterval(() => {
        setSubtextIdx(i => (i + 1) % HERO_SUBTEXTS.length)
      }, 3600)
    }

    const resizeObserver = new ResizeObserver(() => {
      scheduleCompute()
    })

    resizeObserver.observe(heroSection)
    resizeObserver.observe(copyStack)
    if (header instanceof HTMLElement) {
      resizeObserver.observe(header)
    }

    window.addEventListener('resize', scheduleCompute, { passive: true })

    return () => {
      if (frameId !== 0) {
        window.cancelAnimationFrame(frameId)
      }
      if (subtextInterval !== 0) {
        window.clearInterval(subtextInterval)
      }
      resizeObserver.disconnect()
      window.removeEventListener('resize', scheduleCompute)
    }
  }, [])

  return (
    <section
      id="home"
      ref={heroSectionRef}
      className="relative min-h-[92svh] overflow-hidden pb-16 pt-4 sm:min-h-[96svh] sm:pb-20 sm:pt-8 md:min-h-[102svh] md:pb-28 md:pt-10"
    >
      <div className="zone-hero-dotted-layer">
        <HeroDottedField theme="dark" safeZone={safeZone ?? undefined} topCutoffPx={topCutoffPx} />
      </div>

      <div className="site-shell relative z-10 flex min-h-[74svh] flex-col items-center justify-center text-center text-white md:min-h-[80svh]">
        <div ref={copyStackRef} className="hero-copy-stack">
          <h1 className="hero-title-copy max-w-5xl text-balance text-[clamp(3.3rem,10vw,7rem)] leading-[0.9] text-white">
            Private Dictation.
          </h1>

          <div className="relative mt-5 flex min-h-[3.6rem] max-w-xl items-center justify-center">
            <AnimatePresence mode="wait">
              <motion.p
                key={subtextIdx}
                initial={{ opacity: 0, y: 7 }}
                animate={{ opacity: 1, y: 0 }}
                exit={{ opacity: 0, y: -7 }}
                transition={{ duration: 0.38, ease: [0.16, 1, 0.3, 1] }}
                className="hero-body-copy text-pretty text-[clamp(0.78rem,1.9vw,1rem)] leading-relaxed text-[#d7ecff]"
              >
                {HERO_SUBTEXTS[subtextIdx]}
              </motion.p>
            </AnimatePresence>
          </div>

          <ul className="mt-4 flex flex-wrap items-center justify-center gap-x-4 gap-y-1.5 text-[11px] font-semibold uppercase tracking-[0.12em] text-[#dff0ff] sm:text-xs">
            {HERO_POINTS.map((point) => (
              <li key={point} className="inline-flex items-center gap-1.5">
                <span className="inline-block h-1 w-1 rounded-full bg-[#7ed8ff]" />
                {point}
              </li>
            ))}
          </ul>

          <a
            href={windowsDownloadUrl}
            target="_blank"
            rel="noreferrer"
            download
            className="lime-cta pointer-events-auto mt-8 px-6 py-2.5"
          >
            Download Setup
            <ArrowRight className="ml-2 h-3.5 w-3.5" />
          </a>

          <button
            type="button"
            onClick={copyWingetCommand}
            aria-label={
              wingetCopied
                ? 'winget command copied to clipboard'
                : `Copy winget install command: ${WINGET_COMMAND}`
            }
            className="hero-winget-strip pointer-events-auto mt-4 inline-flex items-center gap-2 rounded-full border border-white/15 bg-white/[0.06] px-3.5 py-1.5 backdrop-blur-sm transition-colors duration-200 hover:border-white/30 hover:bg-white/[0.1] sm:gap-2.5 sm:px-4"
          >
            <span className="font-mono text-[9px] uppercase tracking-[0.2em] text-[#9fbedd] sm:text-[10px]">
              or
            </span>
            <code className="font-mono text-[11px] font-medium text-[#e6f1ff] sm:text-xs">
              {WINGET_COMMAND}
            </code>
            {wingetCopied ? (
              <Check className="h-3 w-3 text-[#bef264]" aria-hidden="true" />
            ) : (
              <Copy className="h-3 w-3 text-[#9fbedd]" aria-hidden="true" />
            )}
          </button>
        </div>
      </div>
    </section>
  )
}

