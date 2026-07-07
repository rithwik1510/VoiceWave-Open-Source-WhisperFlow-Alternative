type VoiceWaveLogoProps = {
  size?: number;
  className?: string;
  strokeWidth?: number;
  tone?: 'brand' | 'white' | 'adaptive';
  adaptiveOn?: 'light' | 'dark';
};

export default function VoiceWaveLogo({
  size = 24,
  className,
  strokeWidth = 3,
  tone = 'adaptive',
  adaptiveOn = 'light'
}: VoiceWaveLogoProps) {
  const palette = (() => {
    if (tone === 'white') {
      return { left: '#FFFFFF', right: '#FFFFFF' };
    }

    if (tone === 'adaptive') {
      return adaptiveOn === 'dark'
        ? { left: '#FFFFFF', right: '#FFFFFF' }
        : { left: '#000000', right: '#000000' };
    }

    return { left: '#1B8EFF', right: '#7ED8FF' };
  })();

  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      className={className}
      aria-hidden="true"
      focusable="false"
    >
      <path d="M2 10v4" stroke={palette.left} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" />
      <path d="M6 7v10" stroke={palette.left} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" />
      <path d="M10 3v18" stroke={palette.left} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" />
      <path d="M14 3v18" stroke={palette.right} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" />
      <path d="M18 7v10" stroke={palette.right} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" />
      <path d="M22 10v4" stroke={palette.right} strokeWidth={strokeWidth} strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}
