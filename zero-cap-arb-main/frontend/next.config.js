/** @type {import('next').NextConfig} */

// Dependencies that are legitimately absent from a browser bundle. Both arrive only
// transitively (wagmi -> @wagmi/connectors -> @metamask/sdk and -> @walletconnect/*)
// and are already guarded at runtime, so stubbing them is safe and keeps the build clean.
//
//  - @react-native-async-storage/async-storage: required by MetaMask SDK's
//    getReactNativeAnonId(). getAnonId() dispatches to getBrowserAnonId() (localStorage)
//    whenever platformManager.isBrowser() is true, so the require is never reached in a
//    browser, and it sits inside a try/catch that falls back to a random ID.
//  - pino-pretty: an optional dev-time log pretty-printer that pino requires only when
//    configured with a pretty transport target; unreachable in a client bundle.
const optionalBrowserOnlyModules = [
  '@react-native-async-storage/async-storage',
  'pino-pretty',
];

const nextConfig = {
  output: 'standalone',
  poweredByHeader: false,
  reactStrictMode: true,
  webpack: (config) => {
    for (const name of optionalBrowserOnlyModules) {
      // `false` makes webpack substitute an empty module instead of failing resolution.
      config.resolve.alias[name] = false;
    }
    return config;
  },
  async rewrites() {
    return [
      {
        source: '/api/:path*',
        destination: 'http://localhost:3001/api/:path*',
      },
    ];
  },
};

module.exports = nextConfig;
