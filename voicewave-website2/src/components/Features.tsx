import { motion } from 'framer-motion'
import {
  BookMarked,
  ClipboardCopy,
  LineChart,
  MicOff,
  RefreshCcw,
  WandSparkles
} from 'lucide-react'
import VoiceWaveLogo from './VoiceWaveLogo'

const features = [
  {
    icon: <WandSparkles className="w-6 h-6" />,
    title: 'Spoken Edit Commands',
    description:
      'Say "new line" or "bullet point" mid-sentence and get the formatting, not the words. Always on.',
    spec: 'Inline commands -> formatted text',
    badge: 'New'
  },
  {
    icon: <ClipboardCopy className="w-6 h-6" />,
    title: 'Lands In Every App',
    description:
      'Detects the focused app and picks the right insertion strategy — editors, browsers, chats, even terminals.',
    spec: 'Direct -> paste -> clipboard fallback'
  },
  {
    icon: <LineChart className="w-6 h-6" />,
    title: 'History & Stats',
    description:
      'Searchable history and a stats dashboard, all stored on your device. Keep it or wipe it.',
    spec: 'Stored locally, delete anytime',
    badge: 'New'
  },
  {
    icon: <BookMarked className="w-6 h-6" />,
    title: 'Custom Dictionary',
    description:
      'Teach it your names and jargon once — right every time after. Portable between machines.',
    spec: 'Your terms, portable'
  },
  {
    icon: <MicOff className="w-6 h-6" />,
    title: 'Mic-Volume Guard',
    description:
      'A too-quiet mic gets flagged before you lose a take — never silent empty text.',
    spec: 'Bad takes flagged, not swallowed'
  },
  {
    icon: <RefreshCcw className="w-6 h-6" />,
    title: 'Signed Auto-Updates',
    description:
      'One-click updates from GitHub, cryptographically verified before they touch your machine.',
    spec: 'Verified releases via GitHub'
  }
]

function FeaturesD() {
  return (
    <section id="features" className="section-pad-tight relative scroll-mt-28 overflow-hidden bg-transparent px-0">
      <div className="absolute left-4 top-1/2 -translate-y-1/2 -rotate-90 origin-top-left text-[10px] font-mono uppercase tracking-[0.2em] text-[#b4c7dd] whitespace-nowrap pointer-events-none hidden xl:block z-0">
        Shipping_Log // LOCAL_RUNTIME
      </div>

      <div className="site-shell-tight flex flex-col items-start gap-10 md:gap-14 relative z-10">
        <div className="max-w-2xl relative">
          <div className="section-title-row mb-6">
            <span className="section-motif">
              <VoiceWaveLogo size={9} strokeWidth={2.6} tone="adaptive" adaptiveOn="light" />
            </span>
            <span className="font-mono text-xs uppercase tracking-widest font-bold text-[#4b5e76]">What ships in the box</span>
          </div>
          <motion.h2
            initial={{ opacity: 0, y: 30 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true, margin: '-100px' }}
            transition={{ duration: 0.6, ease: [0.16, 1, 0.3, 1] }}
            className="font-display text-3xl sm:text-4xl md:text-6xl lg:text-[4.4rem] tracking-tighter font-bold text-[#09090B] mb-4 leading-[0.95]"
          >
            Small app. <br /> <span className="text-[#5b7392]">Serious tooling.</span>
          </motion.h2>
        </div>

        <div className="w-full flex flex-col gap-10 relative">
          <div className="absolute left-[34px] md:left-1/2 top-3 bottom-3 w-[1px] bg-[#d7e4f2] hidden md:block z-0" />

          {features.map((feature, idx) => (
            <motion.div
              initial={{ opacity: 0, y: 40 }}
              whileInView={{ opacity: 1, y: 0 }}
              viewport={{ once: true, margin: '-100px' }}
              transition={{ duration: 0.8, delay: idx * 0.1, ease: [0.16, 1, 0.3, 1] }}
              key={idx}
              className={`flex flex-col md:flex-row gap-5 md:gap-10 items-start md:items-center relative z-10 group ${idx % 2 === 1 ? 'md:flex-row-reverse' : ''}`}
            >
              <div className="hidden md:flex absolute left-1/2 -translate-x-1/2 w-4 h-4 rounded-full bg-[#FFFFFF] border-2 border-[#d7e4f2] z-20 items-center justify-center transition-colors duration-300 group-hover:border-[#1b8eff]/75 group-active:border-[#0050d2]">
                <div className="w-1.5 h-1.5 rounded-full bg-[#7ed8ff] opacity-0 group-hover:opacity-100 group-active:opacity-100 transition-opacity" />
              </div>

              <div className={`w-full md:w-1/2 flex ${idx % 2 === 1 ? 'justify-start md:justify-start' : 'justify-start md:justify-end'}`}>
                <div className="p-5 sm:p-6 md:p-7 bg-[#FFFFFF]/90 backdrop-blur-xl vw-radius-shell border border-[#d8e5f3] shadow-[0_14px_40px_-20px_rgba(0,0,0,0.05)] w-full max-w-[31rem] cursor-pointer transition-all duration-300 hover:-translate-y-0.5 hover:border-[#1b8eff]/60 hover:shadow-[0_20px_48px_-20px_rgba(27,142,255,0.24)] active:translate-y-0 active:border-[#0050d2] active:shadow-[0_16px_36px_-18px_rgba(0,80,210,0.3)] relative overflow-hidden">
                  <div className="absolute top-0 right-0 w-24 h-24 bg-[#f4f9ff] rounded-bl-[100%] z-0" />
                  <div className="absolute left-0 top-0 bottom-0 w-0.5 bg-[linear-gradient(180deg,#0032b8_0%,#1b8eff_58%,#7ed8ff_100%)] opacity-0 transition-opacity duration-300 group-hover:opacity-100 group-active:opacity-100" />

                  <div className="relative z-10">
                    <div className="flex items-start justify-between">
                      <div className="feature-icon-glow w-12 h-12 vw-radius-tab bg-[#09090B] group-hover:bg-[#1b8eff] group-active:bg-[#0050d2] flex items-center justify-center text-[#FFFFFF] mb-5 shadow-inner ring-1 ring-[#09090B]/10 transition-colors duration-300">
                        {feature.icon}
                      </div>
                      {feature.badge ? (
                        <span className="font-mono text-[10px] font-bold uppercase tracking-[0.12em] text-[#0b3f98] bg-[#e5f3ff] px-2 py-1 rounded-full">
                          {feature.badge}
                        </span>
                      ) : null}
                    </div>
                    <h3 className="text-xl sm:text-2xl font-bold font-display tracking-tight text-[#09090B] mb-3">{feature.title}</h3>
                    <p className="text-[#475569] leading-relaxed text-sm sm:text-base font-medium">{feature.description}</p>

                    <div className="mt-6 pt-4 border-t border-[#e9eff7] flex justify-between items-center">
                      <span className="font-mono text-xs font-bold text-[#6a7f98] uppercase">0{idx + 1}</span>
                      <span className="font-mono text-[10px] text-[#0b3f98] uppercase bg-[#e5f3ff] px-2 py-1 rounded">Shipped</span>
                    </div>
                  </div>
                </div>
              </div>
              <div className={`w-full md:w-1/2 hidden md:block ${idx % 2 === 1 ? 'text-right pr-8' : 'text-left pl-8'}`}>
                <h4 className="font-mono text-[11px] uppercase tracking-[0.14em] text-[#8ba2bb] mb-2 drop-shadow-sm">// In practice</h4>
                <div className="font-mono text-sm leading-relaxed text-[#0F172A] font-bold">{feature.spec}</div>
              </div>
            </motion.div>
          ))}
        </div>
      </div>
    </section>
  )
}

export default function Features() {
  return <FeaturesD />
}
