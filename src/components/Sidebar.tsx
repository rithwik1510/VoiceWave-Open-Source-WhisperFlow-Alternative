const navItems = ["Home", "Sessions", "Dictionary", "Snippets", "Style", "Models", "Settings", "Help"];

export function Sidebar() {
  return (
    <aside className="w-full max-w-[260px] border-r border-pine-100 bg-pine-50/40 px-4 py-6">
      <div className="mb-7 flex items-center gap-3">
        <div className="flex h-9 w-9 items-center justify-center rounded-full bg-pine-700 text-on-accent">V</div>
        <div>
          <p className="font-display text-2xl leading-none text-pine-900">VoiceWave</p>
          <p className="text-xs uppercase tracking-[0.18em] text-pine-700">Local Core</p>
        </div>
      </div>
      <nav className="space-y-2">
        {navItems.map((item, idx) => {
          const isActive = idx === 0;
          return (
            <button
              key={item}
              type="button"
              className={`w-full rounded-xl px-4 py-3 text-left text-sm font-medium transition ${
                isActive ? "bg-pine-100 text-pine-900" : "text-pine-700 hover:bg-surface"
              }`}
            >
              {item}
            </button>
          );
        })}
      </nav>
      <section className="mt-10 rounded-2xl bg-surface p-4 shadow-card">
        <p className="text-xs font-semibold uppercase tracking-[0.2em] text-pine-700">Phase I</p>
        <p className="mt-2 text-sm text-pine-900">Core audio and local inference foundation is in progress.</p>
      </section>
    </aside>
  );
}
