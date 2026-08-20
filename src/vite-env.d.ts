/**
 * SOURCE OF TRUTH KEYWORDS: vite-env, ImportMetaEnv, vite-client-types
 * WHAT:  Pulls in Vite's ambient client types.
 * WHY:   Without it, import.meta.env and the ?url / ?raw import suffixes are
 *        untyped, and the strictness the rest of the codebase relies on has a
 *        hole in it at the module boundary.
 * WHERE: Referenced by tsconfig via the src include. Not authored — do not add
 *        declarations here; a shared type belongs in src-tauri/src/types/.
 */

/// <reference types="vite/client" />
