'use client';

import { useMemo, useState, useCallback } from 'react';
import { BubbleData } from '@/lib/api';

interface CryptoBubbleChartProps {
  data: BubbleData[];
  onTokenClick?: (token: BubbleData) => void;
  rtl?: boolean;
}

const CHAIN_COLORS: Record<string, string> = {
  ethereum: '#627EEA',
  bsc: '#F0B90B',
  polygon: '#8247E5',
  arbitrum: '#2D374B',
  optimism: '#FF0420',
  avalanche: '#E84142',
  solana: '#9945FF',
  fantom: '#1969FF',
  base: '#0052FF',
  celo: '#35D07F',
  tron: '#FF0600',
  gnosis: '#04795B',
  cronos: '#002D74',
  moonbeam: '#F2B705',
  linea: '#121212',
  scroll: '#FFE699',
  zksync: '#4E529A',
  polygon_zkevm: '#8247E5',
};

const DEFAULT_CHAIN_COLOR = '#8B8B8B';

function formatLiquidity(value: number): string {
  if (value >= 1_000_000_000) return `$${(value / 1_000_000_000).toFixed(1)}B`;
  if (value >= 1_000_000) return `$${(value / 1_000_000).toFixed(1)}M`;
  if (value >= 1_000) return `$${(value / 1_000).toFixed(0)}K`;
  return `$${value.toFixed(0)}`;
}

function formatPrice(value: number): string {
  if (value < 0.0001) return `$${value.toExponential(3)}`;
  if (value < 1) return `$${value.toFixed(6)}`;
  if (value < 1000) return `$${value.toFixed(2)}`;
  return `$${value.toLocaleString('en-US', { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
}

function CryptoBubbleChart({ data, onTokenClick, rtl }: CryptoBubbleChartProps) {
  const [hoveredToken, setHoveredToken] = useState<string | null>(null);
  const [tooltipPos, setTooltipPos] = useState({ x: 0, y: 0 });

  const { minSize, maxSize, chainList, totalLiquidity, opportunitiesCount, tokensByChain } = useMemo(() => {
    const sizes = data.map((t) => t.bubble_size);
    const min = sizes.length ? Math.min(...sizes) : 1;
    const max = sizes.length ? Math.max(...sizes) : 1;
    const chains = [...new Set(data.map((t) => t.chain_name))].sort();

    const byChain = new Map<string, BubbleData[]>();
    for (const token of data) {
      const list = byChain.get(token.chain_name) ?? [];
      list.push(token);
      byChain.set(token.chain_name, list);
    }

    const totalLiq = data.reduce((sum, t) => sum + t.liquidity_usd, 0);
    const oppCount = data.filter((t) => t.has_opportunity).length;

    return { minSize: min, maxSize: max, chainList: chains, totalLiquidity: totalLiq, opportunitiesCount: oppCount, tokensByChain: byChain };
  }, [data]);

  const getNormalizedSize = useCallback(
    (bubbleSize: number) => {
      const range = maxSize - minSize || 1;
      const ratio = (bubbleSize - minSize) / range;
      const mobile = 2.5 + ratio * 3.5;
      const desktop = 3.5 + ratio * 6.5;
      return { mobile, desktop };
    },
    [minSize, maxSize],
  );

  const handleMouseEnter = useCallback((e: React.MouseEvent, tokenId: string) => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    setTooltipPos({ x: rect.left + rect.width / 2, y: rect.top });
    setHoveredToken(tokenId);
  }, []);

  const handleMouseLeave = useCallback(() => setHoveredToken(null), []);

  const handleClick = useCallback((token: BubbleData) => onTokenClick?.(token), [onTokenClick]);

  const hovered = hoveredToken ? data.find((t) => t.token === hoveredToken) : null;

  return (
    <div className={`space-y-5 ${rtl ? 'text-right' : 'text-left'}`} dir={rtl ? 'rtl' : 'ltr'}>
      {/* Summary */}
      <div className="glass-card flex flex-wrap items-center gap-x-6 gap-y-2 p-4 text-sm">
        <span className="text-gray-300">
          <span className="font-semibold text-white">{data.length}</span> tokens tracked
        </span>
        <span className="text-gray-600">|</span>
        <span className="text-gray-300">
          <span className="font-semibold text-emerald-400">{opportunitiesCount}</span> with{' '}
          <span className="text-emerald-400">opportunities</span>
        </span>
        <span className="text-gray-600">|</span>
        <span className="text-gray-300">
          <span className="font-semibold text-indigo-300">{formatLiquidity(totalLiquidity)}</span> total liquidity
        </span>
      </div>

      {/* Bubble Grid */}
      <div className="glass-card p-6">
        <div className="flex flex-wrap items-center justify-center gap-4 sm:gap-6">
          {data.map((token) => {
            const sizes = getNormalizedSize(token.bubble_size);
            const color = CHAIN_COLORS[token.chain_name.toLowerCase()] ?? DEFAULT_CHAIN_COLOR;
            const sizeClamp = `clamp(2.5rem, ${sizes.desktop}rem, 10rem)`;

            return (
              <div
                key={token.token}
                className="relative flex cursor-pointer flex-col items-center transition-transform duration-200 hover:z-10 hover:scale-110"
                style={{ width: sizeClamp }}
                onMouseEnter={(e) => handleMouseEnter(e, token.token)}
                onMouseLeave={handleMouseLeave}
                onClick={() => handleClick(token)}
              >
                <div
                  className="flex items-center justify-center rounded-full transition-all duration-300"
                  style={{
                    width: sizeClamp,
                    height: sizeClamp,
                    background: `radial-gradient(circle at 30% 35%, ${color}bb, ${color}33)`,
                    boxShadow: token.has_opportunity
                      ? `0 0 12px ${color}66, 0 0 0 3px rgba(52,211,153,0.7), 0 0 20px rgba(52,211,153,0.3)`
                      : `0 0 12px ${color}44`,
                    animation: token.has_opportunity ? 'opp-pulse 2s ease-in-out infinite' : undefined,
                  }}
                >
                  <span className="select-none text-center text-[0.5rem] font-bold leading-tight text-white sm:text-xs">
                    {token.symbol}
                  </span>
                </div>
              </div>
            );
          })}
        </div>
      </div>

      {/* Legend */}
      <div className="glass-card p-4">
        <h4 className="mb-3 text-[10px] font-semibold uppercase tracking-widest text-gray-500">Chains</h4>
        <div className="flex flex-wrap gap-x-5 gap-y-2">
          {chainList.map((chain) => {
            const color = CHAIN_COLORS[chain.toLowerCase()] ?? DEFAULT_CHAIN_COLOR;

            return (
              <div key={chain} className="flex items-center gap-1.5 text-xs">
                <span className="h-2 w-2 rounded-full" style={{ backgroundColor: color }} />
                <span className="text-gray-300">{chain}</span>
                <span className="text-gray-600">({tokensByChain.get(chain)?.length ?? 0})</span>
              </div>
            );
          })}
        </div>
      </div>

      {/* Tooltip */}
      {hovered && (
        <div
          className="pointer-events-none fixed z-50 rounded-xl bg-white/5 p-3 text-xs shadow-xl backdrop-blur-xl"
          style={{
            left: tooltipPos.x,
            top: tooltipPos.y,
            transform: 'translate(-50%, -110%)',
            border: '1px solid rgba(255,255,255,0.06)',
          }}
        >
          <p className="mb-1.5 text-sm font-bold text-white">{hovered.symbol}</p>
          <div className="space-y-0.5 text-gray-400">
            <p>
              Price: <span className="font-mono text-white">{formatPrice(hovered.price_usd)}</span>
            </p>
            <p>
              Liquidity: <span className="font-mono text-white">{formatLiquidity(hovered.liquidity_usd)}</span>
            </p>
            <p>
              Best Spread:{' '}
              <span className="font-mono text-emerald-400">{hovered.best_spread_pct.toFixed(2)}%</span>
            </p>
            <p>
              DEXes: <span className="font-mono text-white">{hovered.dexes_available.length}</span>
            </p>
            <p>
              Chain: <span className="text-indigo-300">{hovered.chain_name}</span>
            </p>
          </div>
        </div>
      )}

      <style jsx>{`
        @keyframes opp-pulse {
          0%,
          100% {
            filter: brightness(1) saturate(1);
          }
          50% {
            filter: brightness(1.3) saturate(1.3);
          }
        }
      `}</style>
    </div>
  );
}

export default CryptoBubbleChart;
