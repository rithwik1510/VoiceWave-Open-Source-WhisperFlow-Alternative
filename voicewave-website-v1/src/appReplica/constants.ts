import React from "react";
import type { ThemeConfig } from "./types";
import {
  Crown,
  Cpu,
  FileText,
  HelpCircle,
  Home,
  Palette,
  Settings,
  Zap
} from "lucide-react";

type IconProps = React.SVGProps<SVGSVGElement> & {
  size?: number;
};

const BaseIcon = ({ children, ...props }: React.PropsWithChildren<IconProps>) =>
  React.createElement(
    "svg",
    {
      width: props.size || 24,
      height: props.size || 24,
      viewBox: "0 0 24 24",
      fill: "none",
      strokeWidth: "3",
      strokeLinecap: "round",
      strokeLinejoin: "round",
      ...props
    },
    children
  );

const WaveLogo = (props: IconProps) =>
  React.createElement(
    BaseIcon,
    props,
    React.createElement("path", { d: "M2 10v4", stroke: "currentColor" }),
    React.createElement("path", { d: "M6 7v10", stroke: "currentColor" }),
    React.createElement("path", { d: "M10 3v18", stroke: "currentColor" }),
    React.createElement("path", { d: "M14 3v18", stroke: "currentColor" }),
    React.createElement("path", { d: "M18 7v10", stroke: "currentColor" }),
    React.createElement("path", { d: "M22 10v4", stroke: "currentColor" })
  );

const ONYX_PALETTE = {
  bg: "bg-[#FAFAFA]",
  sidebarBg: "bg-[#EFEFF3]",
  surface: "bg-white",
  surfaceHighlight: "bg-[#F4F4F5]",
  border: "border-[#E4E4E7]",
  textPrimary: "text-[#09090B]",
  textSecondary: "text-[#475569]",
  textTertiary: "text-[#94A3B8]",
  accent: "bg-[#18181B]",
  accentFg: "text-white",
  success: "text-[#18181B]",
  warning: "text-[#4B5563]",
  error: "text-[#991B1B]",
  recording: "bg-[#000000]",
  navActiveBg: "bg-[#FFFFFF]",
  navActiveFg: "text-[#18181B]",
  shellBg: "bg-[#EFEFF3]",
  canvasBg: "bg-[#FFFFFF]",
  canvasBorder: "border-transparent",
  surfaceBorder: "border-[#E4E4E7]",
  divider: "border-[#D4D4D8]",
  accentBlue: "#1B8EFF",
  accentCyan: "#7ED8FF",
  accentGradient: "linear-gradient(96deg, #0A2A8C 8%, #1B8EFF 52%, #6FD4FF 84%, #A7E8FF 100%)",
  accentGradientSoft: "linear-gradient(135deg, rgba(10,42,140,0.1) 8%, rgba(27,142,255,0.16) 54%, rgba(126,216,255,0.2) 82%, rgba(167,232,255,0.18) 100%)"
};

const SHARED_STRUCTURE = {
  typography: {
    fontDisplay: 'font-["Fraunces"]',
    fontBody: 'font-["Manrope"]',
    weightHeading: "font-semibold"
  },
  shapes: {
    radius: "rounded-3xl",
    borderWidth: "border-0",
    buttonShape: "rounded-full",
    navItemShape: "rounded-full"
  },
  effects: {
    blur: "backdrop-blur-none",
    shadow: "shadow-[0_8px_30px_rgba(0,0,0,0.08)]"
  }
};

export const THEME: ThemeConfig = {
  id: "A",
  name: "Harmonic v1.0",
  description: "Production Release",
  logo: WaveLogo,
  ...SHARED_STRUCTURE,
  colors: ONYX_PALETTE
};

export const THEMES: Record<string, ThemeConfig> = {
  A: THEME,
  harmonic: THEME
};

export const NAV_ITEMS_TOP = [
  { id: "home", label: "Home", icon: Home },
  { id: "models", label: "Models", icon: Cpu },
  { id: "dictionary", label: "Dictionary", icon: FileText },
  { id: "pro", label: "Pro", icon: Crown },
  { id: "pro-tools", label: "Pro Tools", icon: Zap }
];

export const NAV_ITEMS_BOTTOM = [
  { id: "style", label: "Style", icon: Palette },
  { id: "settings", label: "Settings", icon: Settings },
  { id: "help", label: "Help", icon: HelpCircle }
];

export const MOCK_SESSIONS = [
  {
    id: "1",
    title: "Q3 Financial Overview",
    preview: "The revenue growth in the third quarter was primarily driven by...",
    date: "Today, 9:41 AM",
    duration: "4m 12s",
    wordCount: 540
  },
  {
    id: "2",
    title: "Client Email Draft",
    preview: "Hi Sarah, regarding the proposal sent last week, I wanted to clarify...",
    date: "Yesterday, 4:20 PM",
    duration: "1m 30s",
    wordCount: 180
  },
  {
    id: "3",
    title: "Project Roadmap Notes",
    preview: "Phase 1 should focus on infrastructure stability before we move to...",
    date: "Oct 24",
    duration: "12m 05s",
    wordCount: 1450
  }
];
