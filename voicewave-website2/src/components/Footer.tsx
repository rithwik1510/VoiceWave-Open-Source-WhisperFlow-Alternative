import {
  GITHUB_CHANGELOG_URL,
  GITHUB_LICENSE_URL,
  GITHUB_RELEASES_URL,
  GITHUB_REPO_URL
} from '../config/site'
import VoiceWaveLogo from './VoiceWaveLogo'

const footerLinks = [
  { label: 'GitHub', href: GITHUB_REPO_URL },
  { label: 'Releases', href: GITHUB_RELEASES_URL },
  { label: 'Changelog', href: GITHUB_CHANGELOG_URL },
  { label: 'License', href: GITHUB_LICENSE_URL }
]

export default function Footer() {
  return (
    <footer className="bottom-mirror-band relative overflow-hidden px-0 pb-12 pt-14 sm:pb-14 sm:pt-16">
      <div className="pointer-events-none absolute inset-0 z-0">
        <div className="absolute left-1/2 top-12 h-48 w-[min(1100px,94vw)] -translate-x-1/2 rounded-[2.25rem] bg-white/26 blur-2xl" />
        <div className="absolute inset-x-0 top-0 h-52 bg-gradient-to-b from-white/12 to-transparent" />
      </div>

      <div className="site-shell relative z-10">
        <div className="grid gap-14 md:grid-cols-[1.2fr_0.8fr] md:items-start">
          <div>
            <div className="inline-flex items-center gap-3">
              <span className="inline-flex h-10 w-10 items-center justify-center rounded-full border border-white/55 bg-white/22">
                <VoiceWaveLogo size={14} strokeWidth={2.6} tone="adaptive" adaptiveOn="light" />
              </span>
              <span className="font-mono text-xs font-semibold uppercase tracking-[0.2em] text-[#0d2b58]">
                Open-source · Offline · Windows
              </span>
            </div>

            <p className="mt-6 max-w-lg text-base leading-relaxed text-[#0c2248] sm:text-lg">
              Free dictation that keeps your voice on your machine. Built in the open —
              stars, issues, and pull requests all welcome.
            </p>

            <p className="mt-10 font-display text-6xl leading-[0.9] tracking-tight sm:text-7xl md:text-[8.5rem]">
              <span className="text-white drop-shadow-[0_4px_14px_rgba(2,10,32,0.55)]">Voice</span>
              <span className="text-[#020814]">Wave</span>
            </p>
          </div>

          <nav className="grid gap-5 font-mono text-xs uppercase tracking-[0.14em] text-[#091b3a] md:justify-items-end md:pt-3">
            {footerLinks.map((link) => (
              <a
                key={link.label}
                href={link.href}
                target="_blank"
                rel="noreferrer"
                className="transition-colors hover:text-[#000000]"
              >
                / {link.label}
              </a>
            ))}
          </nav>
        </div>

        <div className="mt-12 border-t border-[#0b2756]/30 pt-6">
          <p className="font-mono text-xs uppercase tracking-[0.16em] text-[#0d2b58]">
            &copy; {new Date().getFullYear()} VoiceWave &middot; Apache-2.0 licensed &middot; your audio never left this machine
          </p>
        </div>
      </div>
    </footer>
  )
}
