/**
 * SOURCE OF TRUTH KEYWORDS: GlassPanel, GlassPanelProps, GlassMaterial,
 *   GlassRadius, MATERIAL_CLASS, RADIUS_CLASS, glass-noise
 * WHAT:  The surface primitive. Renders one of the three materials from
 *        docs/04 §3 with its hairline, inner top highlight, shadow and the 3%
 *        noise layer, and nothing else.
 * WHY:   Every glass surface in the app is the same four-part treatment, so it
 *        is authored once here and picked by name. The noise sits at z-index
 *        -10 inside an isolated stacking context: that is what puts it above
 *        the material and below content without wrapping children in an extra
 *        div, which would break any flex layout the caller sets on the panel.
 *        The material itself is native NSVisualEffectView vibrancy behind the
 *        webview — this component only contributes the web layer's share.
 * WHERE: The base of every surface: the pill window, the dashboard sidebar,
 *        menus and sheets. Materials live as utilities in styles/global.css.
 */

import type { ComponentPropsWithoutRef, Ref } from "react";
import { cn } from "@/lib/utils";

export type GlassMaterial = "pill" | "panel" | "elevated";
export type GlassRadius = "pill" | "panel" | "card" | "input" | "none";

export interface GlassPanelProps extends ComponentPropsWithoutRef<"div"> {
  /** Which of the three materials (docs/04 §3). Defaults to the panel material. */
  material?: GlassMaterial;
  radius?: GlassRadius;
  /** The 3% noise layer. On by default — off only for surfaces small enough
   *  that the texture is invisible and the extra layer is not worth it. */
  noise?: boolean;
  ref?: Ref<HTMLDivElement>;
}

const MATERIAL_CLASS: Readonly<Record<GlassMaterial, string>> = {
  pill: "material-pill",
  panel: "material-panel",
  elevated: "material-elevated",
};

const RADIUS_CLASS: Readonly<Record<GlassRadius, string>> = {
  pill: "rounded-pill",
  panel: "rounded-panel",
  card: "rounded-card",
  input: "rounded-input",
  none: "",
};

export function GlassPanel({
  material = "panel",
  radius = "panel",
  noise = true,
  className,
  children,
  ...rest
}: GlassPanelProps) {
  return (
    <div
      className={cn("relative isolate overflow-hidden", MATERIAL_CLASS[material], RADIUS_CLASS[radius], className)}
      {...rest}
    >
      {noise ? <span aria-hidden="true" className="glass-noise absolute inset-0 -z-10" /> : null}
      {children}
    </div>
  );
}
