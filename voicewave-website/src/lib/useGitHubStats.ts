import { useEffect, useState } from 'react'
import { GITHUB_API_RELEASES_URL, GITHUB_API_REPO_URL } from '../config/site'

export type GitHubStats = {
  stars: number | null
  installerTransfers: number | null
  latestVersion: string | null
  /** Installer of the newest published release that has one. */
  installerUrl: string | null
}

const CACHE_KEY = 'vw-github-stats-v3'
const CACHE_TTL_MS = 30 * 60 * 1000

const EMPTY: GitHubStats = { stars: null, installerTransfers: null, latestVersion: null, installerUrl: null }

let inFlight: Promise<GitHubStats> | null = null

const readCache = (): GitHubStats | null => {
  try {
    const raw = sessionStorage.getItem(CACHE_KEY)
    if (!raw) {
      return null
    }
    const parsed = JSON.parse(raw) as { at: number; stats: GitHubStats }
    if (Date.now() - parsed.at > CACHE_TTL_MS) {
      return null
    }
    return parsed.stats
  } catch {
    return null
  }
}

const writeCache = (stats: GitHubStats) => {
  try {
    sessionStorage.setItem(CACHE_KEY, JSON.stringify({ at: Date.now(), stats }))
  } catch {
    // Storage unavailable — skip caching.
  }
}

const fetchStats = async (): Promise<GitHubStats> => {
  const [repoResult, releasesResult] = await Promise.allSettled([
    fetch(GITHUB_API_REPO_URL).then((res) => (res.ok ? res.json() : null)),
    fetch(GITHUB_API_RELEASES_URL).then((res) => (res.ok ? res.json() : null))
  ])

  const repo = repoResult.status === 'fulfilled' ? repoResult.value : null
  const releases = releasesResult.status === 'fulfilled' ? releasesResult.value : null

  const stars = typeof repo?.stargazers_count === 'number' ? repo.stargazers_count : null

  let installerTransfers: number | null = null
  let latestVersion: string | null = null
  let installerUrl: string | null = null
  if (Array.isArray(releases)) {
    installerTransfers = 0
    // Releases come newest first.
    for (const release of releases) {
      const published = !release?.prerelease && !release?.draft
      if (!latestVersion && published && typeof release?.tag_name === 'string') {
        latestVersion = release.tag_name
      }
      if (Array.isArray(release?.assets)) {
        for (const asset of release.assets) {
          if (typeof asset?.name !== 'string' || !asset.name.toLowerCase().endsWith('.exe')) {
            continue
          }
          if (typeof asset?.download_count === 'number') {
            installerTransfers += asset.download_count
          }
          // Skips a release whose installer is mid re-upload.
          if (!installerUrl && published && typeof asset?.browser_download_url === 'string') {
            installerUrl = asset.browser_download_url
          }
        }
      }
    }
  }

  return { stars, installerTransfers, latestVersion, installerUrl }
}

/**
 * Live repo stats from the public GitHub API — the honest replacement for
 * fabricated counters. Session-cached; null fields mean "hide the number".
 */
export function useGitHubStats(): GitHubStats {
  const [stats, setStats] = useState<GitHubStats>(() => readCache() ?? EMPTY)

  useEffect(() => {
    if (readCache()) {
      return
    }
    let cancelled = false
    inFlight = inFlight ?? fetchStats()
    inFlight
      .then((next) => {
        writeCache(next)
        if (!cancelled) {
          setStats(next)
        }
      })
      .catch(() => {
        // API rate-limited or offline — leave nulls, sections render without numbers.
      })
      .finally(() => {
        inFlight = null
      })
    return () => {
      cancelled = true
    }
  }, [])

  return stats
}

export const formatCompact = (value: number): string =>
  new Intl.NumberFormat('en', { notation: 'compact', maximumFractionDigits: 1 }).format(value)
