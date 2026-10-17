import { ArrowRight } from 'lucide-react'
import { motion } from 'framer-motion'
import { windowsDownloadUrl } from '../config/download'
import { useGitHubStats } from '../lib/useGitHubStats'

export default function FinalCta() {
  const { installerUrl } = useGitHubStats()

  return (
    <section className="px-0 pb-16 pt-8 sm:pb-24 sm:pt-12">
      <div className="site-shell-tight">
        <motion.div
          initial={{ opacity: 0, y: 18 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, amount: 0.4 }}
          transition={{ duration: 0.55, ease: [0.16, 1, 0.3, 1] }}
          className="relative overflow-hidden rounded-[2rem] bg-[#09090B] px-6 py-14 text-center text-white sm:px-10 sm:py-20"
        >
          <div
            className="pointer-events-none absolute inset-0"
            aria-hidden="true"
            style={{
              background:
                'radial-gradient(52% 68% at 50% 110%, rgba(27,142,255,0.34), transparent 70%), radial-gradient(38% 42% at 82% -10%, rgba(126,216,255,0.16), transparent 70%)'
            }}
          />

          <div className="relative z-10 mx-auto flex max-w-2xl flex-col items-center">
            <p className="font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#7ed8ff]">
              Free · Open source · Private
            </p>
            <h2 className="mt-4 text-balance font-display text-[clamp(2.2rem,6vw,4rem)] font-bold leading-[0.98] tracking-tight">
              Stop typing.
              <br />
              Start talking.
            </h2>
            <p className="mt-5 max-w-lg text-pretty text-base leading-relaxed text-[#b8c9de]">
              One installer, no account, no cloud. Setup downloads a speech model once
              (about 0.5 to 1.6 GB, picked for your PC). After that it works offline in
              whatever app you're using.
            </p>

            <a
              href={installerUrl ?? windowsDownloadUrl}
              target="_blank"
              rel="noreferrer"
              download
              data-goatcounter-click="download-exe-final"
              className="lime-cta mt-9 px-7 py-3"
            >
              Download for Windows
              <ArrowRight className="ml-2 h-3.5 w-3.5" />
            </a>

            <p className="mt-6 font-mono text-[10px] uppercase tracking-[0.16em] text-[#6d87a8]">
              Windows 10/11 · 64-bit · no account required
            </p>
            <p className="mt-3 max-w-md text-pretty text-xs leading-relaxed text-[#8aa3c2]">
              Windows may show &quot;Unknown publisher&quot; the first time, because the installer
              isn&apos;t code-signed yet. Choose More info, then Run anyway. It&apos;s built in
              public from this repo.
            </p>
          </div>
        </motion.div>
      </div>
    </section>
  )
}
