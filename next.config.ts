import type { NextConfig } from "next";

const nextConfig: NextConfig = {
  output: "standalone",
  // better-sqlite3 is a native module; must not be bundled by webpack/turbopack.
  serverExternalPackages: ["better-sqlite3"],
};

export default nextConfig;
