/**
 * SOURCE OF TRUTH KEYWORDS: onboarding-entry, OnboardingEntry, createRoot,
 *   onboarding.html
 * WHAT:  The onboarding window's entry point.
 * WHY:   Its own Vite entry so first run does not download the dashboard's
 *        charts and tables before the user has said a word (docs/02 §9).
 * WHERE: Loaded by onboarding.html, opened once on first launch.
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Onboarding } from "@/app/onboarding/Onboarding";
import "@/styles/global.css";

const container = document.getElementById("root");
if (!container) throw new Error("onboarding.html is missing its #root element.");

createRoot(container).render(
  <StrictMode>
    <Onboarding />
  </StrictMode>,
);
