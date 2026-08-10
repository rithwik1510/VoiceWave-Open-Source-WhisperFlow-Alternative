import type React from "react";
import { LogIn, PanelLeftClose, PanelLeftOpen, Settings, UserCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { NAV_ITEMS_BOTTOM, NAV_ITEMS_TOP } from "../constants";
import type { ThemeConfig } from "../types";

interface LayoutProps {
  theme: ThemeConfig;
  children: React.ReactNode;
  activeNav: string;
  activePopupNav?: string | null;
  setActiveNav: (id: string) => void;
  isRecording: boolean;
  onUpgradeClick?: () => void;
  isPro?: boolean;
  showProTools?: boolean;
  profileDisplayName?: string;
  profileStatusLabel?: string;
  isProfileAuthenticated?: boolean;
}

export const Layout: React.FC<LayoutProps> = ({
  theme,
  children,
  activeNav,
  activePopupNav = null,
  setActiveNav,
  isRecording,
  onUpgradeClick,
  isPro = false,
  showProTools = false,
  profileDisplayName = "Workspace",
  profileStatusLabel = "Guest mode",
  isProfileAuthenticated = false
}) => {
  const { colors, typography, shapes } = theme;
  const LogoComponent = theme.logo;
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [profileMenuOpen, setProfileMenuOpen] = useState(false);
  const profileMenuRef = useRef<HTMLDivElement | null>(null);
  const topNavItems = NAV_ITEMS_TOP.filter(
    (item) => showProTools || item.id !== "pro-tools"
  );

  useEffect(() => {
    if (!profileMenuOpen) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      if (!profileMenuRef.current) {
        return;
      }
      if (!profileMenuRef.current.contains(event.target as Node)) {
        setProfileMenuOpen(false);
      }
    };

    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        setProfileMenuOpen(false);
      }
    };

    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [profileMenuOpen]);

  const NavButton = ({ item }: { item: { id: string; label: string; icon: React.ComponentType<any> } }) => {
    const isActive = activeNav === item.id || activePopupNav === item.id;
    const Icon = item.icon;
    const activate = () => setActiveNav(item.id);
    return (
      <button
        onPointerDown={(event) => {
          event.preventDefault();
          activate();
        }}
        onClick={activate}
        aria-current={isActive ? "page" : undefined}
        title={sidebarCollapsed ? item.label : undefined}
        className={`
          vw-nav-button w-full cursor-pointer select-none flex items-center ${sidebarCollapsed ? "justify-center px-2" : "justify-between px-4"} py-2.5 text-sm transition-colors duration-150 group
          ${shapes.navItemShape}
          ${
            isActive
              ? "bg-surface text-ink-strong font-semibold shadow-[var(--vw-shadow-card)] border border-edge"
              : "border border-transparent text-quiet hover:bg-surface-hover hover:text-ink-strong"
          }
        `}
        type="button"
      >
        <div className={`flex items-center ${sidebarCollapsed ? "gap-0 justify-center w-full" : "gap-3"}`}>
          <Icon
            size={17}
            className={`vw-nav-icon ${isActive ? "text-ink-strong" : "text-faint group-hover:text-ink-strong"}`}
          />
          <span className="vw-nav-label">{item.label}</span>
        </div>
        {!sidebarCollapsed && <div className={`vw-nav-active-dot ${isActive ? "opacity-100" : "opacity-0"}`} />}
      </button>
    );
  };

  const openWorkspacePanel = (panelId: string) => {
    setProfileMenuOpen(false);
    setActiveNav(panelId);
  };

  return (
    <div className={`relative isolate flex h-screen w-full overflow-hidden ${colors.shellBg} ${colors.textPrimary} ${typography.fontBody}`}>
      <aside
        data-sidebar-collapsed={sidebarCollapsed ? "true" : "false"}
        className={`vw-sidebar-shell relative z-40 ${sidebarCollapsed ? "w-20" : "w-52"} flex-shrink-0 flex flex-col ${colors.shellBg}`}
      >
        <div className={`${sidebarCollapsed ? "px-3 pt-4 pb-4 flex-col gap-2" : "p-6 pb-4 gap-3"} flex items-center flex-shrink-0`}>
          {sidebarCollapsed ? (
            <>
              <button
                type="button"
                className="h-8 w-8 text-quiet transition hover:text-ink"
                onClick={() => setSidebarCollapsed((prev) => !prev)}
                aria-label="Expand sidebar"
                title="Expand sidebar"
              >
                <PanelLeftOpen size={17} className="mx-auto" />
              </button>
              <div
                className={`
                  w-10 h-10 aspect-square shrink-0 flex items-center justify-center rounded-full
                  ${colors.accent} ${colors.accentFg}
                  transition-all duration-300 shadow-lg shadow-black/5
                `}
              >
                <LogoComponent size={20} />
              </div>
            </>
          ) : (
            <>
              <div
                className={`
                  w-10 h-10 aspect-square shrink-0 flex items-center justify-center rounded-full
                  ${colors.accent} ${colors.accentFg}
                  transition-all duration-300 shadow-lg shadow-black/5
                `}
              >
                <LogoComponent size={20} />
              </div>
              <div>
                <span className={`${typography.fontDisplay} ${typography.weightHeading} text-xl tracking-tight block leading-none`}>
                  VoiceWave
                </span>
                <span className={`text-[10px] uppercase tracking-widest opacity-60 font-medium ${typography.fontBody} mt-1 block`}>
                  {theme.name}
                </span>
              </div>
              <button
                type="button"
                className="ml-auto h-8 w-8 text-quiet transition hover:text-ink"
                onClick={() => setSidebarCollapsed((prev) => !prev)}
                aria-label="Collapse sidebar"
                title="Collapse sidebar"
              >
                <PanelLeftClose size={17} className="mx-auto" />
              </button>
            </>
          )}
        </div>

        <nav className={`relative z-50 ${sidebarCollapsed ? "px-2" : "px-4"} space-y-1 flex-shrink-0`}>
          {topNavItems.map((item) => (
            <NavButton key={item.id} item={item} />
          ))}
        </nav>

        <div className="flex-1" />

        <div className={`vw-sidebar-pro-wrap ${sidebarCollapsed ? "is-collapsed" : ""}`}>
          <div className="vw-sidebar-pro-wrap-inner">
            <div className="vw-sidebar-pro-panel p-4">
              <span
                className={`text-[10px] uppercase font-bold px-2 py-0.5 rounded ${
                  isPro ? "vw-pro-badge-active" : "vw-pro-badge"
                }`}
              >
                {isPro ? "PRO ACTIVE" : "PRO"}
              </span>
              <p className="mt-2.5 mb-3 text-xs leading-relaxed text-quiet">
                {isPro
                  ? "Format profiles, code mode, and power history are unlocked on this device."
                  : "The release offer unlocks every Pro tool for coders and students."}
              </p>
              <button
                className="vw-btn-primary vw-btn-sm w-full"
                type="button"
                onClick={() => {
                  if (isPro) {
                    setActiveNav("pro-tools");
                    return;
                  }
                  if (onUpgradeClick) {
                    onUpgradeClick();
                    return;
                  }
                  setActiveNav("pro");
                }}
              >
                {isPro ? "Open Pro Tools" : "View Release Offer"}
              </button>
            </div>
          </div>
        </div>

        <div className="vw-sidebar-divider-wrap">
          <div className={`h-px mx-1 border-t ${colors.divider}`} />
        </div>

        <nav className={`relative z-50 ${sidebarCollapsed ? "px-2" : "px-4"} pb-7 space-y-1 flex-shrink-0`}>
          {NAV_ITEMS_BOTTOM.map((item) => (
            <NavButton key={item.id} item={item} />
          ))}
        </nav>
      </aside>

      <main className="relative z-10 flex-1 flex flex-col min-w-0">
        <header className={`h-14 flex items-center justify-between px-6 flex-shrink-0 z-20 ${colors.shellBg}`}>
          <div className="flex items-center gap-3 text-sm">
            {isRecording && (
              <div className={`px-3 py-1 flex items-center gap-2 ${colors.recording} text-on-accent rounded-full text-xs font-medium`}>
                <div className="w-2 h-2 rounded-full bg-on-accent animate-pulse" />
                Recording
              </div>
            )}
          </div>

          <div className="flex items-center gap-6">
            <div ref={profileMenuRef} className="relative">
              <button
                type="button"
                className="flex items-center gap-3 rounded-full px-1.5 py-1 transition hover:bg-surface-hover"
                onClick={() => setProfileMenuOpen((prev) => !prev)}
                aria-haspopup="menu"
                aria-expanded={profileMenuOpen}
                aria-label="Open workspace menu"
              >
                <div className={`w-9 h-9 flex items-center justify-center border ${colors.bg} border-transparent rounded-full shadow-sm`}>
                  <UserCircle size={20} className="opacity-70" />
                </div>
                <div className="text-sm hidden sm:block text-left">
                  <p className="leading-none font-medium text-ink-strong">{profileDisplayName}</p>
                  <p className="mt-1 text-[11px] leading-none text-faint">{profileStatusLabel}</p>
                </div>
              </button>
              {profileMenuOpen && (
                <div className="vw-profile-menu" role="menu" aria-label="Workspace menu">
                  <button
                    type="button"
                    className="vw-profile-menu-item"
                    role="menuitem"
                    onClick={() => openWorkspacePanel("profile")}
                  >
                    <UserCircle size={15} />
                    Profile
                  </button>
                  <button
                    type="button"
                    className="vw-profile-menu-item"
                    role="menuitem"
                    onClick={() => openWorkspacePanel("settings")}
                  >
                    <Settings size={15} />
                    Settings
                  </button>
                  <button
                    type="button"
                    className="vw-profile-menu-item"
                    role="menuitem"
                    onClick={() => openWorkspacePanel("auth")}
                  >
                    <LogIn size={15} />
                    {isProfileAuthenticated ? "Account" : "Sign In"}
                  </button>
                </div>
              )}
            </div>
          </div>
        </header>

        <div className="flex-1 overflow-hidden relative pr-2 pb-2">
          <div
            className={`vw-canvas-scroll w-full h-full overflow-y-auto relative ${colors.canvasBg} rounded-3xl border border-edge shadow-[var(--vw-shadow-card)]`}
          >
            <div className="px-6 py-6 min-h-full">{children}</div>
          </div>
        </div>
      </main>
    </div>
  );
};
