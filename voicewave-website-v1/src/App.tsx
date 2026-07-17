import { useEffect } from 'react'
import Header from './components/Header'
import Hero from './components/Hero'
import HoldToTalk from './components/HoldToTalk'
import WhereItWorks from './components/WhereItWorks'
import ComingSoonDots from './components/ComingSoonDots'
import ScrollDemo from './components/ScrollDemo'
import CapabilityDeepDive from './components/CapabilityDeepDive'
import Features from './components/Features'
import TrustProof from './components/TrustProof'
import Footer from './components/Footer'
import FloatingNav from './components/FloatingNav'

function App() {
  useEffect(() => {
    const root = document.documentElement
    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches

    const applyStaticDrift = () => {
      root.style.setProperty('--site-drift-x', '0px')
      root.style.setProperty('--site-drift-y', '0px')
      root.style.setProperty('--site-grid-shift-x', '0px')
      root.style.setProperty('--site-grid-shift-y', '0px')
      root.style.setProperty('--scroll-progress', '0')
    }

    if (prefersReducedMotion) {
      applyStaticDrift()
      return
    }

    let rafId: number | null = null
    let lastProgress = -1

    const commitDrift = () => {
      rafId = null
      const scrollTop = window.scrollY || window.pageYOffset || 0
      const scrollRange = Math.max(document.documentElement.scrollHeight - window.innerHeight, 1)
      const progress = Math.min(Math.max(scrollTop / scrollRange, 0), 1)

      if (Math.abs(progress - lastProgress) < 0.002) {
        return
      }

      lastProgress = progress

      const driftX = (0.5 - progress) * 52
      const driftY = (progress - 0.5) * 72
      const gridX = (progress - 0.5) * 18
      const gridY = (0.5 - progress) * 14

      root.style.setProperty('--site-drift-x', `${driftX.toFixed(2)}px`)
      root.style.setProperty('--site-drift-y', `${driftY.toFixed(2)}px`)
      root.style.setProperty('--site-grid-shift-x', `${gridX.toFixed(2)}px`)
      root.style.setProperty('--site-grid-shift-y', `${gridY.toFixed(2)}px`)
      root.style.setProperty('--scroll-progress', progress.toFixed(4))
    }

    const onScroll = () => {
      if (rafId !== null) {
        return
      }
      rafId = window.requestAnimationFrame(commitDrift)
    }

    commitDrift()
    window.addEventListener('scroll', onScroll, { passive: true })
    window.addEventListener('resize', onScroll, { passive: true })

    return () => {
      if (rafId !== null) {
        window.cancelAnimationFrame(rafId)
      }
      window.removeEventListener('scroll', onScroll)
      window.removeEventListener('resize', onScroll)
      applyStaticDrift()
    }
  }, [])

  return (
    <div className="site-root min-h-screen w-full selection:bg-[#bfe8ff]/45 selection:text-[#071126]">
      <div className="waveform-scroll-bar" aria-hidden />
      <FloatingNav />

      <section className="zone-a" data-dotted-ready="false" data-dotted-quality="desktop_high">
        <Header />
        <Hero />
      </section>

      <section className="bg-[#f7fbff] py-10 sm:py-14">
        <WhereItWorks />
      </section>

      <hr className="site-divider" aria-hidden="true" />

      <section className="bg-[#f7fbff]">
        <HoldToTalk />
      </section>

      <hr className="site-divider" aria-hidden="true" />

      <section className="bg-[#f7fbff] py-12 sm:py-16">
        <ScrollDemo />
      </section>

      <hr className="site-divider" aria-hidden="true" />

      <section className="bg-[#f7fbff] py-10 sm:py-14">
        <CapabilityDeepDive />
        <Features />
      </section>

      <section className="coming-soon-band">
        <ComingSoonDots />
      </section>

      <hr className="site-divider" aria-hidden="true" />

      <section className="bg-[#f7fbff] pb-10 sm:pb-14">
        <TrustProof />
      </section>

      <Footer />
    </div>
  )
}

export default App
