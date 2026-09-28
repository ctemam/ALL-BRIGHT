'use client';

import { useMemo, useState } from 'react';
import { LiquidityMapResponse, LiquidityChainSummary } from '@/lib/api';

interface DexInfo {
  name: string;
  liquidity: number;
  tokenCount?: number;
  fee?: string;
}

interface Props {
  data: LiquidityMapResponse;
  rtl?: boolean;
}

const usdFormatter = (value: number) =>
  new Intl.NumberFormat('en-US', {
    style: 'currency',
    currency: 'USD',
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(value);

const compactUsd = (value: number): string => {
  if (value >= 1_000_000_000) return `$${(value / 1_000_000_000).toFixed(2)}B`;
  if (value >= 1_000_000) return `$${(value / 1_000_000).toFixed(2)}M`;
  if (value >= 1_000) return `$${(value / 1_000).toFixed(1)}K`;
  return usdFormatter(value);
};

const CHAIN_ICONS: Record<string, string> = {
  Ethereum: 'ETH',
  'BNB Chain': 'BNB',
  Polygon: 'MATIC',
  Arbitrum: 'ARB',
  Optimism: 'OP',
  Avalanche: 'AVAX',
  Solana: 'SOL',
  Base: 'BASE',
  Fantom: 'FTM',
  Cronos: 'CRO',
};

const chainColors = [
  'from-indigo-500 to-purple-600',
  'from-cyan-500 to-blue-600',
  'from-emerald-500 to-teal-600',
  'from-rose-500 to-pink-600',
  'from-amber-500 to-orange-600',
  'from-violet-500 to-fuchsia-600',
  'from-sky-500 to-indigo-600',
  'from-lime-500 to-green-600',
];

const getGradient = (index: number) => chainColors[index % chainColors.length];

export default function LiquidityMap({ data, rtl }: Props) {
  const [expandedChains, setExpandedChains] = useState<Set<string>>(new Set());

  const sortedChains = useMemo(() => {
    if (!data?.chains) return [];
    return [...data.chains].sort(
      (a, b) => b.totalLiquidity - a.totalLiquidity
    );
  }, [data]);

  const totalLiquidity = useMemo(
    () => sortedChains.reduce((sum, c) => sum + c.totalLiquidity, 0),
    [sortedChains]
  );

  const chainCount = sortedChains.length;

  const toggleChain = (chainName: string) => {
    setExpandedChains((prev) => {
      const next = new Set(prev);
      if (next.has(chainName)) next.delete(chainName);
      else next.add(chainName);
      return next;
    });
  };

  const getDexList = (
    chain: LiquidityChainSummary
  ): { dexes: DexInfo[]; total: number } => {
    if ('dexes' in chain && Array.isArray((chain as any).dexes)) {
      const dexes = (chain as any).dexes as DexInfo[];
      const sorted = [...dexes].sort((a, b) => b.liquidity - a.liquidity);
      return { dexes: sorted, total: chain.totalLiquidity };
    }
    return { dexes: [], total: chain.totalLiquidity };
  };

  return (
    <div
      dir={rtl ? 'rtl' : 'ltr'}
      className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6"
    >
      <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4 mb-8">
        <h2 className="text-xl font-semibold text-white">
          {rtl ? 'توزيع السيولة' : 'Liquidity Distribution'}
        </h2>
        <div className="text-sm text-white/60">
          {rtl
            ? `إجمالي السيولة ${compactUsd(totalLiquidity)} عبر ${chainCount} شبكات`
            : `${compactUsd(totalLiquidity)} Total Liquidity Across ${chainCount} Chains`}
        </div>
      </div>

      <div className="grid grid-cols-1 lg:grid-cols-2 gap-8">
        <div className="space-y-5">
          {sortedChains.map((chain, index) => {
            const percentage =
              totalLiquidity > 0
                ? (chain.totalLiquidity / totalLiquidity) * 100
                : 0;
            const icon =
              CHAIN_ICONS[chain.chainName] ??
              chain.chainName.slice(0, 3).toUpperCase();
            const { dexes } = getDexList(chain);
            const isExpanded = expandedChains.has(chain.chainName);
            const displayDexes = isExpanded
              ? dexes
              : dexes.slice(0, 5);
            const hasMore = dexes.length > 5;

            return (
              <button
                key={chain.chainName}
                onClick={() => toggleChain(chain.chainName)}
                className="w-full text-left"
              >
                <div className="bg-white/[0.03] hover:bg-white/[0.06] transition-colors rounded-xl p-4 border border-white/5">
                  <div className="flex items-center justify-between mb-3">
                    <div className="flex items-center gap-3">
                      <span className="w-8 h-8 rounded-full bg-white/10 flex items-center justify-center text-xs font-bold text-white/80">
                        {icon}
                      </span>
                      <span className="text-sm font-medium text-white">
                        {chain.chainName}
                      </span>
                    </div>
                    <span className="text-sm font-semibold text-white">
                      {compactUsd(chain.totalLiquidity)}
                    </span>
                  </div>

                  <div className="w-full h-2 bg-white/10 rounded-full overflow-hidden mb-3">
                    <div
                      className={`h-full rounded-full bg-gradient-to-r ${getGradient(
                        index
                      )} transition-all duration-700 ease-out`}
                      style={{ width: `${Math.min(percentage, 100)}%` }}
                    />
                  </div>

                  <div className="flex items-center gap-2">
                    <span className="text-[11px] px-2 py-0.5 rounded-full bg-white/10 text-white/60">
                      {chain.dexCount ?? dexes.length} DEXes
                    </span>
                    <span className="text-[11px] px-2 py-0.5 rounded-full bg-white/10 text-white/60">
                      {chain.tokenCount ?? 0} Tokens
                    </span>
                    <span className="text-[11px] text-white/40 ml-auto">
                      {percentage < 0.1 ? '<0.1' : percentage.toFixed(1)}%
                    </span>
                  </div>

                  {dexes.length > 0 && (
                    <div className="mt-4 pt-3 border-t border-white/5 space-y-2">
                      {displayDexes.map((dex) => {
                        const dexPct =
                          chain.totalLiquidity > 0
                            ? (dex.liquidity / chain.totalLiquidity) * 100
                            : 0;
                        return (
                          <div
                            key={dex.name}
                            className="flex items-center gap-3"
                          >
                            <span className="text-xs text-white/70 w-20 truncate shrink-0">
                              {dex.name}
                            </span>
                            <div className="flex-1 h-1.5 bg-white/10 rounded-full overflow-hidden">
                              <div
                                className="h-full rounded-full bg-white/30"
                                style={{
                                  width: `${Math.min(dexPct, 100)}%`,
                                }}
                              />
                            </div>
                            <span className="text-xs text-white/50 w-16 text-right shrink-0">
                              {compactUsd(dex.liquidity)}
                            </span>
                          </div>
                        );
                      })}
                      {hasMore && (
                        <span className="text-xs text-indigo-400 hover:text-indigo-300 transition-colors block text-center pt-1">
                          {rtl
                            ? `عرض ${dexes.length - 5} المزيد`
                            : `View ${dexes.length - 5} more`}
                        </span>
                      )}
                    </div>
                  )}
                </div>
              </button>
            );
          })}
        </div>

        <div className="hidden lg:block">
          <div className="bg-white/[0.03] rounded-xl p-5 border border-white/5 sticky top-6">
            <h3 className="text-sm font-medium text-white/70 mb-4">
              {rtl ? 'ملخص الشبكات' : 'Chain Summary'}
            </h3>
            <div className="space-y-3">
              {sortedChains.map((chain, index) => {
                const percentage =
                  totalLiquidity > 0
                    ? (chain.totalLiquidity / totalLiquidity) * 100
                    : 0;
                const icon =
                  CHAIN_ICONS[chain.chainName] ??
                  chain.chainName.slice(0, 3).toUpperCase();
                const { dexes } = getDexList(chain);
                return (
                  <div
                    key={chain.chainName}
                    className="flex items-center gap-3"
                  >
                    <span className="w-6 h-6 rounded-full bg-white/10 flex items-center justify-center text-[9px] font-bold text-white/60 shrink-0">
                      {icon}
                    </span>
                    <div className="flex-1 min-w-0">
                      <div className="flex items-center justify-between mb-1">
                        <span className="text-xs text-white/70 truncate">
                          {chain.chainName}
                        </span>
                        <span className="text-xs text-white/50">
                          {percentage.toFixed(1)}%
                        </span>
                      </div>
                      <div className="w-full h-1 bg-white/10 rounded-full overflow-hidden">
                        <div
                          className={`h-full rounded-full bg-gradient-to-r ${getGradient(
                            index
                          )}`}
                          style={{ width: `${Math.min(percentage, 100)}%` }}
                        />
                      </div>
                    </div>
                    <span className="text-xs text-white/60 w-14 text-right shrink-0">
                      {compactUsd(chain.totalLiquidity)}
                    </span>
                  </div>
                );
              })}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
