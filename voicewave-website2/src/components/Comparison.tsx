import { motion } from 'framer-motion'
import { Check, Minus, X } from 'lucide-react'
import VoiceWaveLogo from './VoiceWaveLogo'

type CellValue = {
  kind: 'yes' | 'no' | 'partial'
  note?: string
}

type CompareRow = {
  label: string
  voicewave: CellValue
  wispr: CellValue
  superwhisper: CellValue
  windows: CellValue
}

const ROWS: CompareRow[] = [
  {
    label: 'Price',
    voicewave: { kind: 'yes', note: 'Free' },
    wispr: { kind: 'partial', note: '~$15/mo' },
    superwhisper: { kind: 'partial', note: 'Freemium + Pro' },
    windows: { kind: 'yes', note: 'Built-in' }
  },
  {
    label: 'Audio stays on your device',
    voicewave: { kind: 'yes', note: 'Always' },
    wispr: { kind: 'no', note: 'Cloud servers' },
    superwhisper: { kind: 'partial', note: 'Local + optional cloud models' },
    windows: { kind: 'no', note: 'Microsoft speech services' }
  },
  {
    label: 'Works fully offline',
    voicewave: { kind: 'yes' },
    wispr: { kind: 'no' },
    superwhisper: { kind: 'yes' },
    windows: { kind: 'no' }
  },
  {
    label: 'Open source',
    voicewave: { kind: 'yes', note: 'Apache-2.0' },
    wispr: { kind: 'no' },
    superwhisper: { kind: 'no' },
    windows: { kind: 'no' }
  },
  {
    label: 'Runs on Windows',
    voicewave: { kind: 'yes', note: 'Native' },
    wispr: { kind: 'yes' },
    superwhisper: { kind: 'no', note: 'macOS / iOS only' },
    windows: { kind: 'yes' }
  },
  {
    label: 'AI cleanup of filler & punctuation',
    voicewave: { kind: 'yes', note: 'On-device, opt-in' },
    wispr: { kind: 'yes', note: 'In the cloud' },
    superwhisper: { kind: 'yes' },
    windows: { kind: 'partial', note: 'Basic punctuation' }
  },
  {
    label: 'Custom dictionary',
    voicewave: { kind: 'yes', note: 'With export/import' },
    wispr: { kind: 'yes' },
    superwhisper: { kind: 'yes' },
    windows: { kind: 'no' }
  }
]

function Cell({ value, highlight = false }: { value: CellValue; highlight?: boolean }) {
  const icon =
    value.kind === 'yes' ? (
      <Check className={`h-4 w-4 ${highlight ? 'text-[#0050d2]' : 'text-[#2e9e5b]'}`} aria-label="Yes" />
    ) : value.kind === 'no' ? (
      <X className="h-4 w-4 text-[#c2506a]" aria-label="No" />
    ) : (
      <Minus className="h-4 w-4 text-[#8ba2bb]" aria-label="Partial" />
    )

  return (
    <div className="flex flex-col items-center gap-1 text-center">
      {icon}
      {value.note ? (
        <span className={`text-[11px] leading-tight ${highlight ? 'font-semibold text-[#0b3f98]' : 'text-[#64748b]'}`}>
          {value.note}
        </span>
      ) : null}
    </div>
  )
}

export default function Comparison() {
  return (
    <section id="compare" className="scroll-mt-28 py-16 sm:py-24">
      <div className="site-shell">
        <div className="max-w-2xl">
          <p className="section-eyebrow font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
            <span aria-hidden="true" className="section-eyebrow-tick" />
            An honest comparison
          </p>
          <h2 className="mt-4 font-display text-[clamp(2rem,5vw,3.6rem)] font-bold leading-[1.02] tracking-tight text-[#09090B]">
            The tools are good.
            <br />
            <span className="text-[#5b7392]">The trade-offs differ.</span>
          </h2>
          <p className="mt-4 max-w-xl text-base leading-relaxed text-[#475569] sm:text-lg">
            Wispr Flow and SuperWhisper are polished products. VoiceWave covers the
            corner they don't: Windows, fully offline, free.
          </p>
        </div>

        <motion.div
          initial={{ opacity: 0, y: 18 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, amount: 0.2 }}
          transition={{ duration: 0.55, ease: [0.16, 1, 0.3, 1] }}
          className="panel-card mt-10 overflow-x-auto"
        >
          <table className="w-full min-w-[720px] border-collapse text-sm">
            <thead>
              <tr className="border-b border-[#dbe5f2]">
                <th className="px-4 py-4 text-left font-mono text-[10px] font-bold uppercase tracking-[0.16em] text-[#61758f]">
                  July 2026
                </th>
                <th className="rounded-t-2xl bg-[#eef6ff] px-4 py-4">
                  <span className="flex flex-col items-center gap-1.5">
                    <span className="flex h-7 w-7 items-center justify-center rounded-full bg-[#09090B]">
                      <VoiceWaveLogo size={10} strokeWidth={2.7} tone="adaptive" adaptiveOn="dark" />
                    </span>
                    <span className="font-display text-base font-bold tracking-tight text-[#09090B]">VoiceWave</span>
                  </span>
                </th>
                <th className="px-4 py-4 text-center font-display text-base font-bold tracking-tight text-[#3b4c63]">
                  Wispr Flow
                </th>
                <th className="px-4 py-4 text-center font-display text-base font-bold tracking-tight text-[#3b4c63]">
                  SuperWhisper
                </th>
                <th className="px-4 py-4 text-center font-display text-base font-bold tracking-tight text-[#3b4c63]">
                  Windows Voice Typing
                </th>
              </tr>
            </thead>
            <tbody>
              {ROWS.map((row, idx) => (
                <tr key={row.label} className={idx % 2 === 0 ? 'bg-white/50' : 'bg-[#f8fbff]/70'}>
                  <td className="px-4 py-4 font-medium text-[#0f172a]">{row.label}</td>
                  <td className="bg-[#eef6ff] px-4 py-4">
                    <Cell value={row.voicewave} highlight />
                  </td>
                  <td className="px-4 py-4">
                    <Cell value={row.wispr} />
                  </td>
                  <td className="px-4 py-4">
                    <Cell value={row.superwhisper} />
                  </td>
                  <td className="px-4 py-4">
                    <Cell value={row.windows} />
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </motion.div>

        <p className="mt-4 max-w-2xl font-mono text-[10px] uppercase tracking-[0.12em] leading-relaxed text-[#8ba2bb]">
          Based on public docs and pricing pages, July 2026. Spot something outdated?
          Open an issue — we'll fix it.
        </p>
      </div>
    </section>
  )
}
