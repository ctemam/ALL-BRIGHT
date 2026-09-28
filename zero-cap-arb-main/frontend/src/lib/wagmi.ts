import { http, createConfig } from 'wagmi';
import { mainnet, arbitrum, optimism, polygon, bsc, avalanche } from 'wagmi/chains';
import { walletConnect, metaMask, injected } from 'wagmi/connectors';

export const SUPPORTED_CHAINS = [
  mainnet,
  arbitrum,
  optimism,
  polygon,
  bsc,
  avalanche,
] as const;

export const wagmiConfig = createConfig({
  chains: SUPPORTED_CHAINS,
  connectors: [
    metaMask({
      shimDisconnect: true,
    }),
    walletConnect({
      projectId: process.env.NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID ?? '',
      showQrModal: true,
    }),
    injected({ shimDisconnect: true }),
  ],
  transports: {
    [mainnet.id]: http(
      process.env.NEXT_PUBLIC_ETH_RPC_URL ?? 'https://eth.merkle.io'
    ),
    [arbitrum.id]: http(
      process.env.NEXT_PUBLIC_ARB_RPC_URL ?? 'https://arb1.arbitrum.io/rpc'
    ),
    [optimism.id]: http(
      process.env.NEXT_PUBLIC_OP_RPC_URL ?? 'https://mainnet.optimism.io'
    ),
    [polygon.id]: http(
      process.env.NEXT_PUBLIC_POLY_RPC_URL ?? 'https://polygon-rpc.com'
    ),
    [bsc.id]: http(
      process.env.NEXT_PUBLIC_BSC_RPC_URL ?? 'https://bsc-dataseed.binance.org'
    ),
    [avalanche.id]: http(
      process.env.NEXT_PUBLIC_AVAX_RPC_URL ?? 'https://api.avax.network/ext/bc/C/rpc'
    ),
  },
});

export const CHAIN_LOGOS: Record<number, string> = {
  [mainnet.id]: 'ETH',
  [arbitrum.id]: 'ARB',
  [optimism.id]: 'OP',
  [polygon.id]: 'MATIC',
  [bsc.id]: 'BNB',
  [avalanche.id]: 'AVAX',
};
