import { AnimatePresence, LayoutGroup, motion, useReducedMotion } from 'framer-motion'
import { Cpu, ShieldCheck, Sparkles, ToggleLeft } from 'lucide-react'
import { useEffect, useRef, useState } from 'react'

// The polish pass, shown as it actually behaves: the raw transcript appears
// word by word, then filler words drop out and the survivors slide together
// into the polished sentence. One sentence, transforming in place.

type Token = {
  heard: string
  /** undefined = survives unchanged, null = filler (drops), string = survives with this fix */
  polished?: string | null
}

const SAMPLES: Token[][] = [
  [
    { heard: 'um', polished: null },
    { heard: 'so', polished: null },
    { heard: 'basically', polished: null },
    { heard: 'we', polished: 'We' },
    { heard: 'should' },
    { heard: 'uh', polished: null },
    { heard: 'ship' },
    { heard: 'the' },
    { heard: 'the', polished: null },
    { heard: 'release' },
    { heard: 'on' },
    { heard: 'friday', polished: 'Friday.' },
    { heard: 'i', polished: null },
    { heard: 'think', polished: null }
  ],
  [
    { heard: 'hey', polished: 'Hey,' },
    { heard: 'can' },
    { heard: 'you' },
    { heard: 'er', polished: null },
    { heard: 'send' },
    { heard: 'me' },
    { heard: 'the' },
    { heard: 'the', polished: null },
    { heard: 'figma', polished: 'Figma' },
    { heard: 'link' },
    { heard: 'when' },
    { heard: 'you' },
    { heard: 'get' },
    { heard: 'a' },
    { heard: 'chance', polished: 'chance?' },
    { heard: 'thanks', polished: 'Thanks.' }
  ],
  [
    { heard: 'note', polished: 'Note' },
    { heard: 'to' },
    { heard: 'self', polished: 'self:' },
    { heard: 'um', polished: null },
    { heard: 'review' },
    { heard: 'the' },
    { heard: 'pull' },
    { heard: 'request' },
    { heard: 'before' },
    { heard: 'standup' },
    { heard: 'tomorrow' },
    { heard: 'morning', polished: 'morning.' }
  ]
]

const GUARANTEES = [
  { icon: Cpu, title: 'Runs on your CPU', body: 'A small local model. No API key, no cloud.' },
  {
    icon: ShieldCheck,
    title: 'Never changes your meaning',
    body: 'A validator checks every polish. If it drifts, you get the raw transcript.'
  },
  { icon: ToggleLeft, title: 'Off by default', body: 'Opt in from Settings.' }
]

type Phase = 'heard' | 'polishing' | 'polished'

export default function PolishSpotlight() {
  const reducedMotion = Boolean(useReducedMotion())
  const [sampleIdx, setSampleIdx] = useState(0)
  const [phase, setPhase] = useState<Phase>(reducedMotion ? 'polished' : 'heard')
  const timerRef = useRef<ReturnType<typeof setTimeout> | null>(null)

  const tokens = SAMPLES[sampleIdx]

  useEffect(() => {
    if (reducedMotion) {
      return
    }
    const durations: Record<Phase, number> = {
      heard: 1400 + tokens.length * 90,
      polishing: 900,
      polished: 3400
    }
    timerRef.current = setTimeout(() => {
      setPhase((current) => {
        if (current === 'heard') return 'polishing'
        if (current === 'polishing') return 'polished'
        setSampleIdx((idx) => (idx + 1) % SAMPLES.length)
        return 'heard'
      })
    }, durations[phase])
    return () => {
      if (timerRef.current) clearTimeout(timerRef.current)
    }
  }, [phase, reducedMotion, tokens.length])

  const showPolished = phase === 'polished'

  return (
    <section id="polish" className="scroll-mt-28 py-16 sm:py-24">
      <div className="site-shell grid items-center gap-10 lg:grid-cols-12 lg:gap-14">
        <div className="lg:col-span-5">
          <p className="section-eyebrow font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
            <span aria-hidden="true" className="section-eyebrow-tick" />
            On-device AI polish
          </p>
          <h2 className="mt-4 font-display text-[clamp(2rem,4.6vw,3.6rem)] font-bold leading-[1.02] tracking-tight text-[#09090B]">
            A tiny editor,
            <br />
            <span className="text-[#5b7392]">living on your machine.</span>
          </h2>
          <p className="mt-5 max-w-md text-base leading-relaxed text-[#475569] sm:text-lg">
            Speech is messy. A local language model cleans it up before the text lands —
            the feature others put behind a subscription, running free on your CPU.
          </p>

          <div className="mt-8 space-y-4">
            {GUARANTEES.map((item) => (
              <div key={item.title} className="flex items-start gap-3.5">
                <span className="mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-full border border-[#d8e5f5] bg-white text-[#1b8eff]">
                  <item.icon className="h-4 w-4" aria-hidden="true" />
                </span>
                <div>
                  <p className="text-sm font-bold text-[#09090B]">{item.title}</p>
                  <p className="mt-0.5 text-sm leading-relaxed text-[#475569]">{item.body}</p>
                </div>
              </div>
            ))}
          </div>
        </div>

        <div className="lg:col-span-7">
          <motion.div
            initial={{ opacity: 0, y: 18 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, amount: 0.3 }}
            transition={{ duration: 0.55, ease: [0.16, 1, 0.3, 1] }}
            className="panel-card relative overflow-hidden p-6 sm:p-8"
          >
            <div className="flex items-center justify-between">
              <span className="inline-flex items-center gap-1.5 rounded-full border border-[#cfe0f4] bg-white/90 px-2.5 py-1 font-mono text-[10px] font-bold uppercase tracking-[0.1em] text-[#2a4261]">
                <Sparkles
                  className={`h-3 w-3 text-[#1b8eff] ${phase === 'polishing' ? 'animate-pulse' : ''}`}
                  aria-hidden="true"
                />
                Polish pass
              </span>
              <span className="font-mono text-[10px] font-bold uppercase tracking-[0.1em] text-[#61758f]">
                {phase === 'heard' ? 'listening' : phase === 'polishing' ? 'polishing…' : 'inserted'}
              </span>
            </div>

            {/* The sentence, transforming in place */}
            <div className="mt-8 flex min-h-[10rem] items-center sm:min-h-[9rem]">
              <LayoutGroup>
                <p className="flex flex-wrap items-baseline gap-x-[0.45em] gap-y-2 text-lg leading-relaxed sm:text-2xl">
                  <AnimatePresence mode="popLayout">
                    {tokens.map((token, idx) => {
                      const isFiller = token.polished === null
                      if (showPolished && isFiller) {
                        return null
                      }
                      const text = showPolished ? (token.polished ?? token.heard) : token.heard
                      const highlighted = phase === 'polishing' && isFiller
                      return (
                        <motion.span
                          key={`${sampleIdx}-${idx}`}
                          layout
                          initial={{ opacity: 0, y: 8 }}
                          animate={{
                            opacity: highlighted ? 0.38 : 1,
                            y: 0,
                            transition: {
                              delay: phase === 'heard' && !reducedMotion ? idx * 0.09 : 0,
                              duration: 0.32,
                              ease: [0.16, 1, 0.3, 1]
                            }
                          }}
                          exit={{ opacity: 0, y: 10, scale: 0.86, transition: { duration: 0.3 } }}
                          className={
                            showPolished
                              ? 'font-medium text-[#09090B]'
                              : `font-mono text-[0.88em] ${highlighted ? 'text-[#b3bfd1] line-through' : 'text-[#41556e]'}`
                          }
                        >
                          {text}
                        </motion.span>
                      )
                    })}
                  </AnimatePresence>
                </p>
              </LayoutGroup>
            </div>

            <p className="mt-6 text-center font-mono text-[9px] uppercase tracking-[0.15em] text-[#8ba2bb]">
              Real polish-pass behavior &middot; validator-gated &middot; sample {sampleIdx + 1} of {SAMPLES.length}
            </p>
          </motion.div>
        </div>
      </div>
    </section>
  )
}
