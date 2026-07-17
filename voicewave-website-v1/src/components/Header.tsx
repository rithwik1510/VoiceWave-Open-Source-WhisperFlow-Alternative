import { Download } from 'lucide-react'
import { windowsDownloadUrl } from '../config/download'
import VoiceWaveLogo from './VoiceWaveLogo'

const topLinks = [
  { href: '#home', label: 'Product' },
  { href: '#demo', label: 'Demo' },
  { href: '#features', label: 'Features' },
  { href: '#privacy', label: 'Privacy' }
]

export default function Header() {
  return (
    <header className="relative z-40 px-0 pt-4 sm:pt-5">
      <div className="site-shell">
        <div
          data-top-nav
          className="flex items-center justify-between gap-4 rounded-full border border-white/25 bg-white/[0.05] px-4 py-2.5 backdrop-blur-sm sm:px-6"
        >
          <a href="#home" className="inline-flex items-center gap-2 text-white">
            <span className="inline-flex h-8 w-8 items-center justify-center rounded-full border border-white/25 bg-white/10">
              <VoiceWaveLogo size={12} strokeWidth={2.7} tone="adaptive" adaptiveOn="dark" />
            </span>
            <span className="font-display text-xl leading-none tracking-tight">VoiceWave</span>
          </a>

          <nav className="hidden items-center gap-4 sm:flex sm:gap-6 md:gap-8">
            {topLinks.map((link) => (
              <a
                key={link.href}
                href={link.href}
                className="top-nav-link font-mono text-[11px] font-semibold uppercase tracking-[0.18em]"
              >
                {link.label}
              </a>
            ))}
          </nav>

          <a
            href={windowsDownloadUrl}
            target="_blank"
            rel="noreferrer"
            download
            className="inline-flex items-center gap-1.5 rounded-full border border-white/35 bg-white/10 px-3 py-2 font-mono text-[10px] font-bold uppercase tracking-[0.14em] text-white transition-colors hover:bg-white/16"
          >
            <span className="hidden sm:inline">Download</span>
            <Download className="h-3.5 w-3.5" />
          </a>
        </div>
      </div>
    </header>
  )
}
