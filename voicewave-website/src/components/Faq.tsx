import { Plus } from 'lucide-react'
import { GITHUB_REPO_URL } from '../config/site'

const FAQS: Array<{ q: string; a: React.ReactNode }> = [
  {
    q: 'Is it really offline?',
    a: (
      <>
        Transcription is. Audio is captured, decoded by Whisper, and discarded on your
        machine. The app goes online for three things: a one-time speech model download
        during first-run setup, signed update checks against GitHub, and an anonymous
        usage count (random install ID, app version, update channel, and a few setup
        events, sent through TelemetryDeck; on by default, off with one switch). Never
        audio, text, or settings. You can verify all of it in the{' '}
        <a href={GITHUB_REPO_URL} target="_blank" rel="noreferrer" className="font-semibold text-[#0b3f98] underline decoration-[#9cc6f5] underline-offset-2 hover:text-[#0050d2]">
          source code
        </a>
        .
      </>
    )
  },
  {
    q: 'Windows says "Unknown publisher". Is it safe?',
    a: (
      <>
        The installer isn&apos;t Authenticode-signed yet, so Windows SmartScreen shows
        &quot;Windows protected your PC&quot; for new downloads. Click More info, then Run
        anyway. The installer is built in public by GitHub Actions from this repo, and
        you can read the{' '}
        <a href={GITHUB_REPO_URL} target="_blank" rel="noreferrer" className="font-semibold text-[#0b3f98] underline decoration-[#9cc6f5] underline-offset-2 hover:text-[#0050d2]">
          source and the build workflow
        </a>
        . Updates inside the app are signed and verified before they install.
      </>
    )
  },
  {
    q: 'How accurate is it?',
    a: "It runs OpenAI's Whisper, the same model family behind most modern dictation tools. Setup picks a model size that suits your PC, and the dictionary catches names and jargon."
  },
  {
    q: 'Do I need a GPU?',
    a: 'No. VoiceWave runs on the CPU by default. If you have a discrete GPU from NVIDIA, AMD or Intel with Vulkan 1.2, it uses the GPU automatically after a one-time check, with no CUDA or driver download. That is faster and fits a bigger model. PCs with only integrated graphics use the CPU for now.'
  },
  {
    q: 'What does the AI polish actually send anywhere?',
    a: 'Nothing. The polish pass is a small language model (Qwen2.5 1.5B, about a 1 GB download) running on your CPU. It is off by default, needs a CPU with AVX2, and a fidelity validator rejects any rewrite that changes what you said.'
  },
  {
    q: 'Which languages are supported?',
    a: 'English only for now. VoiceWave is built and tuned for English dictation, and the speech models it installs are English models.'
  },
  {
    q: 'What are the system requirements?',
    a: 'Windows 10 or 11, 64-bit, and no account. No Python or other dependencies. The installer does not include a speech model, so first-run setup downloads one once: about 488 MB (Small) up to 1.6 GB (Large Turbo), chosen for your PC. After that, dictation works fully offline.'
  },
  {
    q: 'Is it free? What is the catch?',
    a: 'There is no catch. VoiceWave is free and open source under Apache-2.0, with every feature included and no account or subscription.'
  }
]

export default function Faq() {
  return (
    <section id="faq" className="scroll-mt-28 py-14 sm:py-20">
      <div className="site-shell-tight">
        <div className="mx-auto max-w-3xl">
          <p className="section-eyebrow font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
            <span aria-hidden="true" className="section-eyebrow-tick" />
            Questions, answered
          </p>
          <h2 className="mt-4 font-display text-[clamp(1.9rem,4.6vw,3rem)] font-bold leading-[1.03] tracking-tight text-[#09090B]">
            The things people ask before installing.
          </h2>

          <div className="mt-10 divide-y divide-[#dde7f3] border-y border-[#dde7f3]">
            {FAQS.map((item) => (
              <details key={item.q} className="faq-item group">
                <summary className="flex cursor-pointer list-none items-center justify-between gap-4 py-5 text-left [&::-webkit-details-marker]:hidden">
                  <span className="text-base font-semibold text-[#0f172a] sm:text-lg">{item.q}</span>
                  <Plus
                    className="h-4 w-4 shrink-0 text-[#1b8eff] transition-transform duration-200 group-open:rotate-45"
                    aria-hidden="true"
                  />
                </summary>
                <div className="pb-6 pr-8 text-[15px] leading-relaxed text-[#475569]">{item.a}</div>
              </details>
            ))}
          </div>
        </div>
      </div>
    </section>
  )
}
