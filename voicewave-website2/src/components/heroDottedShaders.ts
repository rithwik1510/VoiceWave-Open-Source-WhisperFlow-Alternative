export const heroFullscreenVertexShader = `
  varying vec2 vUv;

  void main() {
    vUv = uv;
    gl_Position = vec4(position.xy, 0.0, 1.0);
  }
`

export const heroSimulationFragmentShader = `
  precision highp float;

  varying vec2 vUv;

  uniform sampler2D uPrev;
  uniform vec2 uPointer;
  uniform vec2 uPointerDelta;
  uniform float uPointerActive;
  uniform float uTime;
  uniform float uDt;
  uniform float uDecay;
  uniform float uAmbientStrength;
  uniform vec2 uScrollVelocity;
  uniform vec2 uTexelSize;

  float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
  }

  void main() {
    vec4 previous = texture2D(uPrev, vUv);
    vec2 velocity = previous.gb - 0.5;

    vec2 ambientDrift = vec2(
      sin(vUv.y * 10.0 + uTime * 0.27) + cos(vUv.x * 9.0 - uTime * 0.33),
      cos(vUv.x * 11.0 - uTime * 0.22) - sin(vUv.y * 7.0 + uTime * 0.31)
    ) * 0.003;

    vec2 advectUv = clamp(vUv - velocity * 0.020 - ambientDrift, 0.0, 1.0);
    vec4 advected = texture2D(uPrev, advectUv);

    float field = advected.r * uDecay;
    velocity = mix(advected.gb - 0.5, ambientDrift * 14.0, 0.06);

    // Local curl reinforces its own rotation, so moving smoke rolls into
    // vortices instead of smearing. Gated on speed: idle drift is untouched.
    vec2 vL = texture2D(uPrev, clamp(vUv - vec2(uTexelSize.x, 0.0), 0.0, 1.0)).gb - 0.5;
    vec2 vR = texture2D(uPrev, clamp(vUv + vec2(uTexelSize.x, 0.0), 0.0, 1.0)).gb - 0.5;
    vec2 vB = texture2D(uPrev, clamp(vUv - vec2(0.0, uTexelSize.y), 0.0, 1.0)).gb - 0.5;
    vec2 vT = texture2D(uPrev, clamp(vUv + vec2(0.0, uTexelSize.y), 0.0, 1.0)).gb - 0.5;
    float curl = 0.5 * ((vR.y - vL.y) - (vT.x - vB.x));
    float speed = length(velocity);
    vec2 swirl = vec2(velocity.y, -velocity.x) * curl * 16.0;
    velocity += swirl * smoothstep(0.015, 0.09, speed) * uDt;

    vec2 uvTop = vec2(vUv.x, 1.0 - vUv.y);
    float pointerDist = distance(uvTop, uPointer);
    float impulse = exp(-pointerDist * pointerDist * 380.0) * uPointerActive;

    field += impulse * 0.82;
    velocity += uPointerDelta * impulse * 0.68;
    velocity += uScrollVelocity * 0.55;

    float sparkle = hash12(floor(vUv * 280.0) + floor(uTime * 0.82));
    field += uAmbientStrength * (0.58 + sparkle * 0.42) * uDt;

    field = clamp(field, 0.0, 1.0);
    velocity = clamp(velocity, vec2(-0.5), vec2(0.5));

    gl_FragColor = vec4(field, velocity + 0.5, 1.0);
  }
`

export const heroDisplayFragmentShader = `
  precision highp float;

  varying vec2 vUv;

  uniform sampler2D uField;
  uniform sampler2D uVideo;
  uniform sampler2D uMask;
  uniform vec2 uResolution;
  uniform vec2 uPointer;
  uniform float uPointerActive;
  uniform float uTopCutoffPx;
  uniform float uTime;
  uniform float uDotSize;
  uniform float uMinDotRadius;
  uniform float uDotGap;
  uniform float uDotAlphaMultiplier;
  uniform float uMaskStrength;
  uniform float uPointerRadius;
  uniform float uPointerStrength;
  uniform float uFluidStrength;
  uniform vec2 uSafeCenterPx;
  uniform vec2 uSafeHalfSizePx;
  uniform float uSafeCornerPx;
  uniform vec2 uSafe2CenterPx;
  uniform vec2 uSafe2HalfSizePx;
  uniform float uSafe2CornerPx;
  uniform float uSafe2Active;
  uniform float uSafeRadiusPx;
  uniform float uSafeFeatherPx;
  uniform float uVideoMix;
  uniform float uMaskMix;
  uniform float uGamma;
  uniform float uBackgroundLuminanceFloor;
  uniform vec3 uColorPrimary;
  uniform vec3 uColorSecondary;
  uniform vec3 uColorHighlight;

  float hash12(vec2 p) {
    vec3 p3 = fract(vec3(p.xyx) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return fract((p3.x + p3.y) * p3.z);
  }

  float roundedRectSdf(vec2 pointPx, vec2 centerPx, vec2 halfSizePx, float cornerPx) {
    vec2 delta = abs(pointPx - centerPx) - halfSizePx;
    float corner = max(2.0, cornerPx);
    vec2 q = delta + vec2(corner);
    return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - corner;
  }

  void main() {
    vec2 fragPx = gl_FragCoord.xy;
    vec2 fragTopPx = vec2(fragPx.x, uResolution.y - fragPx.y);
    vec2 uv = fragTopPx / uResolution;
    vec4 fluid = texture2D(
      uField,
      vUv + vec2(sin(uTime * 0.18 + vUv.y * 6.0), cos(uTime * 0.16 + vUv.x * 7.5)) * 0.0012
    );
    float field = fluid.r;
    float flow = length(fluid.gb - 0.5) * 2.0;
    float pointerRadius = max(1.0, uPointerRadius);
    float pointerDist = distance(uv, uPointer);
    float pointerFalloff = exp(-pow(pointerDist, 2.0) * pointerRadius) * uPointerActive * uPointerStrength;

    float horizontalBand = max(0.22, exp(-pow((uv.x - 0.5) / 0.82, 2.0)));
    float verticalBand = smoothstep(0.035, 0.13, uv.y) * (1.0 - smoothstep(0.93, 0.998, uv.y));
    float lowerLift = exp(-pow((uv.y - 0.66) / 0.32, 2.0));
    float structure = clamp(horizontalBand * verticalBand * (0.72 + 0.28 * lowerLift), 0.0, 1.0);

    float safeSignedDist = roundedRectSdf(fragTopPx, uSafeCenterPx, uSafeHalfSizePx, uSafeCornerPx);
    float safe2SignedDist = roundedRectSdf(fragTopPx, uSafe2CenterPx, uSafe2HalfSizePx, uSafe2CornerPx);
    float safe2AdjustedDist = safe2SignedDist + uSafeFeatherPx * 2.2;
    float safeNoGlowDist = min(safeSignedDist, mix(100000.0, safe2AdjustedDist, uSafe2Active));
    float cellSize = max(4.8, uDotSize + uDotGap);
    vec2 tile = fract(fragPx / cellSize) - 0.5;
    float distInCell = length(tile);
    vec2 gridCell = floor(fragPx / cellSize);
    float cellNoise = hash12(gridCell);
    float edgeNoise = hash12(gridCell * 1.13 + 91.0) - 0.5;
    float edgeJitterPx = edgeNoise * 1.2;
    float safeBlend = smoothstep(
      -uSafeFeatherPx * 0.58 + edgeJitterPx * 0.18,
      uSafeFeatherPx * 2.05 + edgeJitterPx * 0.18,
      safeSignedDist
    );
    float safeDynamicZone = smoothstep(
      -uSafeFeatherPx * 0.36 + edgeJitterPx * 0.08,
      uSafeFeatherPx * 1.95 + edgeJitterPx * 0.08,
      safeSignedDist
    );
    safeBlend = clamp(safeBlend, 0.0, 1.0);
    safeDynamicZone = clamp(safeDynamicZone, 0.0, 1.0);
    float safeStaticZone = mix(0.96, 1.0, safeBlend);
    float safeDynamicMix = mix(0.72, 1.0, safeDynamicZone);
    float centerBlend = smoothstep(-uSafeFeatherPx * 0.12, uSafeFeatherPx * 0.82, safeNoGlowDist);
    float centerNoGlow = smoothstep(-uSafeFeatherPx * 0.08, uSafeFeatherPx * 0.66, safeNoGlowDist);
    float centerSoft = mix(0.78, 1.0, centerNoGlow);
    float centerCalm = 1.0 - centerNoGlow;
    float topZone = smoothstep(uTopCutoffPx - 2.0, uTopCutoffPx + 8.0, fragTopPx.y);
    float pointerZone = smoothstep(
      -uSafeFeatherPx * 1.15,
      uSafeFeatherPx * 1.3 + 18.0,
      safeNoGlowDist
    );
    float activePointer = pointerFalloff * pointerZone * centerSoft * 0.78;
    float rippleEnvelope = exp(-pointerDist * pointerDist * 420.0) * activePointer;
    float rippleWave = 0.5 + 0.5 * sin(uTime * 9.0 - pointerDist * 140.0);
    float ripple = rippleEnvelope * rippleWave;

    vec3 videoColor = texture2D(uVideo, vUv).rgb;
    float videoLuma = dot(pow(max(videoColor, vec3(0.0)), vec3(uGamma)), vec3(0.299, 0.587, 0.114));
    videoLuma = mix(0.0, videoLuma, clamp(uVideoMix, 0.0, 1.0));
    float maskValue = dot(texture2D(uMask, vUv).rgb, vec3(0.3333));
    float maskBlend = mix(1.0, maskValue, clamp(uMaskMix, 0.0, 1.0));
    maskBlend = max(maskBlend, 0.55);

    float dynamicEnergy =
      (field * 1.02 + flow * 0.72 + structure * 0.5 + activePointer * 0.22 + ripple * 0.24) *
      safeDynamicMix *
      mix(0.92, 1.0, centerBlend);
    float staticPattern = 0.42 + 0.58 * hash12(gridCell * 0.91 + 17.0);
    float staticEnergy = (0.16 + structure * 0.24 + staticPattern * 0.24) * safeStaticZone * mix(0.95, 1.0, centerBlend);
    float energy = clamp(dynamicEnergy + staticEnergy * 0.46 + videoLuma * (0.14 + uFluidStrength * 0.14), 0.0, 1.0);
    energy = max(energy, uBackgroundLuminanceFloor * structure * 0.55);
    energy = mix(energy * 0.98, energy, centerBlend);
    float hoverPresence = smoothstep(0.01, 0.18, activePointer);
    float motionPresence = smoothstep(0.04, 0.30, flow + field * 0.52);
    float activity = clamp(activePointer * 0.72 + flow * 0.84 + field * 0.64 + ripple * 0.24, 0.0, 1.0);
    float activityCurve = smoothstep(0.20, 0.86, activity);
    float idleFade = mix(0.30, 1.0, activityCurve);
    float prominence = mix(0.64 + motionPresence * 0.12, 0.96, hoverPresence);

    float radius = mix(uMinDotRadius / max(cellSize, 1.0), 0.41, energy) + activePointer * 0.06;
    float dot = 1.0 - smoothstep(radius, radius + 0.075, distInCell);

    float dynamicTwinkle = 0.82 + 0.18 * sin(uTime * 1.8 + cellNoise * 6.2831853);
    float staticTwinkle = 0.95 + 0.05 * sin(uTime * 0.42 + cellNoise * 4.0);
    float dynamicAlpha =
      dot *
      dynamicTwinkle *
      (0.07 + dynamicEnergy * 0.62) *
      safeDynamicMix *
      mix(0.74, 1.14, hoverPresence) *
      (1.0 + ripple * 0.58);
    float staticIdleMix = mix(0.25, 1.0, activityCurve);
    float staticAlpha = dot * staticTwinkle * (0.022 + staticEnergy * 0.32 * staticIdleMix) * safeStaticZone;
    float blendDust =
      dot *
      (0.004 + cellNoise * 0.004) *
      (1.0 - safeBlend) *
      smoothstep(-uSafeFeatherPx * 0.38, uSafeFeatherPx * 1.95, safeSignedDist);
    float calmDots = dot * (0.006 + cellNoise * 0.005) * centerCalm * structure;
    float idleDotsFloor = dot * (0.020 + staticPattern * 0.024) * safeStaticZone * (1.0 - hoverPresence * 0.42);
    float alpha = (dynamicAlpha + staticAlpha + blendDust) * structure;
    alpha *= idleFade * prominence;
    alpha *= mix(1.0, safeStaticZone, uMaskStrength);
    alpha *= topZone;
    alpha *= maskBlend;
    float bottomZone = 1.0 - smoothstep(0.82, 0.95, uv.y);
    alpha *= bottomZone;
    alpha += idleDotsFloor * structure * topZone * maskBlend * bottomZone * 0.92;
    alpha += calmDots * topZone * maskBlend * bottomZone * 0.50;
    float basePattern = dot * (0.09 + staticPattern * 0.05) * structure * topZone * bottomZone;
    alpha = max(alpha, basePattern * mix(0.62, 0.82, hoverPresence));
    alpha *= mix(1.0, 1.14, hoverPresence);
    float centerGradualFade = smoothstep(-uSafeFeatherPx * 0.5, uSafeFeatherPx * 4.2, safeNoGlowDist);
    alpha *= centerGradualFade;
    alpha = clamp(alpha, 0.0, 1.0);
    alpha *= uDotAlphaMultiplier;

    // Soft bloom: the half-res field upscales into a naturally blurred plume,
    // so a faint additive haze between the dots reads as light under glass.
    // Driven by the sim field only (hover/scroll smoke) — idle look unchanged.
    float glowSource = clamp(field * 0.85 + flow * 0.35 + activePointer * 0.5, 0.0, 1.0);
    float glow = pow(glowSource, 1.7) * 0.16;
    glow *= structure * topZone * bottomZone * maskBlend;
    glow *= smoothstep(-uSafeFeatherPx * 0.3, uSafeFeatherPx * 1.6, safeNoGlowDist);
    alpha = clamp(alpha + glow, 0.0, 1.0);

    vec3 color = mix(uColorPrimary, uColorSecondary, clamp(energy * 1.0 + videoLuma * 0.22, 0.0, 1.0));
    color = mix(
      color,
      uColorHighlight,
      clamp((activePointer * 0.72 + flow * 0.24 + field * 0.20 + ripple * 0.34 + videoLuma * 0.08) * centerSoft, 0.0, 1.0)
    );
    color = mix(color, uColorHighlight, glow * 1.3);

    gl_FragColor = vec4(color, alpha);
  }
`
