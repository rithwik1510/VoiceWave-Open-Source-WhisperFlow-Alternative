import { Download, Star } from 'lucide-react'
import { windowsDownloadUrl } from '../config/download'
import { GITHUB_REPO_URL } from '../config/site'
import { formatCompact, useGitHubStats } from '../lib/useGitHubStats'
import VoiceWaveLogo from './VoiceWaveLogo'

const topLinks = [
  { href: '#how', label: 'How it works' },
  { href: '#features', label: 'Features' },
  { href: '#privacy', label: 'Privacy' },
  { href: '#compare', label: 'Compare' },
  { href: '#faq', label: 'FAQ' }
]

export default function Header() {
  const { stars } = useGitHubStats()

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

          <nav className="hidden items-center gap-4 md:flex lg:gap-6">
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

          <div className="flex items-center gap-2">
            <a
              href={GITHUB_REPO_URL}
              target="_blank"
              rel="noreferrer"
              className="hidden items-center gap-1.5 rounded-full border border-white/20 px-3 py-2 font-mono text-[10px] font-bold uppercase tracking-[0.12em] text-white/85 transition-colors hover:border-white/45 hover:text-white sm:inline-flex"
              aria-label={stars !== null ? `View source on GitHub — ${stars} stars` : 'View source on GitHub'}
            >
              <Star className="h-3 w-3 fill-current text-[#7ed8ff]" aria-hidden="true" />
              {stars !== null ? formatCompact(stars) : 'GitHub'}
            </a>

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
      </div>
    </header>
  )
}
