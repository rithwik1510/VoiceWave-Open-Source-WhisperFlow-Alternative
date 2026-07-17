export type HeroDottedTheme = 'light' | 'dark'
export type HeroDottedGridLayout = 'straight' | 'radial' | 'alternating-grid'
export type HeroDottedQualityPreset = 'desktop_high' | 'mobile_adaptive' | 'fallback_procedural'

export type HeroDottedConfig = {
  qualityPreset: HeroDottedQualityPreset
  videoSource: string
  maskSource: string
  posterSource: string
  enableMask: boolean
  gridLayout: HeroDottedGridLayout
  loopAtSeconds: number
  readyTimeSeconds: number
  readyFallbackDelayMs: number
  dotsEnabled: boolean
  dotSize: number
  minDotSize: number
  dotMargin: number
  dotAlphaMultiplier: number
  gamma: number
  maskMix: number
  backgroundLuminanceFloor: number
  fluidCurl: number
  fluidVelocityDissipation: number
  fluidDyeDissipation: number
  fluidSplatRadius: number
  fluidPressureIterations: number
  fluidStrength: number
  targetFps: number
  lowPowerTargetFps: number
  decay: number
  ambientStrength: number
  dotGap: number
  maskStrength: number
  pointerRadius: number
  pointerStrength: number
  baseFps: number
  idleFps: number
  simulationScale: number
  lowPowerSimulationScale: number
  lowPowerBaseFps: number
  lowPowerIdleFps: number
  maxDpr: number
  lowPowerMaxDpr: number
  centerMaskFeatherMultiplier: number
}

export const heroDottedDefaultConfig: HeroDottedConfig = {
  qualityPreset: 'desktop_high',
  videoSource: '/assets/hero/phase1/dotted-base.mp4',
  maskSource: '/assets/hero/phase1/dotted-mask.avif',
  posterSource: '/assets/hero/phase1/dotted-poster.avif',
  enableMask: true,
  gridLayout: 'radial',
  loopAtSeconds: 0,
  readyTimeSeconds: 0.95,
  readyFallbackDelayMs: 1050,
  dotsEnabled: true,
  dotSize: 9,
  minDotSize: 1,
  dotMargin: 0.2,
  dotAlphaMultiplier: 0.98,
  gamma: 0.94,
  maskMix: 0.18,
  backgroundLuminanceFloor: 0.038,
  fluidCurl: 92,
  fluidVelocityDissipation: 0.93,
  fluidDyeDissipation: 0.95,
  fluidSplatRadius: 0.006,
  fluidPressureIterations: 2,
  fluidStrength: 0.16,
  targetFps: 60,
  lowPowerTargetFps: 34,
  decay: 0.976,
  ambientStrength: 0.13,
  dotGap: 1.15,
  maskStrength: 0.88,
  pointerRadius: 210,
  pointerStrength: 0.82,
  baseFps: 60,
  idleFps: 42,
  simulationScale: 0.5,
  lowPowerSimulationScale: 0.34,
  lowPowerBaseFps: 38,
  lowPowerIdleFps: 24,
  maxDpr: 1.75,
  lowPowerMaxDpr: 1.1,
  centerMaskFeatherMultiplier: 0.42
}

export const getHeroDottedPalette = (theme: HeroDottedTheme) => {
  if (theme === 'dark') {
    return {
      primary: [0.035, 0.13, 0.35],
      secondary: [0.095, 0.41, 0.82],
      highlight: [0.62, 0.86, 0.99]
    } as const
  }

  return {
    primary: [0.06, 0.24, 0.6],
    secondary: [0.18, 0.6, 0.93],
    highlight: [0.64, 0.89, 0.99]
  } as const
}
