import type { NextConfig } from "next";

// `output: "export"` produces a static `out/` directory consumed by the
// Tauri webview (`desktop/src-tauri/tauri.conf.json::build.frontendDist`).
// The web dev workflow is unchanged — `next dev` still works, and a future
// SaaS deployment can drop `output` to re-enable SSR/route handlers. There
// are no API routes today and all data flows client-side to the Rust core
// via WS+HTTP, so static export is loss-free for the current frontend.
const config: NextConfig = {
  reactStrictMode: true,
  output: "export",
  // next/image's default loader expects a runtime; static export needs
  // `unoptimized` so it falls back to raw <img>. We don't use next/image
  // today, so this is purely defensive.
  images: { unoptimized: true },
  // Tauri serves the static export from a flat asset host; trailing
  // slashes keep relative paths resolving under both file:// and
  // tauri://localhost without bespoke rewrites.
  trailingSlash: true,
};

export default config;
