import { ArrowRight } from 'lucide-react'
import { windowsDownloadUrl } from '../config/download'

type GradientBandProps = {
  variant?: 'mid' | 'bottom'
}

export default function GradientBand({ variant = 'mid' }: GradientBandProps) {
  const isBottom = variant === 'bottom'

  return (
    <section className={`band-shell ${isBottom ? 'band-bottom' : 'band-mid'} py-16 sm:py-20`}>
      <div className="site-shell relative z-10">
        <div className="mx-auto max-w-3xl text-center text-white">
          <p className="font-display text-3xl leading-tight sm:text-4xl">
            {isBottom ? 'Be ready for whatever you launch next' : 'Engineered to keep release-to-text flow stable'}
          </p>
          <p className="mt-3 text-sm text-[#d8eaff] sm:text-base">
            {isBottom
              ? 'Private dictation, verified local models, and predictable insertion.'
              : 'Capture, decode, and insert all happen on your device with fallback-safe behavior.'}
          </p>
          <div className="mt-7 flex justify-center">
            <a
              href={windowsDownloadUrl}
              target="_blank"
              rel="noreferrer"
              download
              className="lime-cta px-6 py-2.5"
            >
              Download Setup
              <ArrowRight className="ml-2 h-3.5 w-3.5" />
            </a>
          </div>
        </div>
      </div>
    </section>
  )
}
