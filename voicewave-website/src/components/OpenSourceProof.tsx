import { motion } from 'framer-motion'
import { ArrowUpRight, DownloadCloud, ScrollText, Star, Tag } from 'lucide-react'
import {
  GITHUB_CHANGELOG_URL,
  GITHUB_LICENSE_URL,
  GITHUB_RELEASES_URL,
  GITHUB_REPO_URL
} from '../config/site'
import { formatCompact, useGitHubStats } from '../lib/useGitHubStats'

export default function OpenSourceProof() {
  const { stars, installerTransfers, latestVersion } = useGitHubStats()

  const stats = [
    {
      icon: Star,
      value: stars !== null ? formatCompact(stars) : '—',
      label: 'GitHub stars',
      href: GITHUB_REPO_URL
    },
    {
      icon: DownloadCloud,
      value: installerTransfers !== null ? formatCompact(installerTransfers) : '—',
      label: 'Installer transfers',
      href: GITHUB_RELEASES_URL
    },
    {
      icon: Tag,
      value: latestVersion ?? '—',
      label: 'Latest release',
      href: GITHUB_RELEASES_URL
    },
    {
      icon: ScrollText,
      value: 'Apache-2.0',
      label: 'License',
      href: GITHUB_LICENSE_URL
    }
  ]

  return (
    <section id="trust" className="scroll-mt-28 px-0 pb-8 pt-12 sm:pb-10 sm:pt-16">
      <div className="site-shell-tight">
        <motion.div
          initial={{ opacity: 0, y: 18 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true, amount: 0.3 }}
          transition={{ duration: 0.55, ease: [0.16, 1, 0.3, 1] }}
          className="panel-card overflow-hidden p-6 sm:p-8"
        >
          <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-[#64748b]">
            Open source, verifiable
          </p>
          <h2 className="mt-3 max-w-3xl font-display text-[clamp(1.6rem,6vw,3.2rem)] font-bold leading-[1.03] tracking-tight text-[#0a1020]">
            A privacy claim you don't have to take on faith.
          </h2>
          <p className="mt-4 max-w-2xl text-base leading-relaxed text-[#475569] sm:text-lg">
            The entire app is public. These numbers come from the GitHub API, live —
            a privacy product shouldn't fake its own counters.
          </p>

          <div className="mt-8 grid grid-cols-2 gap-4 lg:grid-cols-4">
            {stats.map((stat) => (
              <a
                key={stat.label}
                href={stat.href}
                target="_blank"
                rel="noreferrer"
                className="group rounded-2xl border border-[#dbe5f2] bg-[#f8fbff] p-4 transition-colors hover:border-[#1b8eff]/55 sm:p-5"
              >
                <stat.icon className="h-4 w-4 text-[#1b8eff]" aria-hidden="true" />
                <p className="mt-3 font-display text-2xl font-bold tabular-nums tracking-tight text-[#0a1020] sm:text-3xl">
                  {stat.value}
                </p>
                <p className="mt-1.5 flex items-center gap-1 text-sm text-[#475569]">
                  {stat.label}
                  <ArrowUpRight
                    className="h-3 w-3 opacity-0 transition-opacity group-hover:opacity-100"
                    aria-hidden="true"
                  />
                </p>
              </a>
            ))}
          </div>

          <div className="mt-6 flex flex-wrap items-center gap-x-6 gap-y-2">
            <a
              href={GITHUB_REPO_URL}
              target="_blank"
              rel="noreferrer"
              data-goatcounter-click="github-open-source"
              className="inline-flex items-center gap-1.5 font-mono text-[11px] font-bold uppercase tracking-[0.14em] text-[#0b3f98] transition-colors hover:text-[#0050d2]"
            >
              Browse the source <ArrowUpRight className="h-3 w-3" aria-hidden="true" />
            </a>
            <a
              href={GITHUB_CHANGELOG_URL}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1.5 font-mono text-[11px] font-bold uppercase tracking-[0.14em] text-[#0b3f98] transition-colors hover:text-[#0050d2]"
            >
              Read the changelog <ArrowUpRight className="h-3 w-3" aria-hidden="true" />
            </a>
          </div>
        </motion.div>
      </div>
    </section>
  )
}
