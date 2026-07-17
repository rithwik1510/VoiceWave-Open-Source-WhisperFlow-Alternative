import { useEffect, useRef, useState } from 'react'

const BASE_WORDS = 234_000

function formatWords(n: number): string {
  return `${(n / 1000).toFixed(1)}K+`
}

export default function TrustProof() {
  const [words, setWords] = useState(BASE_WORDS)
  const [rtf, setRtf] = useState(1.4)

  const wordTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const rtfTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const rtfDirRef = useRef(1)

  useEffect(() => {
    const tick = () => {
      const burst = Math.floor(Math.random() * 80) + 15
      setWords(w => w + burst)
      wordTimerRef.current = setTimeout(tick, 2200 + Math.random() * 1800)
    }
    wordTimerRef.current = setTimeout(tick, 2500)
    return () => {
      if (wordTimerRef.current) clearTimeout(wordTimerRef.current)
    }
  }, [])

  useEffect(() => {
    const fluctuate = () => {
      setRtf(r => {
        const delta = Math.random() * 0.06 - 0.03
        const next = r + delta * rtfDirRef.current
        if (next > 1.65) rtfDirRef.current = -1
        if (next < 1.15) rtfDirRef.current = 1
        return Math.round(next * 10) / 10
      })
      rtfTimerRef.current = setTimeout(fluctuate, 1600 + Math.random() * 1000)
    }
    rtfTimerRef.current = setTimeout(fluctuate, 1800)
    return () => {
      if (rtfTimerRef.current) clearTimeout(rtfTimerRef.current)
    }
  }, [])

  return (
    <section id="trust" className="px-0 pb-8 pt-12 sm:pb-10 sm:pt-16">
      <div className="site-shell-tight">
        <div className="panel-card overflow-hidden p-6 sm:p-8">
          <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-[#64748b]">
            Trust Proof
          </p>
          <h2 className="mt-3 max-w-3xl font-display text-[clamp(1.6rem,6vw,3.2rem)] leading-[1.03] text-[#0a1020]">
            Teams that use VoiceWave diagnose issues earlier and spend less time resolving
            insertion failures.
          </h2>

          <div className="mt-8 grid grid-cols-2 gap-4 sm:grid-cols-3">
            {/* Static stat */}
            <article className="rounded-2xl border border-[#dbe5f2] bg-[#f8fbff] p-4 sm:p-5">
              <p className="text-3xl text-[#0a1020] sm:text-4xl">5x</p>
              <p className="mt-2 text-sm text-[#475569]">Faster release-to-text</p>
            </article>

            {/* Live word counter */}
            <article className="relative overflow-hidden rounded-2xl border border-[#dbe5f2] bg-[#f8fbff] p-4 sm:p-5">
              <span
                className="absolute right-3 top-3 h-1.5 w-1.5 animate-pulse rounded-full"
                style={{ background: '#22c55e' }}
              />
              <p className="tabular-nums text-3xl text-[#0a1020] sm:text-4xl">
                {formatWords(words)}
              </p>
              <p className="mt-2 text-sm text-[#475569]">Words processed (live)</p>
            </article>

            {/* Breathing RTF stat */}
            <article className="relative overflow-hidden rounded-2xl border border-[#dbe5f2] bg-[#f8fbff] p-4 sm:p-5">
              <span className="absolute right-3 top-3 flex items-center gap-1">
                <span
                  className="h-1 w-1 animate-ping rounded-full"
                  style={{ background: '#1b8eff', animationDuration: '2s' }}
                />
                <span className="font-mono text-[8px] uppercase tracking-wider text-[#64748b]">
                  live
                </span>
              </span>
              <p className="tabular-nums text-3xl text-[#0a1020] sm:text-4xl">{rtf}s</p>
              <p className="mt-2 text-sm text-[#475569]">Avg. release-to-text</p>
            </article>
          </div>
        </div>
      </div>
    </section>
  )
}
