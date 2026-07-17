import { useEffect, useRef, useState } from 'react'

type DotParticle = {
  homeX: number
  homeY: number
  x: number
  y: number
  vx: number
  vy: number
  seed: number
}

const DISPLAY_TEXT = 'OUT NOW'
const MAX_PARTICLES = 4200

export default function ComingSoonDots() {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const canvasRef = useRef<HTMLCanvasElement | null>(null)
  const [hasGlyphParticles, setHasGlyphParticles] = useState(false)

  useEffect(() => {
    const root = rootRef.current
    const canvas = canvasRef.current

    if (!root || !canvas) {
      return
    }

    const context = canvas.getContext('2d')
    if (!context) {
      return
    }

    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches

    const pointer = { x: 0, y: 0, active: 0 }
    const layout = { width: 0, height: 0, dpr: 1, dotSize: 2 }

    let particles: DotParticle[] = []
    let animationFrameId = 0
    let destroyed = false
    let isVisible = true
    let resizeObserver: ResizeObserver | null = null
    let intersectionObserver: IntersectionObserver | null = null

    const buildParticles = (width: number, height: number): DotParticle[] => {
      const sampleCanvas = document.createElement('canvas')
      sampleCanvas.width = width
      sampleCanvas.height = height
      const sampleContext = sampleCanvas.getContext('2d')
      if (!sampleContext) {
        return []
      }

      sampleContext.clearRect(0, 0, width, height)
      sampleContext.textAlign = 'center'
      sampleContext.textBaseline = 'middle'
      sampleContext.fillStyle = '#ffffff'

      // Scale calibrated for the 7-char "OUT NOW". If DISPLAY_TEXT is changed
      // back to a longer phrase, drop these multipliers back to 0.2 / 0.17.
      const fontSize = Math.min(width * (width < 760 ? 0.3 : 0.26), height * 0.7)
      sampleContext.font = `800 ${fontSize}px Fraunces, serif`
      sampleContext.fillText(DISPLAY_TEXT, width * 0.5, height * 0.52)

      const imageData = sampleContext.getImageData(0, 0, width, height)
      const data = imageData.data
      const step = width < 760 ? 5 : 6
      const nextParticles: DotParticle[] = []

      for (let y = 0; y < height; y += step) {
        for (let x = 0; x < width; x += step) {
          const alphaIndex = (y * width + x) * 4 + 3
          if (data[alphaIndex] < 120) {
            continue
          }

          const jitterX = (Math.random() - 0.5) * 9
          const jitterY = (Math.random() - 0.5) * 9

          nextParticles.push({
            homeX: x,
            homeY: y,
            x: x + jitterX,
            y: y + jitterY,
            vx: (Math.random() - 0.5) * 0.5,
            vy: (Math.random() - 0.5) * 0.5,
            seed: Math.random()
          })
        }
      }

      if (nextParticles.length <= MAX_PARTICLES) {
        return nextParticles
      }

      const stride = Math.ceil(nextParticles.length / MAX_PARTICLES)
      return nextParticles.filter((_, index) => index % stride === 0)
    }

    const resize = () => {
      const rect = root.getBoundingClientRect()
      const width = Math.max(320, Math.round(rect.width))
      const height = Math.max(220, Math.round(rect.height))
      const dpr = Math.min(window.devicePixelRatio || 1, 1.5)

      layout.width = width
      layout.height = height
      layout.dpr = dpr
      layout.dotSize = Math.max(2.2, Math.min(width, height) / 170)

      canvas.width = Math.max(1, Math.round(width * dpr))
      canvas.height = Math.max(1, Math.round(height * dpr))
      canvas.style.width = `${width}px`
      canvas.style.height = `${height}px`

      context.setTransform(dpr, 0, 0, dpr, 0, 0)
      particles = buildParticles(width, height)
      setHasGlyphParticles(particles.length > 200)
    }

    const pointerMove = (event: PointerEvent) => {
      const rect = root.getBoundingClientRect()
      pointer.x = event.clientX - rect.left
      pointer.y = event.clientY - rect.top
      pointer.active = 1
    }

    const pointerLeave = () => {
      pointer.active = 0
    }

    const draw = (timestamp: number) => {
      animationFrameId = window.requestAnimationFrame(draw)

      if (destroyed || !isVisible) {
        return
      }

      const width = layout.width
      const height = layout.height
      if (width <= 0 || height <= 0) {
        return
      }

      context.clearRect(0, 0, width, height)

      const repelRadius = Math.max(80, Math.min(width, height) * 0.2)
      const repelRadiusSquared = repelRadius * repelRadius
      const pointerInfluence = pointer.active

      if (!prefersReducedMotion) {
        pointer.active = Math.max(0, pointer.active - 0.017)
      }

      context.fillStyle = '#f7fdff'
      for (const particle of particles) {
        if (prefersReducedMotion) {
          particle.x += (particle.homeX - particle.x) * 0.22
          particle.y += (particle.homeY - particle.y) * 0.22
          particle.vx *= 0.72
          particle.vy *= 0.72
        } else {
          const homePullX = (particle.homeX - particle.x) * 0.046
          const homePullY = (particle.homeY - particle.y) * 0.046

          particle.vx += homePullX
          particle.vy += homePullY

          if (pointerInfluence > 0.001) {
            const fromPointerX = particle.x - pointer.x
            const fromPointerY = particle.y - pointer.y
            const distanceSquared = fromPointerX * fromPointerX + fromPointerY * fromPointerY

            if (distanceSquared < repelRadiusSquared) {
              const distance = Math.max(0.001, Math.sqrt(distanceSquared))
              const force = 1 - distance / repelRadius
              const push = force * force * 2.45 * pointerInfluence
              particle.vx += (fromPointerX / distance) * push
              particle.vy += (fromPointerY / distance) * push
            }
          }

          particle.vx *= 0.84
          particle.vy *= 0.84
          particle.x += particle.vx
          particle.y += particle.vy
        }

        const pulse = 0.72 + Math.sin(timestamp * 0.0011 + particle.seed * 6.2831853) * 0.28
        const pointerDx = particle.x - pointer.x
        const pointerDy = particle.y - pointer.y
        const pointerDistance = Math.sqrt(pointerDx * pointerDx + pointerDy * pointerDy)
        const pointerBoost = pointerInfluence > 0 ? Math.max(0, 1 - pointerDistance / repelRadius) : 0

        const dotSize = layout.dotSize * (0.9 + pulse * 0.34 + pointerBoost * 0.28)
        context.globalAlpha = (prefersReducedMotion ? 0.78 : 0.74) + pointerBoost * 0.2
        context.fillRect(particle.x - dotSize * 0.5, particle.y - dotSize * 0.5, dotSize, dotSize)
      }

      context.globalAlpha = 1
    }

    resize()
    if ('fonts' in document && 'ready' in document.fonts) {
      void document.fonts.ready.then(() => {
        if (!destroyed) {
          resize()
        }
      })
    }

    root.addEventListener('pointermove', pointerMove, { passive: true })
    root.addEventListener('pointerdown', pointerMove, { passive: true })
    root.addEventListener('pointerleave', pointerLeave)

    resizeObserver = new ResizeObserver(() => {
      resize()
    })
    resizeObserver.observe(root)

    if ('IntersectionObserver' in window) {
      intersectionObserver = new IntersectionObserver(
        (entries) => {
          const [entry] = entries
          isVisible = entry?.isIntersecting ?? true
        },
        { threshold: 0.02 }
      )
      intersectionObserver.observe(root)
    }

    animationFrameId = window.requestAnimationFrame(draw)

    return () => {
      destroyed = true
      window.cancelAnimationFrame(animationFrameId)
      root.removeEventListener('pointermove', pointerMove)
      root.removeEventListener('pointerdown', pointerMove)
      root.removeEventListener('pointerleave', pointerLeave)
      resizeObserver?.disconnect()
      intersectionObserver?.disconnect()
    }
  }, [])

  return (
    <div ref={rootRef} className="coming-soon-dots-wrap">
      <canvas ref={canvasRef} className="coming-soon-dots-canvas" />
      <p className={`coming-soon-fallback-text ${hasGlyphParticles ? 'is-hidden' : ''}`}>{DISPLAY_TEXT}</p>
      <span className="sr-only">Out now</span>
    </div>
  )
}
