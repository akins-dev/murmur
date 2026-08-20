/**
 * SOURCE OF TRUTH KEYWORDS: iconFor, ICONS, LucideIcon, normaliseIconName, FALLBACK_ICON
 * WHAT:  Resolves the registry's `nav.icon` string to a Lucide component.
 * WHY:   The registry names an icon and the frontend resolves it (NavDef), so
 *        the nav is never hardcoded. Resolution is a named-import map rather
 *        than `import * as icons`: the star import defeats tree-shaking and
 *        drags all 1,500 icons into the bundle for the sake of the six the app
 *        uses. An unknown name degrades to an obviously-wrong glyph instead of
 *        throwing — a nav item with the wrong icon is a blemish, a nav that
 *        crashes is an unusable window. Every name the registry actually
 *        declares is asserted to resolve by a test, so the fallback is a safety
 *        net rather than something a real build is ever allowed to reach.
 * WHERE: The dashboard sidebar. Names are matched case- and separator-insensitively,
 *        so "bar-chart", "BarChart" and "bar_chart" all resolve.
 */

import {
  BookMarked,
  BookOpen,
  Boxes,
  ChartColumn,
  ChartLine,
  ChartNoAxesColumn,
  Circle,
  CircleDot,
  CircleHelp,
  Clock,
  Cpu,
  CreditCard,
  Database,
  Gauge,
  Keyboard,
  Languages,
  Mic,
  Package,
  RefreshCw,
  Settings,
  ShieldCheck,
  SlidersHorizontal,
  Sparkles,
  Type,
  WandSparkles,
  Zap,
  type LucideIcon,
} from "lucide-react";

/**
 * WHAT:  What an unresolvable icon name renders as.
 * WHY:   It used to be CircleDot, and that is precisely why two wrong icons
 *        shipped: a neat circle with a dot in it looks like a DESIGNED icon, so
 *        Billing and Stats sat in the sidebar wearing a plausible glyph until
 *        the operator noticed. A fallback's job here is not to look acceptable,
 *        it is to look WRONG — a question mark is read as "this is missing" in
 *        about a second, by anyone, without checking anything.
 *
 *        It still falls back rather than throwing: a nav item with the wrong
 *        icon is a blemish, a nav that crashes is an unusable window. The
 *        guarantee that it never renders at all is a test, not a throw — see
 *        src-tauri/src/registry/icons.rs.
 */
const FALLBACK_ICON: LucideIcon = CircleHelp;

const ICONS: Readonly<Record<string, LucideIcon>> = {
  bookmarked: BookMarked,
  bookopen: BookOpen,
  boxes: Boxes,
  chartcolumn: ChartColumn,
  chartline: ChartLine,
  chartnoaxescolumn: ChartNoAxesColumn,
  barchart: ChartColumn,
  barchart3: ChartColumn,
  linechart: ChartLine,
  circle: Circle,
  circledot: CircleDot,
  circlehelp: CircleHelp,
  clock: Clock,
  history: Clock,
  cpu: Cpu,
  creditcard: CreditCard,
  database: Database,
  gauge: Gauge,
  keyboard: Keyboard,
  languages: Languages,
  mic: Mic,
  microphone: Mic,
  package: Package,
  refreshcw: RefreshCw,
  settings: Settings,
  shieldcheck: ShieldCheck,
  slidershorizontal: SlidersHorizontal,
  sliders: SlidersHorizontal,
  sparkles: Sparkles,
  type: Type,
  wandsparkles: WandSparkles,
  zap: Zap,
};

function normaliseIconName(name: string): string {
  return name.toLowerCase().replace(/[^a-z0-9]/g, "");
}

export function iconFor(name: string): LucideIcon {
  return ICONS[normaliseIconName(name)] ?? FALLBACK_ICON;
}

export type { LucideIcon };
