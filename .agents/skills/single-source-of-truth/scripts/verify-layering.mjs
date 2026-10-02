#!/usr/bin/env node
/**
 * SOURCE OF TRUTH KEYWORDS: verifyLayering, LayerValidator, UpwardImports,
 *   LayerOrder, SelfCleaningAllowlist
 * WHAT:  Automated guardrail script enforcing downward-only architectural layers.
 * WHY:   Prevents circular dependencies and structural decay in CI. Fails if
 *        an import points upward or if an allowlisted exception was cleaned up.
 * WHERE: Located in scripts/; run during CI and pre-commit checks.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, extname } from 'node:path';

const ROOT = process.cwd();

// Lowest layer is index 0. Modules may only import their own layer or lower.
const LAYER_ORDER = [
  ['config', 'db', 'error', 'telemetry', 'types', 'infra'],
  ['ports'],
  ['adapters'],
  ['registry'],
  ['pipeline', 'services', 'session', 'domain'],
  ['bootstrap', 'ipc', 'tray', 'controllers', 'cli', 'app']
];

// Documented exceptions that must be cleaned up over time.
const KNOWN_UPWARD_IMPORTS = [
  // Example: { file: 'session/actor.rs', module: 'ipc', reason: 'SessionContext migration in progress' }
];

function getLayer(moduleName) {
  for (let i = 0; i < LAYER_ORDER.length; i++) {
    if (LAYER_ORDER[i].includes(moduleName)) return i;
  }
  return -1;
}

function scanImports(filePath) {
  const content = readFileSync(filePath, 'utf8');
  const imports = [];
  const lines = content.split('\n');

  for (const line of lines) {
    const trimmed = line.trim();
    // Rust crate imports: use crate::<module>
    const rustMatch = trimmed.match(/^use\s+crate::([a-zA-Z0-9_]+)/);
    if (rustMatch) imports.push(rustMatch[1]);

    // TypeScript/ESM imports: import ... from '@/module' or relative
    const tsMatch = trimmed.match(/from\s+['"]@\/([a-zA-Z0-9_\-]+)/);
    if (tsMatch) imports.push(tsMatch[1]);
  }
  return imports;
}

function collectFiles(dir, out = []) {
  try {
    for (const entry of readdirSync(dir)) {
      if (['node_modules', 'target', 'dist', '.git'].includes(entry)) continue;
      const full = join(dir, entry);
      if (statSync(full).isDirectory()) collectFiles(full, out);
      else if (['.rs', '.ts', '.tsx', '.go'].includes(extname(entry))) out.push(full);
    }
  } catch {}
  return out;
}

function main() {
  console.log('--- Verifying Downward-Only Layering ---');
  const files = collectFiles(ROOT);
  const violations = [];
  const foundExceptions = new Set();

  for (const file of files) {
    const rel = relative(ROOT, file).replace(/\\/g, '/');
    const parts = rel.split('/');
    // Extract top-level module under src or root
    let mod = parts[0];
    if (parts[0] === 'src' || parts[0] === 'src-tauri') mod = parts[1];
    if (!mod) continue;

    const fileLayer = getLayer(mod);
    if (fileLayer === -1) continue;

    const importedModules = scanImports(file);
    for (const imported of importedModules) {
      const importedLayer = getLayer(imported);
      if (importedLayer > fileLayer) {
        const isExcepted = KNOWN_UPWARD_IMPORTS.some(
          (ex) => rel.includes(ex.file) && ex.module === imported
        );
        if (isExcepted) {
          foundExceptions.add(`${rel}:${imported}`);
        } else {
          violations.push(`${rel} (layer ${fileLayer}) imports ${imported} (layer ${importedLayer})`);
        }
      }
    }
  }

  // Check for stale exceptions
  for (const ex of KNOWN_UPWARD_IMPORTS) {
    const key = `${ex.file}:${ex.module}`;
    if (![...foundExceptions].some((f) => f.includes(ex.file) && f.includes(ex.module))) {
      console.error(`\x1b[31mStale exception in KNOWN_UPWARD_IMPORTS: ${ex.file} -> ${ex.module}\x1b[0m`);
      console.error('The violation no longer exists. Remove this entry to keep the allowlist honest.');
      process.exit(1);
    }
  }

  if (violations.length > 0) {
    console.error('\x1b[31mLayering violations found (Upward Imports Prohibited):\x1b[0m');
    for (const v of violations) console.error(`  - ${v}`);
    console.error('\nMove shared types/contracts DOWN to a common lower layer.');
    process.exit(1);
  }

  console.log('\x1b[32m✔ All architectural layer dependencies flow strictly downward.\x1b[0m');
}

main();
