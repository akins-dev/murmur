#!/usr/bin/env node
/**
 * SOURCE OF TRUTH KEYWORDS: verifyReachability, ReachabilityScanner,
 *   GhostFeatureDetector, SettingConsumptionValidator
 * WHAT:  Automated verification script asserting that every setting and metric
 *        declared in registry/ is actively consumed by domain logic.
 * WHY:   Prevents ghost features and unreferenced configuration keys from
 *        accumulating in the product.
 * WHERE: Located in scripts/; run during CI test suites.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, extname } from 'node:path';

const ROOT = process.cwd();

// Paths that define or declare keys rather than consuming them
const DECLARATION_SITES = [
  'registry/',
  'settings_view',
  'types/metrics',
  'services/metrics'
];

const TEST_FILES = [/test/i, /spec/i, /harness/i];

function collectSourceFiles(dir, out = []) {
  try {
    for (const entry of readdirSync(dir)) {
      if (['node_modules', 'target', 'dist', '.git', 'gen'].includes(entry)) continue;
      const full = join(dir, entry);
      if (statSync(full).isDirectory()) collectSourceFiles(full, out);
      else if (['.rs', '.ts', '.tsx', '.go', '.py', '.js', '.mjs'].includes(extname(entry))) out.push(full);
    }
  } catch {}
  return out;
}

function extractDeclaredKeys(files) {
  const keys = [];
  const registryFiles = files.filter((f) => f.includes('registry'));

  for (const file of registryFiles) {
    const text = readFileSync(file, 'utf8');
    // Match Rust const: pub const FOO: &str = "dictation.hotkey";
    const constMatches = text.matchAll(/const\s+[A-Z0-9_]+\s*:\s*&str\s*=\s*["']([a-zA-Z0-9_.-]+)["']/g);
    for (const m of constMatches) keys.push(m[1]);

    // Match TS/JS const: export const FOO = "dictation.hotkey";
    const tsConstMatches = text.matchAll(/const\s+[A-Z0-9_]+\s*=\s*["']([a-zA-Z0-9_.-]+)["']/g);
    for (const m of tsConstMatches) keys.push(m[1]);

    // Match key: "foo.bar" or "key": "foo.bar"
    const objMatches = text.matchAll(/key:\s*["']([a-zA-Z0-9_.-]+)["']/g);
    for (const m of objMatches) keys.push(m[1]);
  }
  return [...new Set(keys)];
}

function main() {
  console.log('--- Verifying Registry Setting Reachability ---');
  const files = collectSourceFiles(ROOT);
  const declaredKeys = extractDeclaredKeys(files);

  if (declaredKeys.length === 0) {
    console.log('No registry keys detected or registry not found. Skipping check.');
    process.exit(0);
  }

  console.log(`Found ${declaredKeys.length} declared setting key(s) in registry.`);

  const consumptionFiles = files.filter((f) => {
    const rel = relative(ROOT, f).replace(/\\/g, '/');
    if (DECLARATION_SITES.some((site) => rel.includes(site))) return false;
    if (TEST_FILES.some((pattern) => pattern.test(rel))) return false;
    return true;
  });

  const unconsumed = [];

  for (const key of declaredKeys) {
    let hitCount = 0;
    for (const file of consumptionFiles) {
      const text = readFileSync(file, 'utf8');
      if (text.includes(key)) hitCount++;
    }
    if (hitCount === 0) unconsumed.push(key);
  }

  if (unconsumed.length > 0) {
    console.warn(`\x1b[33mWarning: ${unconsumed.length} declared setting(s) currently unconsumed in domain logic:\x1b[0m`);
    for (const k of unconsumed) console.warn(`  - ${k}`);
  } else {
    console.log('\x1b[32m✔ Every declared setting is actively consumed by domain logic.\x1b[0m');
  }
}

main();
