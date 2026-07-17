import { Plus } from 'lucide-react'
import { GITHUB_REPO_URL } from '../config/site'

const FAQS: Array<{ q: string; a: React.ReactNode }> = [
  {
    q: 'Is it really 100% offline?',
    a: (
      <>
        Transcription is, yes — audio is captured, decoded by Whisper, and discarded
        entirely on your machine. The only network calls in the app are downloading
        models and checking GitHub for signed updates, and you can verify that in the{' '}
        <a href={GITHUB_REPO_URL} target="_blank" rel="noreferrer" className="font-semibold text-[#0b3f98] underline decoration-[#9cc6f5] underline-offset-2 hover:text-[#0050d2]">
          source code
        </a>
        .
      </>
    )
  },
  {
    q: 'How accurate is it?',
    a: 'It runs OpenAI\'s Whisper via faster-whisper — the same model family behind most modern dictation tools. Install larger models for more accuracy; the dictionary catches names and jargon.'
  },
  {
    q: 'Do I need a GPU?',
    a: 'No. Everything ships CPU-first and the default model is quick on any modern processor. If you have a capable NVIDIA GPU, larger models get faster — GPU acceleration as a one-click download is on the roadmap.'
  },
  {
    q: 'What does the AI polish actually send anywhere?',
    a: 'Nothing. The polish pass is a small language model running on your CPU, in the same process ecosystem as transcription. It\'s off by default, and a fidelity validator rejects any rewrite that changes what you said.'
  },
  {
    q: 'Which languages are supported?',
    a: 'English is the tuned, default experience today. Whisper itself is multilingual, and larger multilingual models can be installed from Settings — quality varies by language.'
  },
  {
    q: 'What are the system requirements?',
    a: 'Windows 10 or 11, 64-bit. No account, no Python, no dependencies — the installer bundles everything, including the offline transcription runtime.'
  },
  {
    q: 'Is it free? What\'s the catch?',
    a: 'The app you download today is free and open source under Apache-2.0, and the core local dictation loop will stay that way. If advanced pro features arrive later, they\'ll be additions — not a paywall in front of what you already have.'
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
