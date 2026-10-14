/**
 * Static export configuration (`output: 'export'`) — 100% static HTML/CSS/JS,
 * zero server dependency, and zero key custody.
 */
export interface NextExportConfig {
  output: 'export';
  images: { unoptimized: boolean };
  trailingSlash: boolean;
}

const nextConfig: NextExportConfig = {
  output: 'export',
  images: { unoptimized: true },
  trailingSlash: true,
};

export default nextConfig;
