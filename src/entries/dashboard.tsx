/**
 * SOURCE OF TRUTH KEYWORDS: dashboard-entry, DashboardEntry, createRoot, index.html
 * WHAT:  The dashboard window's entry point.
 * WHY:   Its own Vite entry so the pill never ships the charts, the tables or
 *        the settings machinery this window needs (docs/02 §9).
 * WHERE: Loaded by index.html — the "real" window of the app.
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";
import { Dashboard } from "@/app/dashboard/Dashboard";
import "@/styles/global.css";

const container = document.getElementById("root");
if (!container) throw new Error("index.html is missing its #root element.");

createRoot(container).render(
  <StrictMode>
    <Dashboard />
  </StrictMode>,
);
