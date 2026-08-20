/**
 * SOURCE OF TRUTH KEYWORDS: pill-entry, PillEntry, createRoot, pill.html
 * WHAT:  The pill window's entry point. Mounts <Pill /> and nothing else.
 * WHY:   Its own Vite entry so its bundle contains no charts, no tables and no
 *        router — it must paint inside one frame while an audio thread and an
 *        inference worker are already running (docs/02 §9). Deliberately not
 *        wrapped in StrictMode: the double-invoked effects would subscribe to
 *        audio-level-changed twice in development, which is precisely the loop
 *        whose cost is being tuned here.
 * WHERE: Loaded by pill.html, which the Rust side opens as the NSPanel overlay.
 */

import { createRoot } from "react-dom/client";
import { Pill } from "@/app/pill/Pill";
import "@/styles/global.css";

const container = document.getElementById("root");
if (!container) throw new Error("pill.html is missing its #root element.");

createRoot(container).render(<Pill />);
