import { motion } from 'framer-motion'
import { ClipboardCheck, Cpu, Mic } from 'lucide-react'

const runtimeSteps = [
  {
    icon: Mic,
    title: 'Capture',
    text: 'Microphone input is captured locally with deterministic state visibility.'
  },
  {
    icon: Cpu,
    title: 'Decode',
    text: 'whisper.cpp runs on-device via the Rust runtime path used by the desktop app.'
  },
  {
    icon: ClipboardCheck,
    title: 'Insert',
    text: 'Insertion uses a reliability chain with clipboard and history-safe fallback behavior.'
  }
]

export default function RuntimeProof() {
  return (
    <section id="runtime" className="scroll-mt-28 py-20 md:py-24 px-6 bg-transparent relative overflow-hidden">
      <div className="max-w-7xl mx-auto relative z-10">
        <div className="max-w-3xl">
          <div className="inline-flex items-center gap-2 border-b border-[#09090B] pb-3">
            <span className="w-2 h-2 rounded-full bg-[#38BDF8]" />
            <span className="font-mono text-xs font-bold uppercase tracking-widest text-[#52525B]">Specific Runtime</span>
          </div>

          <h2 className="mt-6 font-display text-5xl md:text-6xl leading-[0.95] tracking-tight text-[#09090B]">
            No generic AI black box,
            <span className="text-gradient"> just a clear local pipeline.</span>
          </h2>
        </div>

        <div className="mt-10 grid gap-4 md:grid-cols-3">
          {runtimeSteps.map((step, index) => {
            const Icon = step.icon
            return (
              <motion.article
                key={step.title}
                initial={{ opacity: 0, y: 24 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true, margin: '-100px' }}
                transition={{ duration: 0.55, delay: index * 0.08, ease: [0.16, 1, 0.3, 1] }}
                className="rounded-[28px] border border-[#E4E4E7] bg-[#FFFFFF]/92 p-6 shadow-[0_20px_50px_-38px_rgba(9,9,11,0.25)]"
              >
                <span className="inline-flex h-11 w-11 items-center justify-center rounded-xl bg-[#F4F4F5] text-[#18181B]">
                  <Icon size={20} />
                </span>
                <p className="mt-4 font-display text-3xl tracking-tight text-[#09090B]">{step.title}</p>
                <p className="mt-2 text-sm leading-relaxed text-[#475569]">{step.text}</p>
              </motion.article>
            )
          })}
        </div>

        <div className="mt-8 rounded-2xl border border-[#E4E4E7] bg-[#FFFFFF]/92 p-4 md:p-5">
          <div className="flex flex-wrap gap-2">
            <span className="rounded-full border border-[#D4D4D8] bg-[#F4F4F5] px-3 py-1 font-mono text-[11px] font-bold uppercase tracking-[0.12em] text-[#52525B]">v1 local-only</span>
            <span className="rounded-full border border-[#D4D4D8] bg-[#F4F4F5] px-3 py-1 font-mono text-[11px] font-bold uppercase tracking-[0.12em] text-[#52525B]">whisper.cpp runtime</span>
            <span className="rounded-full border border-[#D4D4D8] bg-[#F4F4F5] px-3 py-1 font-mono text-[11px] font-bold uppercase tracking-[0.12em] text-[#52525B]">fallback-safe insertion</span>
          </div>
          <p className="mt-3 text-xs text-[#64748B]">
            Current execution scope since February 10, 2026: Windows implementation and validation.
          </p>
        </div>
      </div>
    </section>
  )
}
