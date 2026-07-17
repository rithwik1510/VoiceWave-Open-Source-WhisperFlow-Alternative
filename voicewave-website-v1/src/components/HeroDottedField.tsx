import { useEffect, useRef, useState } from 'react'
import * as THREE from 'three'
import { getHeroDottedPalette, heroDottedDefaultConfig, type HeroDottedTheme } from './heroDottedConfig'
import {
  heroDisplayFragmentShader,
  heroFullscreenVertexShader,
  heroSimulationFragmentShader
} from './heroDottedShaders'

export type HeroSafeZone = {
  centerX: number
  centerY: number
  radius?: number
  halfWidth?: number
  halfHeight?: number
}

type HeroDottedFieldProps = {
  onReady?: () => void
  disableInteraction?: boolean
  theme?: HeroDottedTheme
  safeZone?: HeroSafeZone
  topCutoffPx?: number
}

const MIN_CANVAS_SIZE = 64
const POINTER_DELTA_CAP = 0.045
const POINTER_DELTA_CAP_LOW_POWER = 0.03
const POINTER_ACTIVE_CAP = 0.78

export default function HeroDottedField({
  onReady,
  disableInteraction = false,
  theme = 'dark',
  safeZone,
  topCutoffPx = 0
}: HeroDottedFieldProps) {
  const rootRef = useRef<HTMLDivElement | null>(null)
  const canvasMountRef = useRef<HTMLDivElement | null>(null)
  const safeZoneRef = useRef<HeroSafeZone | undefined>(safeZone)
  const topCutoffRef = useRef(topCutoffPx)
  const [hasWebgl, setHasWebgl] = useState(false)
  const [isReady, setIsReady] = useState(false)

  useEffect(() => {
    safeZoneRef.current = safeZone
  }, [safeZone])

  useEffect(() => {
    topCutoffRef.current = topCutoffPx
  }, [topCutoffPx])

  useEffect(() => {
    const rootNode = rootRef.current
    const canvasMount = canvasMountRef.current

    if (!rootNode || !canvasMount) {
      return
    }

    const zoneNode = rootNode.closest('.zone-a') as HTMLElement | null
    const prefersReducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches
    const coarsePointer = window.matchMedia('(pointer: coarse)').matches
    const lowPowerHint = coarsePointer || (navigator.hardwareConcurrency ?? 8) <= 4
    const useLowPower = prefersReducedMotion || lowPowerHint
    const config = heroDottedDefaultConfig
    const palette = getHeroDottedPalette(theme)
    const qualityPreset = useLowPower ? 'mobile_adaptive' : config.qualityPreset
    zoneNode?.setAttribute('data-dotted-ready', 'false')
    zoneNode?.setAttribute('data-dotted-quality', qualityPreset)

    let destroyed = false
    let isVisible = true
    let animationFrameId = 0
    let readyFallbackTimeoutId = 0
    let lastFrameTime = performance.now()
    let simulationAccumulator = 0
    let readyFired = false
    let videoMix = 0

    const pointer = new THREE.Vector2(0.5, 0.72)
    const pointerDelta = new THREE.Vector2(0, 0)
    const nextPointer = new THREE.Vector2(0.5, 0.72)
    let pointerActive = 0
    let lastScrollY = window.scrollY
    let scrollVelY = 0

    const onScroll = () => {
      if (coarsePointer) return
      const delta = window.scrollY - lastScrollY
      lastScrollY = window.scrollY
      scrollVelY = Math.max(-0.018, Math.min(0.018, delta * 0.00028))
    }
    window.addEventListener('scroll', onScroll, { passive: true })

    let renderer: THREE.WebGLRenderer | null = null
    let displayScene: THREE.Scene | null = null
    let simulationScene: THREE.Scene | null = null
    let camera: THREE.OrthographicCamera | null = null
    let displayMaterial: THREE.ShaderMaterial | null = null
    let simulationMaterial: THREE.ShaderMaterial | null = null
    let quadGeometry: THREE.PlaneGeometry | null = null
    let readTarget: THREE.WebGLRenderTarget | null = null
    let writeTarget: THREE.WebGLRenderTarget | null = null
    let interactionTarget: HTMLElement | null = null
    let intersectionObserver: IntersectionObserver | null = null
    let resizeObserver: ResizeObserver | null = null
    let videoElement: HTMLVideoElement | null = null
    let videoTexture: THREE.VideoTexture | null = null
    let posterTexture: THREE.Texture | null = null
    let maskTexture: THREE.Texture | null = null
    let fallbackTexture: THREE.DataTexture | null = null
    let isVideoPlayable = false
    let videoPlayableHandler: (() => void) | null = null
    let videoErrorHandler: (() => void) | null = null

    const resolution = new THREE.Vector2(1, 1)
    const resolutionScale = new THREE.Vector2(1, 1)
    const simStep = 1 / Math.max(24, useLowPower ? config.lowPowerTargetFps : config.targetFps)
    const simScale = useLowPower ? config.lowPowerSimulationScale : config.simulationScale

    const disposeRenderTarget = (target: THREE.WebGLRenderTarget | null) => {
      if (target) {
        target.dispose()
      }
    }

    const getRenderTexture = (target: THREE.WebGLRenderTarget | null): THREE.Texture | null => {
      if (!target) {
        return null
      }
      const typedTarget = target as unknown as { texture?: THREE.Texture }
      return typedTarget.texture ?? null
    }

    const disposeTexture = (texture: THREE.Texture | null) => {
      if (texture) {
        texture.dispose()
      }
    }

    const setTextureDefaults = (texture: THREE.Texture) => {
      texture.minFilter = THREE.LinearFilter
      texture.magFilter = THREE.LinearFilter
      texture.wrapS = THREE.ClampToEdgeWrapping
      texture.wrapT = THREE.ClampToEdgeWrapping
      texture.generateMipmaps = false
      texture.colorSpace = THREE.SRGBColorSpace
      texture.needsUpdate = true
    }

    const makeFallbackTexture = () => {
      const texture = new THREE.DataTexture(new Uint8Array([255, 255, 255, 255]), 1, 1, THREE.RGBAFormat)
      setTextureDefaults(texture)
      return texture
    }

    const createRenderTarget = (width: number, height: number) =>
      new THREE.WebGLRenderTarget(width, height, {
        depthBuffer: false,
        stencilBuffer: false,
        minFilter: THREE.LinearFilter,
        magFilter: THREE.LinearFilter,
        format: THREE.RGBAFormat,
        type: THREE.UnsignedByteType
      })

    const resolveAutoSafeZones = (canvasBounds: DOMRect) => {
      const titleNode = zoneNode?.querySelector<HTMLElement>('.hero-title-copy') ?? null
      const bodyNode = zoneNode?.querySelector<HTMLElement>('.hero-body-copy') ?? null
      const ctaNode = zoneNode?.querySelector<HTMLElement>('.lime-cta') ?? null
      const heroCopyNode = zoneNode?.querySelector<HTMLElement>('[data-hero-copy]') ?? null

      const titleRects: DOMRect[] = []
      if (titleNode) {
        const rect = titleNode.getBoundingClientRect()
        if (rect.width > 1 && rect.height > 1) {
          titleRects.push(rect)
        }
      }

      if (titleRects.length === 0 && heroCopyNode) {
        const rect = heroCopyNode.getBoundingClientRect()
        if (rect.width > 1 && rect.height > 1) {
          titleRects.push(rect)
        }
      }

      if (titleRects.length === 0) {
        return null
      }

      const titleRect = titleRects[0]
      const titlePadX = THREE.MathUtils.clamp(titleRect.width * 0.02, 6, 16)
      const titlePadTop = THREE.MathUtils.clamp(titleRect.height * 0.04, 2, 6)
      const titlePadBottom = THREE.MathUtils.clamp(titleRect.height * 0.09, 5, 12)
      const left = titleRect.left - titlePadX
      const right = titleRect.right + titlePadX
      const top = titleRect.top - titlePadTop
      const bottom = titleRect.bottom + titlePadBottom
      const width = Math.max(1, right - left)
      const height = Math.max(1, bottom - top)
      const primary = {
        centerX: left + width * 0.5 - canvasBounds.left,
        centerY: top + height * 0.5 - canvasBounds.top,
        halfWidth: width * 0.5,
        halfHeight: height * 0.5
      }

      const lowerNodes = [bodyNode, ctaNode].filter(Boolean) as HTMLElement[]
      const lowerRects = lowerNodes
        .map((node) => node.getBoundingClientRect())
        .filter((rect) => rect.width > 1 && rect.height > 1)

      if (lowerRects.length === 0) {
        return { primary, secondary: null as null | HeroSafeZone }
      }

      let lowerLeft = lowerRects[0].left
      let lowerTop = lowerRects[0].top
      let lowerRight = lowerRects[0].right
      let lowerBottom = lowerRects[0].bottom
      for (let index = 1; index < lowerRects.length; index += 1) {
        const rect = lowerRects[index]
        lowerLeft = Math.min(lowerLeft, rect.left)
        lowerTop = Math.min(lowerTop, rect.top)
        lowerRight = Math.max(lowerRight, rect.right)
        lowerBottom = Math.max(lowerBottom, rect.bottom)
      }

      const lowerWidth = Math.max(1, lowerRight - lowerLeft)
      const lowerHeight = Math.max(1, lowerBottom - lowerTop)
      const lowerPadX = THREE.MathUtils.clamp(lowerWidth * 0.01, 4, 9)
      const lowerPadTop = THREE.MathUtils.clamp(lowerHeight * 0.025, 2, 5)
      const lowerPadBottom = THREE.MathUtils.clamp(lowerHeight * 0.045, 3, 7)
      const lowerLeftPadded = lowerLeft - lowerPadX
      const lowerTopPadded = lowerTop - lowerPadTop
      const lowerWidthPadded = lowerWidth + lowerPadX * 2
      const lowerHeightPadded = lowerHeight + lowerPadTop + lowerPadBottom

      const secondary = {
        centerX: lowerLeftPadded + lowerWidthPadded * 0.5 - canvasBounds.left,
        centerY: lowerTopPadded + lowerHeightPadded * 0.5 - canvasBounds.top,
        halfWidth: lowerWidthPadded * 0.5,
        halfHeight: lowerHeightPadded * 0.5
      }

      return { primary, secondary }
    }

    const applyLayoutUniforms = () => {
      if (!displayMaterial) {
        return
      }

      const canvasBounds = canvasMount.getBoundingClientRect()
      const width = Math.max(1, canvasBounds.width)
      const height = Math.max(1, canvasBounds.height)
      const safe = safeZoneRef.current
      const autoSafe = safe ? null : resolveAutoSafeZones(canvasBounds)

      const centerX = (safe?.centerX ?? autoSafe?.primary.centerX ?? width * 0.5) * resolutionScale.x
      const centerY = (safe?.centerY ?? autoSafe?.primary.centerY ?? height * 0.56) * resolutionScale.y
      const halfWidthBase =
        safe?.halfWidth ??
        (safe?.radius ? safe.radius * 0.82 : undefined) ??
        autoSafe?.primary.halfWidth ??
        Math.max(width * 0.2, 220)
      const halfHeightBase =
        safe?.halfHeight ??
        (safe?.radius ? safe.radius * 0.48 : undefined) ??
        autoSafe?.primary.halfHeight ??
        Math.max(height * 0.09, 70)
      const halfWidthPx = halfWidthBase * resolutionScale.x
      const halfHeightPx = halfHeightBase * resolutionScale.y
      const cornerPx = Math.min(halfWidthPx, halfHeightPx) * 0.62
      const feather = Math.max(16, Math.min(halfWidthPx, halfHeightPx) * 0.62)
      const cutoff = Math.max(0, topCutoffRef.current) * resolutionScale.y
      const secondary = autoSafe?.secondary ?? null
      const secondaryHalfWidthPx = (secondary?.halfWidth ?? 0) * resolutionScale.x
      const secondaryHalfHeightPx = (secondary?.halfHeight ?? 0) * resolutionScale.y
      const secondaryCornerPx = Math.min(secondaryHalfWidthPx, secondaryHalfHeightPx) * 0.62

      displayMaterial.uniforms.uSafeCenterPx.value.set(centerX, centerY)
      displayMaterial.uniforms.uSafeHalfSizePx.value.set(halfWidthPx, halfHeightPx)
      displayMaterial.uniforms.uSafeCornerPx.value = cornerPx
      displayMaterial.uniforms.uSafe2CenterPx.value.set(
        (secondary?.centerX ?? 0) * resolutionScale.x,
        (secondary?.centerY ?? 0) * resolutionScale.y
      )
      displayMaterial.uniforms.uSafe2HalfSizePx.value.set(secondaryHalfWidthPx, secondaryHalfHeightPx)
      displayMaterial.uniforms.uSafe2CornerPx.value = secondaryCornerPx
      displayMaterial.uniforms.uSafe2Active.value = secondary ? 1 : 0
      displayMaterial.uniforms.uSafeRadiusPx.value = cornerPx
      displayMaterial.uniforms.uSafeFeatherPx.value = feather
      displayMaterial.uniforms.uTopCutoffPx.value = cutoff
    }

    const updateDrawResolution = () => {
      if (!renderer || !displayMaterial || !simulationMaterial) {
        return
      }

      const bounds = canvasMount.getBoundingClientRect()
      const width = Math.max(MIN_CANVAS_SIZE, Math.round(bounds.width))
      const height = Math.max(MIN_CANVAS_SIZE, Math.round(bounds.height))
      const dprCap = useLowPower ? config.lowPowerMaxDpr : config.maxDpr
      const dpr = Math.min(window.devicePixelRatio || 1, dprCap)
      renderer.setPixelRatio(dpr)
      renderer.setSize(width, height, false)
      renderer.getDrawingBufferSize(resolution)
      resolutionScale.set(resolution.x / width, resolution.y / height)

      displayMaterial.uniforms.uResolution.value.set(resolution.x, resolution.y)

      const simWidth = Math.max(MIN_CANVAS_SIZE, Math.floor(resolution.x * simScale))
      const simHeight = Math.max(MIN_CANVAS_SIZE, Math.floor(resolution.y * simScale))
      disposeRenderTarget(readTarget)
      disposeRenderTarget(writeTarget)
      readTarget = createRenderTarget(simWidth, simHeight)
      writeTarget = createRenderTarget(simWidth, simHeight)

      const seedTexture = getRenderTexture(readTarget)
      simulationMaterial.uniforms.uPrev.value = seedTexture
      displayMaterial.uniforms.uField.value = seedTexture
      applyLayoutUniforms()
    }

    const swapRenderTargets = () => {
      const currentRead = readTarget
      readTarget = writeTarget
      writeTarget = currentRead
    }

    const pointerMoveHandler = (event: PointerEvent) => {
      if (disableInteraction || prefersReducedMotion) {
        return
      }

      const rect = canvasMount.getBoundingClientRect()
      if (rect.width <= 0 || rect.height <= 0) {
        return
      }

      const x = (event.clientX - rect.left) / rect.width
      const y = (event.clientY - rect.top) / rect.height
      const clampedX = THREE.MathUtils.clamp(x, 0, 1)
      const clampedY = THREE.MathUtils.clamp(y, 0, 1)

      nextPointer.set(clampedX, clampedY)
      pointerDelta.set(nextPointer.x - pointer.x, nextPointer.y - pointer.y)
      const pointerDeltaCap = useLowPower ? POINTER_DELTA_CAP_LOW_POWER : POINTER_DELTA_CAP
      if (pointerDelta.lengthSq() > pointerDeltaCap * pointerDeltaCap) {
        pointerDelta.setLength(pointerDeltaCap)
      }
      pointer.copy(nextPointer)
      pointerActive = Math.min(POINTER_ACTIVE_CAP, pointerActive + 0.26)
    }

    const pointerLeaveHandler = () => {
      pointerActive = 0
      pointerDelta.set(0, 0)
    }

    const commitReady = (webglAvailable: boolean) => {
      if (destroyed || readyFired) {
        return
      }

      readyFired = true
      if (webglAvailable) {
        setHasWebgl(true)
      }
      setIsReady(true)
      zoneNode?.setAttribute('data-dotted-ready', 'true')
      onReady?.()
    }

    const tryPlayVideo = () => {
      if (!videoElement) {
        return
      }
      const playPromise = videoElement.play()
      if (playPromise && typeof playPromise.catch === 'function') {
        playPromise.catch(() => {
          // Ignore autoplay restrictions and continue with fallback rendering.
        })
      }
    }

    try {
      renderer = new THREE.WebGLRenderer({
        alpha: true,
        antialias: false,
        powerPreference: 'high-performance',
        premultipliedAlpha: true
      })
      renderer.setClearColor(0x000000, 0)
      renderer.domElement.className = 'hero-dotted-canvas-element'
      canvasMount.appendChild(renderer.domElement)

      camera = new THREE.OrthographicCamera(-1, 1, 1, -1, 0, 1)
      displayScene = new THREE.Scene()
      simulationScene = new THREE.Scene()
      quadGeometry = new THREE.PlaneGeometry(2, 2)
      fallbackTexture = makeFallbackTexture()

      simulationMaterial = new THREE.ShaderMaterial({
        vertexShader: heroFullscreenVertexShader,
        fragmentShader: heroSimulationFragmentShader,
        transparent: false,
        depthWrite: false,
        depthTest: false,
        uniforms: {
          uPrev: { value: null },
          uPointer: { value: pointer.clone() },
          uPointerDelta: { value: pointerDelta.clone() },
          uPointerActive: { value: 0 },
          uTime: { value: 0 },
          uDt: { value: 0.016 },
          uDecay: { value: config.decay },
          uAmbientStrength: { value: prefersReducedMotion ? config.ambientStrength * 0.2 : config.ambientStrength },
          uScrollVelocity: { value: new THREE.Vector2(0, 0) }
        }
      })

      displayMaterial = new THREE.ShaderMaterial({
        vertexShader: heroFullscreenVertexShader,
        fragmentShader: heroDisplayFragmentShader,
        transparent: true,
        depthWrite: false,
        depthTest: false,
        uniforms: {
          uField: { value: null },
          uVideo: { value: fallbackTexture },
          uMask: { value: fallbackTexture },
          uResolution: { value: resolution.clone() },
          uPointer: { value: pointer.clone() },
          uPointerActive: { value: 0 },
          uTime: { value: 0 },
          uDotSize: { value: config.dotSize },
          uMinDotRadius: { value: config.minDotSize },
          uDotGap: { value: config.dotGap },
          uDotAlphaMultiplier: { value: config.dotAlphaMultiplier },
          uMaskStrength: { value: config.maskStrength },
          uPointerRadius: { value: config.pointerRadius },
          uPointerStrength: { value: config.pointerStrength },
          uFluidStrength: { value: config.fluidStrength },
          uSafeCenterPx: { value: new THREE.Vector2(0, 0) },
          uSafeHalfSizePx: { value: new THREE.Vector2(0, 0) },
          uSafeCornerPx: { value: 0 },
          uSafe2CenterPx: { value: new THREE.Vector2(0, 0) },
          uSafe2HalfSizePx: { value: new THREE.Vector2(0, 0) },
          uSafe2CornerPx: { value: 0 },
          uSafe2Active: { value: 0 },
          uSafeRadiusPx: { value: 0 },
          uSafeFeatherPx: { value: 36 },
          uTopCutoffPx: { value: 0 },
          uVideoMix: { value: 0 },
          uMaskMix: { value: config.enableMask ? config.maskMix : 0 },
          uGamma: { value: config.gamma },
          uBackgroundLuminanceFloor: { value: config.backgroundLuminanceFloor },
          uColorPrimary: { value: new THREE.Color(palette.primary[0], palette.primary[1], palette.primary[2]) },
          uColorSecondary: { value: new THREE.Color(palette.secondary[0], palette.secondary[1], palette.secondary[2]) },
          uColorHighlight: { value: new THREE.Color(palette.highlight[0], palette.highlight[1], palette.highlight[2]) }
        }
      })

      const simulationQuad = new THREE.Mesh(quadGeometry, simulationMaterial)
      const displayQuad = new THREE.Mesh(quadGeometry, displayMaterial)
      simulationScene.add(simulationQuad)
      displayScene.add(displayQuad)

      updateDrawResolution()
      readyFallbackTimeoutId = window.setTimeout(() => {
        commitReady(Boolean(renderer))
      }, config.readyFallbackDelayMs)

      const textureLoader = new THREE.TextureLoader()
      textureLoader.load(
        config.posterSource,
        (loadedTexture: THREE.Texture) => {
          if (destroyed) {
            loadedTexture.dispose()
            return
          }
          setTextureDefaults(loadedTexture)
          disposeTexture(posterTexture)
          posterTexture = loadedTexture
          if (displayMaterial && !videoTexture) {
            displayMaterial.uniforms.uVideo.value = posterTexture
          }
        },
        undefined,
        () => {
          if (displayMaterial && fallbackTexture && !videoTexture) {
            displayMaterial.uniforms.uVideo.value = fallbackTexture
          }
        }
      )

      if (config.enableMask) {
        textureLoader.load(
          config.maskSource,
          (loadedTexture: THREE.Texture) => {
            if (destroyed) {
              loadedTexture.dispose()
              return
            }
            setTextureDefaults(loadedTexture)
            disposeTexture(maskTexture)
            maskTexture = loadedTexture
            if (displayMaterial) {
              displayMaterial.uniforms.uMask.value = maskTexture
            }
          },
          undefined,
          () => {
            if (displayMaterial && fallbackTexture) {
              displayMaterial.uniforms.uMask.value = fallbackTexture
            }
          }
        )
      }

      videoElement = document.createElement('video')
      videoElement.preload = 'auto'
      videoElement.muted = true
      videoElement.playsInline = true
      videoElement.autoplay = true
      videoElement.crossOrigin = 'anonymous'
      videoElement.loop = config.loopAtSeconds <= 0
      videoElement.src = config.videoSource
      videoElement.setAttribute('webkit-playsinline', 'true')

      videoPlayableHandler = () => {
        if (destroyed || !videoElement || !displayMaterial) {
          return
        }
        isVideoPlayable = true
        if (!videoTexture) {
          videoTexture = new THREE.VideoTexture(videoElement)
          setTextureDefaults(videoTexture)
        }
        displayMaterial.uniforms.uVideo.value = videoTexture
        tryPlayVideo()
      }

      videoErrorHandler = () => {
        isVideoPlayable = false
      }

      videoElement.addEventListener('loadeddata', videoPlayableHandler)
      videoElement.addEventListener('canplay', videoPlayableHandler)
      videoElement.addEventListener('error', videoErrorHandler)
      videoElement.load()
      tryPlayVideo()

      const initialWarmupPasses = 10
      for (let index = 0; index < initialWarmupPasses; index += 1) {
        const currentRead = readTarget
        const currentWrite = writeTarget
        if (!renderer || !camera || !simulationScene || !simulationMaterial || !currentWrite || !currentRead) {
          break
        }
        simulationMaterial.uniforms.uPrev.value = getRenderTexture(currentRead)
        simulationMaterial.uniforms.uTime.value = index * simStep
        simulationMaterial.uniforms.uDt.value = simStep
        simulationMaterial.uniforms.uPointerActive.value = 0
        renderer.setRenderTarget(currentWrite)
        renderer.render(simulationScene, camera)
        renderer.setRenderTarget(null)
        swapRenderTargets()
      }

      // Mark ready as soon as the WebGL pipeline is alive; waiting for video mix causes reload flash.
      window.clearTimeout(readyFallbackTimeoutId)
      commitReady(true)

      interactionTarget = rootNode.closest('section') ?? rootNode
      if (!disableInteraction && !prefersReducedMotion) {
        interactionTarget.addEventListener('pointermove', pointerMoveHandler, { passive: true })
        interactionTarget.addEventListener('pointerleave', pointerLeaveHandler)
      }

      if ('IntersectionObserver' in window) {
        intersectionObserver = new IntersectionObserver(
          (entries) => {
            const [entry] = entries
            isVisible = entry?.isIntersecting ?? true
          },
          { threshold: 0.05 }
        )
        intersectionObserver.observe(rootNode)
      }

      resizeObserver = new ResizeObserver(() => {
        updateDrawResolution()
      })
      resizeObserver.observe(canvasMount)

      const animate = (timestampMs: number) => {
        if (destroyed) {
          return
        }

        animationFrameId = window.requestAnimationFrame(animate)

        if (!renderer || !camera || !displayScene || !simulationScene || !displayMaterial || !simulationMaterial) {
          return
        }
        if (!readTarget || !writeTarget) {
          return
        }
        if (!isVisible) {
          lastFrameTime = timestampMs
          return
        }

        const deltaSeconds = Math.min((timestampMs - lastFrameTime) / 1000, 0.05)
        lastFrameTime = timestampMs
        if (deltaSeconds <= 0) {
          return
        }

        pointerActive = Math.max(0, pointerActive - deltaSeconds * 1.8)
        pointerDelta.multiplyScalar(0.86)
        simulationAccumulator += deltaSeconds

        if (videoElement && config.loopAtSeconds > 0 && videoElement.duration > 0) {
          if (videoElement.currentTime >= videoElement.duration - 0.04) {
            videoElement.currentTime = config.loopAtSeconds
          }
        }

        while (simulationAccumulator >= simStep) {
          const currentRead = readTarget
          const currentWrite = writeTarget
          if (!currentRead || !currentWrite) {
            break
          }

          simulationMaterial.uniforms.uPrev.value = getRenderTexture(currentRead)
          simulationMaterial.uniforms.uPointer.value.copy(pointer)
          simulationMaterial.uniforms.uPointerDelta.value.copy(pointerDelta)
          simulationMaterial.uniforms.uPointerActive.value = disableInteraction || prefersReducedMotion ? 0 : pointerActive
          simulationMaterial.uniforms.uTime.value = timestampMs * 0.001
          simulationMaterial.uniforms.uDt.value = simStep
          simulationMaterial.uniforms.uScrollVelocity.value.set(0, scrollVelY)
          scrollVelY *= 0.88

          renderer.setRenderTarget(currentWrite)
          renderer.render(simulationScene, camera)
          renderer.setRenderTarget(null)
          swapRenderTargets()

          simulationAccumulator -= simStep
        }

        displayMaterial.uniforms.uField.value = getRenderTexture(readTarget)
        displayMaterial.uniforms.uPointer.value.copy(pointer)
        displayMaterial.uniforms.uPointerActive.value = disableInteraction || prefersReducedMotion ? 0 : pointerActive
        displayMaterial.uniforms.uTime.value = timestampMs * 0.001

        const canUseVideo = Boolean(videoElement && isVideoPlayable && videoElement.readyState >= HTMLMediaElement.HAVE_CURRENT_DATA)
        const targetVideoMix = canUseVideo ? 1 : 0
        const mixLerp = 1 - Math.exp(-deltaSeconds * (canUseVideo ? 4.8 : 3.2))
        videoMix = THREE.MathUtils.lerp(videoMix, targetVideoMix, mixLerp)
        displayMaterial.uniforms.uVideoMix.value = videoMix

        applyLayoutUniforms()
        renderer.setRenderTarget(null)
        renderer.render(displayScene, camera)
      }

      animationFrameId = window.requestAnimationFrame(animate)
    } catch (error) {
      console.warn('HeroDottedField WebGL init failed; using CSS fallback.', error)
      commitReady(false)
    }

    return () => {
      destroyed = true
      window.cancelAnimationFrame(animationFrameId)
      window.clearTimeout(readyFallbackTimeoutId)

      window.removeEventListener('scroll', onScroll)

      if (interactionTarget) {
        interactionTarget.removeEventListener('pointermove', pointerMoveHandler)
        interactionTarget.removeEventListener('pointerleave', pointerLeaveHandler)
      }

      resizeObserver?.disconnect()
      intersectionObserver?.disconnect()

      if (videoElement && videoPlayableHandler && videoErrorHandler) {
        videoElement.removeEventListener('loadeddata', videoPlayableHandler)
        videoElement.removeEventListener('canplay', videoPlayableHandler)
        videoElement.removeEventListener('error', videoErrorHandler)
      }
      if (videoElement) {
        videoElement.pause()
        videoElement.removeAttribute('src')
        videoElement.load()
      }

      if (renderer) {
        renderer.dispose()
        if (renderer.domElement.parentElement === canvasMount) {
          canvasMount.removeChild(renderer.domElement)
        }
      }

      disposeRenderTarget(readTarget)
      disposeRenderTarget(writeTarget)
      disposeTexture(videoTexture)
      disposeTexture(posterTexture)
      disposeTexture(maskTexture)
      disposeTexture(fallbackTexture)
      displayMaterial?.dispose()
      simulationMaterial?.dispose()
      quadGeometry?.dispose()
      zoneNode?.removeAttribute('data-dotted-ready')
      zoneNode?.removeAttribute('data-dotted-quality')
    }
  }, [disableInteraction, onReady, theme])

  return (
    <div
      ref={rootRef}
      aria-hidden="true"
      className={`hero-dotted-field${hasWebgl ? '' : ' is-fallback'}${isReady ? ' is-ready' : ''}`}
    >
      <div ref={canvasMountRef} className="hero-dotted-canvas" />
      <div className="hero-dotted-fallback" />
    </div>
  )
}
