import { AnimatePresence, motion, useReducedMotion } from 'framer-motion'
import { ArrowUp } from 'lucide-react'
import { useEffect, useState } from 'react'

const links = [
  { href: '#home', label: 'Home' },
  { href: '#demo', label: 'Demo' },
  { href: '#features', label: 'Features' },
  { href: '#trust', label: 'Trust' }
]

export default function FloatingNav() {
  const prefersReducedMotion = useReducedMotion()
  const [visible, setVisible] = useState(false)
  const [activeHref, setActiveHref] = useState('#home')
  const [hoveredHref, setHoveredHref] = useState<string | null>(null)

  const targetHref = hoveredHref ?? activeHref

  useEffect(() => {
    const homeSection = document.getElementById('home')
    if (!homeSection) {
      return
    }

    const observer = new IntersectionObserver(
      ([entry]) => {
        const scrolledPastHome = !entry.isIntersecting && entry.boundingClientRect.top < 0
        setVisible((current) => (current === scrolledPastHome ? current : scrolledPastHome))
      },
      {
        threshold: 0,
        rootMargin: '-74px 0px 0px 0px'
      }
    )

    observer.observe(homeSection)

    return () => {
      observer.disconnect()
    }
  }, [])

  useEffect(() => {
    const commitFromHash = () => {
      const hash = window.location.hash
      if (links.some((link) => link.href === hash)) {
        setActiveHref(hash)
      }
    }

    commitFromHash()
    window.addEventListener('hashchange', commitFromHash)
    return () => window.removeEventListener('hashchange', commitFromHash)
  }, [])

  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-4 z-[70] flex justify-center px-2 sm:bottom-6 sm:px-3">
      <AnimatePresence>
        {visible ? (
          <motion.nav
            initial={{ opacity: 0, y: 28, scale: 0.985 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: 20, scale: 0.985 }}
            transition={
              prefersReducedMotion
                ? { duration: 0.01 }
                : { type: 'spring', stiffness: 360, damping: 32, mass: 0.8 }
            }
            style={{ willChange: 'transform, opacity' }}
            className="floating-nav-glass pointer-events-auto flex w-[min(690px,calc(100%_-_2rem))] items-center gap-1.5 rounded-full px-2 py-2 sm:gap-2 sm:px-2.5"
            onMouseLeave={() => setHoveredHref(null)}
            aria-label="Floating site navigation"
          >
            <div className="grid min-w-0 flex-1 grid-cols-4 gap-1.5">
              {links.map((link) => (
                <a
                  key={link.href}
                  href={link.href}
                  onClick={() => setActiveHref(link.href)}
                  onMouseEnter={() => setHoveredHref(link.href)}
                  onFocus={() => setHoveredHref(link.href)}
                  data-current={targetHref === link.href ? 'true' : 'false'}
                  className="floating-nav-link inline-flex min-w-0 items-center justify-center rounded-full px-2 py-3 text-center font-mono text-[11px] font-semibold uppercase tracking-[0.11em] sm:text-xs"
                >
                  {targetHref === link.href ? (
                    <motion.span
                      layoutId="floating-nav-indicator"
                      className="floating-nav-indicator"
                      transition={
                        prefersReducedMotion
                          ? { duration: 0.01 }
                          : { type: 'spring', stiffness: 430, damping: 34, mass: 0.68 }
                      }
                    />
                  ) : null}
                  <span className="floating-nav-label">{link.label}</span>
                </a>
              ))}
            </div>

            <a
              href="#home"
              onClick={(event) => {
                event.preventDefault()
                setActiveHref('#home')
                window.scrollTo({ top: 0, behavior: 'smooth' })
              }}
              className="floating-nav-top-btn inline-flex items-center gap-1 rounded-full px-3 py-3 font-mono text-[11px] font-bold uppercase tracking-[0.12em] sm:px-3.5 sm:text-xs"
            >
              Top
              <ArrowUp className="h-3.5 w-3.5" />
            </a>
          </motion.nav>
        ) : null}
      </AnimatePresence>
    </div>
  )
}
