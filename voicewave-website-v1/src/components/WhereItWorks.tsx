type CompatibleSurface = {
  label: string
  logo: string
}

// Ordered roughly by theme: editors → AI → productivity → comms → browsers.
// All SVGs live in voicewave-website/public/logos/ and are sourced from svgl.app
// (Software / AI categories). Logos are deliberately mixed light/default
// variants — the site background is near-white (#f6faff → #f9fcff) so we
// favour darker/coloured marks over white-on-dark variants.
const COMPATIBLE_SURFACES: CompatibleSurface[] = [
  { label: 'VS Code', logo: '/logos/vscode.svg' },
  { label: 'Cursor', logo: '/logos/cursor.svg' },
  { label: 'OpenCode', logo: '/logos/opencode.svg' },
  { label: 'Claude', logo: '/logos/claude.svg' },
  { label: 'JetBrains', logo: '/logos/jetbrains.svg' },
  { label: 'Neovim', logo: '/logos/neovim.svg' },
  { label: 'Windsurf', logo: '/logos/windsurf.svg' },
  { label: 'Zed', logo: '/logos/zed.svg' },
  { label: 'ChatGPT', logo: '/logos/chatgpt.svg' },
  { label: 'Gemini', logo: '/logos/gemini.svg' },
  { label: 'Perplexity', logo: '/logos/perplexity.svg' },
  { label: 'Notion', logo: '/logos/notion.svg' },
  { label: 'Obsidian', logo: '/logos/obsidian.svg' },
  { label: 'Microsoft Word', logo: '/logos/microsoft-word.svg' },
  { label: 'Google Drive', logo: '/logos/google-drive.svg' },
  { label: 'Linear', logo: '/logos/linear.svg' },
  { label: 'GitHub', logo: '/logos/github.svg' },
  { label: 'Slack', logo: '/logos/slack.svg' },
  { label: 'Microsoft Teams', logo: '/logos/microsoft-teams.svg' },
  { label: 'Discord', logo: '/logos/discord.svg' },
  { label: 'Gmail', logo: '/logos/gmail.svg' },
  { label: 'Chrome', logo: '/logos/chrome.svg' },
  { label: 'Edge', logo: '/logos/edge.svg' },
]

export default function WhereItWorks() {
  return (
    <section id="where-it-works" className="relative scroll-mt-28 overflow-hidden bg-transparent px-0 pb-12 pt-6 md:pb-16 md:pt-9">
      <div className="site-shell relative z-10">
        <div className="mx-auto max-w-3xl text-center">
          <p className="section-eyebrow font-mono text-[11px] font-bold uppercase tracking-[0.22em] text-[#61758f] sm:text-xs">
            <span aria-hidden="true" className="section-eyebrow-tick" />
            Works Where You Write
          </p>
          <p className="mt-4 text-pretty text-[clamp(1.05rem,1.6vw,1.28rem)] leading-relaxed text-[#2a4261] sm:mt-5">
            One hotkey. No setup. Just speak.
          </p>
        </div>
      </div>

      <div className="where-marquee-bleed where-marquee-plane mt-6 md:mt-8">
        <div className="where-marquee-mask">
          <div className="where-marquee-track">
            <ul className="where-marquee-run" aria-label="Compatible editors and writing surfaces">
              {COMPATIBLE_SURFACES.map((surface) => (
                <li key={`run-a-${surface.label}`} className="where-marquee-item" title={`Works with ${surface.label}`}>
                  <img
                    className="where-marquee-logo"
                    src={surface.logo}
                    alt={surface.label}
                    loading="lazy"
                    decoding="async"
                    draggable={false}
                  />
                  <span className="where-marquee-wordmark">{surface.label}</span>
                </li>
              ))}
            </ul>
            <ul className="where-marquee-run" aria-hidden="true">
              {COMPATIBLE_SURFACES.map((surface) => (
                <li key={`run-b-${surface.label}`} className="where-marquee-item" title={`Works with ${surface.label}`}>
                  <img
                    className="where-marquee-logo"
                    src={surface.logo}
                    alt=""
                    loading="lazy"
                    decoding="async"
                    draggable={false}
                  />
                  <span className="where-marquee-wordmark">{surface.label}</span>
                </li>
              ))}
            </ul>
          </div>
        </div>
      </div>
    </section>
  )
}
