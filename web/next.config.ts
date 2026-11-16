import type { NextConfig } from "next";

// Static export: the engine runs in the browser as WebAssembly and the
// conformance numbers are precomputed JSON, so no server is needed.
const nextConfig: NextConfig = {
  output: "export",
  trailingSlash: true,
  poweredByHeader: false,
  agentRules: false,
};

export default nextConfig;
