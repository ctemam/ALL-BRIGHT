'use client';

import { useAccount, useConnect, useDisconnect, useBalance } from 'wagmi';
import { useCallback, useState } from 'react';

function AddressDisplay({ address }: { address: `0x${string}` }) {
  return (
    <span className="font-mono text-sm">
      {address.slice(0, 6)}...{address.slice(-4)}
    </span>
  );
}

function ChainBadge({ chainId }: { chainId: number }) {
  const names: Record<number, string> = {
    1: 'Ethereum',
    42161: 'Arbitrum',
    10: 'Optimism',
    137: 'Polygon',
    56: 'BSC',
    43114: 'Avalanche',
  };

  const colors: Record<number, string> = {
    1: 'bg-blue-500/20 text-blue-400',
    42161: 'bg-blue-600/20 text-blue-400',
    10: 'bg-red-500/20 text-red-400',
    137: 'bg-purple-500/20 text-purple-400',
    56: 'bg-yellow-500/20 text-yellow-400',
    43114: 'bg-red-400/20 text-red-400',
  };

  return (
    <span
      className={`px-2 py-0.5 rounded-full text-xs font-medium ${
        colors[chainId] ?? 'bg-gray-500/20 text-gray-400'
      }`}
    >
      {names[chainId] ?? `Chain ${chainId}`}
    </span>
  );
}

export function WalletConnect() {
  const { address, isConnected, chainId } = useAccount();
  const { connectors, connect } = useConnect();
  const { disconnect } = useDisconnect();
  const { data: balance } = useBalance({ address });
  const [connecting, setConnecting] = useState(false);

  const handleConnect = useCallback(
    async (connector: (typeof connectors)[0]) => {
      setConnecting(true);
      try {
        await connect({ connector });
      } catch {
        // User rejected or connection failed
      } finally {
        setConnecting(false);
      }
    },
    [connect, connectors]
  );

  if (isConnected && address) {
    return (
      <div className="flex items-center gap-3">
        {chainId && <ChainBadge chainId={chainId} />}
        <div className="flex flex-col items-end">
          <AddressDisplay address={address} />
          {balance && (
            <span className="text-xs text-gray-400">
              {parseFloat(balance.formatted).toFixed(4)} {balance.symbol}
            </span>
          )}
        </div>
        <button
          onClick={() => disconnect()}
          className="px-3 py-1.5 text-xs font-medium text-gray-400 hover:text-white 
                     transition-colors border border-white/10 rounded-lg hover:border-white/20"
        >
          Disconnect
        </button>
      </div>
    );
  }

  return (
    <div className="flex items-center gap-2">
      {connectors
        .filter((c) => c.id === 'metaMaskSDK' || c.name === 'MetaMask')
        .slice(0, 1)
        .map((connector) => (
          <button
            key={connector.id}
            onClick={() => handleConnect(connector)}
            disabled={connecting}
            className="relative group px-5 py-2 rounded-xl text-sm font-semibold 
                       bg-gradient-to-r from-indigo-600 to-purple-600 
                       hover:from-indigo-500 hover:to-purple-500 
                       transition-all duration-200 disabled:opacity-50
                       neo-glow"
          >
            <span className="flex items-center gap-2">
              <svg width="16" height="16" viewBox="0 0 35 33" fill="none">
                <path
                  d="M32.9582 1L19.8241 10.5323L22.4732 5.50508L32.9582 1Z"
                  fill="#E2761B"
                  stroke="#E2761B"
                />
                <path
                  d="M2.04175 1L15.0766 10.6122L12.5268 5.50508L2.04175 1Z"
                  fill="#E4761B"
                  stroke="#E4761B"
                />
                <path
                  d="M28.1316 23.5184L24.7858 28.6435L32.1779 30.7035L34.3745 23.6171L28.1316 23.5184Z"
                  fill="#E4761B"
                  stroke="#E4761B"
                />
                <path
                  d="M0.625488 23.6171L2.82209 30.7035L10.2142 28.6435L6.86837 23.5184L0.625488 23.6171Z"
                  fill="#E4761B"
                  stroke="#E4761B"
                />
                <path
                  d="M9.79175 14.624L7.83325 17.7123L15.1749 18.0366L14.8758 9.99512L9.79175 14.624Z"
                  fill="#E4761B"
                  stroke="#E4761B"
                />
                <path
                  d="M25.2083 14.624L20.0249 9.91528L19.8251 18.0366L27.1667 17.7123L25.2083 14.624Z"
                  fill="#E4761B"
                  stroke="#E4761B"
                />
                <path
                  d="M10.2142 28.6435L14.6299 26.2835L10.8141 23.3307L10.2142 28.6435Z"
                  fill="#D7C1B3"
                  stroke="#D7C1B3"
                />
                <path
                  d="M20.37 26.2835L24.7858 28.6435L24.1859 23.3307L20.37 26.2835Z"
                  fill="#D7C1B3"
                  stroke="#D7C1B3"
                />
                <path
                  d="M24.7858 28.6436L20.37 26.2836L20.7264 29.1015L20.696 30.4705L24.7858 28.6436Z"
                  fill="#C0AD9E"
                  stroke="#C0AD9E"
                />
                <path
                  d="M10.2142 28.6436L14.304 30.4705L14.2736 29.1015L14.63 26.2836L10.2142 28.6436Z"
                  fill="#C0AD9E"
                  stroke="#C0AD9E"
                />
              </svg>
              {connecting ? 'Connecting...' : 'Connect MetaMask'}
            </span>
          </button>
        ))}
    </div>
  );
}
