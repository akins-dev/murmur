#!/usr/bin/env node
/**
 * SOURCE OF TRUTH KEYWORDS: sot, sotSearch, parseSotHeaders, collectSourceFiles,
 *   SOT_MARKER, matchKeyword, renderFileList, renderHeaders
 * WHAT:  The SOURCE OF TRUTH keyword search CLI. Given a keyword, prints the files
 *        whose SOT header claims to own that symbol — `node sot.mjs <keyword>` —
 *        or those headers in full with `--show`.
 * WHY:   Keeps AI and developer context cost flat (O(1)) as the codebase grows.
 *        Answers "where does this concept live" in 1 second without opening 40 files.
 * WHERE: Located in scripts/; executed as `node scripts/sot.mjs <keyword>`.
 */

import { readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, extname, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const ROOT = process.cwd();

const SEARCH_ROOTS = ['src', 'src-tauri/src', 'lib', 'app', 'pkg', 'internal', 'scripts'];
const SOURCE_EXTENSIONS = new Set(['.rs', '.ts', '.tsx', '.js', '.jsx', '.mjs', '.go', '.py', '.sql', '.css']);
const IGNORED_DIRS = new Set(['node_modules', 'target', 'dist', '.git', 'gen', 'build', '.next', '__pycache__']);
const GENERATED_PATTERNS = [/bindings\.ts$/, /generated\./, /\.d\.ts$/];

const SOT_MARKER = 'SOURCE OF TRUTH KEYWORDS:';
const SECTION_LABEL = /^\s*(?:\*|\/\/|--|#)?\s*(WHAT|WHY|WHERE)\s*:/i;
const COMMENT_PREFIX = /^\s*(?:\*\/|\*|\/\*[*!]?|\/\/[!/]?|--|#)?\s?/;

function collectSourceFiles(dir, out = []) {
  let entries;
  try {
    entries = readdirSync(dir);
  } catch {
    return out;
  }
  for (const entry of entries) {
    if (IGNORED_DIRS.has(entry)) continue;
    const full = join(dir, entry);
    let stats;
    try {
      stats = statSync(full);
    } catch {
      continue;
    }
    if (stats.isDirectory()) {
      collectSourceFiles(full, out);
    } else if (SOURCE_EXTENSIONS.has(extname(entry))) {
      const rel = relative(ROOT, full).replace(/\\/g, '/');
      if (!GENERATED_PATTERNS.some((p) => p.test(rel))) out.push(full);
    }
  }
  return out;
}

function parseSotHeaders(filePath) {
  let text;
  try {
    text = readFileSync(filePath, 'utf8');
  } catch {
    return [];
  }
  const lines = text.split('\n');
  const headers = [];

  for (let i = 0; i < lines.length; i++) {
    const markerAt = lines[i].indexOf(SOT_MARKER);
    if (markerAt === -1) continue;

    const keywordParts = [lines[i].slice(markerAt + SOT_MARKER.length)];
    const block = [lines[i]];

    let j = i + 1;
    for (; j < lines.length; j++) {
      const line = lines[j];
      if (SECTION_LABEL.test(line) || line.includes('*/') || line.trim() === '') break;
      keywordParts.push(line.replace(COMMENT_PREFIX, ''));
      block.push(line);
    }
    for (; j < lines.length; j++) {
      const line = lines[j];
      block.push(line);
      if (line.includes('*/')) break;
      if (block.length > 40) break;
    }

    const keywords = keywordParts
      .join(' ')
      .split(',')
      .map((k) => k.replace(COMMENT_PREFIX, '').trim())
      .filter(Boolean);

    headers.push({ keywords, block, line: i + 1 });
    i = j;
  }
  return headers;
}

function matchKeyword(keywords, query) {
  const needle = query.toLowerCase();
  return keywords.some((k) => k.toLowerCase().includes(needle));
}

function main() {
  const argv = process.argv.slice(2);
  const show = argv.includes('--show');
  const query = argv.filter((a) => a !== '--show').join(' ').trim();

  if (!query) {
    console.error('Usage: node sot.mjs <keyword>        # lists files that own the symbol');
    console.error('       node sot.mjs --show <keyword> # prints matching files with headers');
    process.exit(1);
  }

  const searchDirs = SEARCH_ROOTS.map((r) => join(ROOT, r));
  const files = searchDirs.flatMap((root) => collectSourceFiles(root));
  const hits = [];

  for (const file of files) {
    const matched = parseSotHeaders(file).filter((h) => matchKeyword(h.keywords, query));
    if (matched.length) hits.push({ file: relative(ROOT, file).replace(/\\/g, '/'), headers: matched });
  }

  if (!hits.length) {
    console.log(`No SOURCE OF TRUTH header claims "${query}".`);
    console.log('Search the codebase normally before creating it — and give the');
    console.log('new file its own SOT header so the next agent finds it.');
    process.exit(0);
  }

  if (!show) {
    for (const hit of hits) console.log(hit.file);
    console.log(`\n${hits.length} file(s) matched. Read headers with --show before opening.`);
    return;
  }

  for (const hit of hits) {
    console.log(`\n\x1b[1m${hit.file}\x1b[0m`);
    for (const header of hit.headers) {
      console.log(`  \x1b[2mLine ${header.line}:\x1b[0m`);
      for (const line of header.block) console.log(`  ${line}`);
    }
  }
  console.log(`\n${hits.length} file(s) matched.`);
}

main();
