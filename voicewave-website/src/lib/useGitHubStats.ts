import { useEffect, useState } from 'react'
import { GITHUB_API_RELEASES_URL, GITHUB_API_REPO_URL } from '../config/site'

export type GitHubStats = {
  stars: number | null
  installerTransfers: number | null
  latestVersion: string | null
}

const CACHE_KEY = 'vw-github-stats-v2'
const CACHE_TTL_MS = 30 * 60 * 1000

const EMPTY: GitHubStats = { stars: null, installerTransfers: null, latestVersion: null }

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
  if (Array.isArray(releases)) {
    installerTransfers = 0
    for (const release of releases) {
      if (!latestVersion && typeof release?.tag_name === 'string' && !release.prerelease && !release.draft) {
        latestVersion = release.tag_name
      }
      if (Array.isArray(release?.assets)) {
        for (const asset of release.assets) {
          if (
            typeof asset?.name === 'string' &&
            asset.name.toLowerCase().endsWith('.exe') &&
            typeof asset?.download_count === 'number'
          ) {
            installerTransfers += asset.download_count
          }
        }
      }
    }
  }

  return { stars, installerTransfers, latestVersion }
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
