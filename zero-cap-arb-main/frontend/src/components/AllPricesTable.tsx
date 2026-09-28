'use client';

import { useState, useMemo } from 'react';
import type { TokenPrice, ChainSummary } from '@/lib/api';

interface Props {
  prices: TokenPrice[];
  summaries: ChainSummary[];
  token: string;
  scanTimeMs: number;
  onExecuteBest?: (buy: TokenPrice, sell: TokenPrice) => void;
}

const chainColors: Record<string, string> = {
  Ethereum: '#6366f1', Arbitrum: '#2563eb', Optimism: '#dc2626',
  Polygon: '#7c3aed', BSC: '#eab308', Avalanche: '#ef4444',
};

export function AllPricesTable({ prices, summaries, token, scanTimeMs, onExecuteBest }: Props) {
  const [sortKey, setSortKey] = useState<keyof TokenPrice>('price_usd');
  const [sortDir, setSortDir] = useState<'asc' | 'desc'>('asc');
  const [chainFilter, setChainFilter] = useState<number | 'all'>('all');
  const [searchDex, setSearchDex] = useState('');

  const sorted = useMemo(() => {
    let filtered = prices;
    if (chainFilter !== 'all') filtered = filtered.filter((p) => p.chain_id === chainFilter);
    if (searchDex) filtered = filtered.filter((p) => p.dex_name.toLowerCase().includes(searchDex.toLowerCase()));

    return [...filtered].sort((a, b) => {
      const aVal = a[sortKey] as number;
      const bVal = b[sortKey] as number;
      return sortDir === 'asc' ? aVal - bVal : bVal - aVal;
    });
  }, [prices, sortKey, sortDir, chainFilter, searchDex]);

  const lowest = useMemo(() => prices.reduce((min, p) => p.price_usd < min.price_usd ? p : min, prices[0]), [prices]);
  const highest = useMemo(() => prices.reduce((max, p) => p.price_usd > max.price_usd ? p : max, prices[0]), [prices]);

  const toggleSort = (key: keyof TokenPrice) => {
    if (sortKey === key) setSortDir((d) => (d === 'asc' ? 'desc' : 'asc'));
    else { setSortKey(key); setSortDir('asc'); }
  };

  const SortIcon = ({ colKey }: { colKey: keyof TokenPrice }) => {
    if (sortKey !== colKey) return <span className="text-gray-600 ml-1">&#8597;</span>;
    return <span className="text-indigo-400 ml-1">{sortDir === 'asc' ? '&#8593;' : '&#8595;'}</span>;
  };

  if (prices.length === 0) return null;

  return (
    <div className="space-y-4">
      {/* Summary Cards */}
      <div className="grid grid-cols-2 md:grid-cols-4 gap-3">
        <div className="glass rounded-xl p-4">
          <p className="text-xs text-gray-500 uppercase tracking-wider">Total DEXes</p>
          <p className="text-2xl font-bold text-white mt-1">{prices.length}</p>
        </div>
        <div className="glass rounded-xl p-4 border border-green-500/20">
          <p className="text-xs text-gray-500 uppercase tracking-wider">Lowest Price</p>
          <p className="text-2xl font-bold text-green-400 mt-1">${lowest.price_usd.toFixed(6)}</p>
          <p className="text-xs text-gray-400 mt-0.5">{lowest.dex_name} &middot; {lowest.chain_name}</p>
        </div>
        <div className="glass rounded-xl p-4 border border-red-500/20">
          <p className="text-xs text-gray-500 uppercase tracking-wider">Highest Price</p>
          <p className="text-2xl font-bold text-red-400 mt-1">${highest.price_usd.toFixed(6)}</p>
          <p className="text-xs text-gray-400 mt-0.5">{highest.dex_name} &middot; {highest.chain_name}</p>
        </div>
        <div className="glass rounded-xl p-4 border border-indigo-500/20">
          <p className="text-xs text-gray-500 uppercase tracking-wider">Max Spread</p>
          <p className="text-2xl font-bold text-indigo-400 mt-1">
            {lowest.price_usd > 0 ? (((highest.price_usd - lowest.price_usd) / lowest.price_usd) * 100).toFixed(2) : '0.00'}%
          </p>
          <p className="text-xs text-gray-400 mt-0.5">Scan: {scanTimeMs}ms</p>
        </div>
      </div>

      {/* Chain Summary */}
      <div className="grid grid-cols-2 md:grid-cols-3 lg:grid-cols-6 gap-2">
        {summaries.map((s) => {
          const color = chainColors[s.chain_name] ?? '#6366f1';
          return (
            <div key={s.chain_id} className="glass rounded-lg p-3 text-center">
              <div className="flex items-center justify-center gap-1.5 mb-1">
                <div className="w-2 h-2 rounded-full" style={{ backgroundColor: color }} />
                <span className="text-xs font-medium text-gray-300">{s.chain_name}</span>
              </div>
              <p className="text-lg font-bold text-white">{s.dex_count}</p>
              <p className="text-[10px] text-gray-500">DEXes</p>
              <div className="flex justify-between text-[10px] text-gray-500 mt-1">
                <span>${s.min_price.toFixed(4)}</span>
                <span className={s.spread_pct > 1 ? 'text-green-400' : ''}>{s.spread_pct.toFixed(2)}%</span>
                <span>${s.max_price.toFixed(4)}</span>
              </div>
            </div>
          );
        })}
      </div>

      {/* Filters */}
      <div className="flex items-center gap-3 flex-wrap">
        <div className="flex items-center gap-2">
          <span className="text-xs text-gray-500">Chain:</span>
          <select
            value={chainFilter}
            onChange={(e) => setChainFilter(e.target.value === 'all' ? 'all' : Number(e.target.value))}
            className="bg-white/5 border border-white/10 rounded-lg px-2 py-1 text-xs text-white"
          >
            <option value="all">All Chains</option>
            {summaries.map((s) => (
              <option key={s.chain_id} value={s.chain_id}>{s.chain_name} ({s.dex_count})</option>
            ))}
          </select>
        </div>
        <input
          type="text"
          value={searchDex}
          onChange={(e) => setSearchDex(e.target.value)}
          placeholder="Search DEX..."
          className="bg-white/5 border border-white/10 rounded-lg px-3 py-1 text-xs text-white placeholder-gray-500 w-36"
        />
        <span className="text-xs text-gray-500">{sorted.length} of {prices.length} prices</span>
      </div>

      {/* Price Table */}
      <div className="glass-card overflow-hidden">
        <div className="overflow-x-auto max-h-[420px] overflow-y-auto">
          <table className="w-full text-xs">
            <thead className="sticky top-0 bg-surface-light z-10">
              <tr className="border-b border-white/5 text-gray-500 uppercase tracking-wider">
                <th className="text-left px-3 py-2 font-medium cursor-pointer hover:text-white" onClick={() => toggleSort('dex_name')}>
                  DEX <SortIcon colKey="dex_name" />
                </th>
                <th className="text-left px-3 py-2 font-medium cursor-pointer hover:text-white" onClick={() => toggleSort('chain_name')}>
                  Chain <SortIcon colKey="chain_name" />
                </th>
                <th className="text-right px-3 py-2 font-medium cursor-pointer hover:text-white" onClick={() => toggleSort('price_usd')}>
                  Price (USD) <SortIcon colKey="price_usd" />
                </th>
                <th className="text-right px-3 py-2 font-medium cursor-pointer hover:text-white" onClick={() => toggleSort('liquidity_usd')}>
                  Liquidity <SortIcon colKey="liquidity_usd" />
                </th>
                <th className="text-right px-3 py-2 font-medium">Spread vs Avg</th>
                <th className="text-center px-3 py-2 font-medium">Action</th>
              </tr>
            </thead>
            <tbody>
              {sorted.map((price, i) => {
                const avg = summaries.find((s) => s.chain_id === price.chain_id)?.avg_price ?? price.price_usd;
                const spreadVsAvg = avg > 0 ? ((price.price_usd - avg) / avg) * 100 : 0;
                const isLowest = price.dex_name === lowest.dex_name && price.chain_id === lowest.chain_id;
                const isHighest = price.dex_name === highest.dex_name && price.chain_id === highest.chain_id;

                return (
                  <tr
                    key={`${price.chain_id}-${price.dex_name}-${i}`}
                    className={`border-b border-white/5 hover:bg-white/5 transition-colors ${
                      isLowest ? 'bg-green-500/5' : isHighest ? 'bg-red-500/5' : ''
                    }`}
                  >
                    <td className="px-3 py-2.5">
                      <div className="flex items-center gap-2">
                        {isLowest && <span className="w-1.5 h-1.5 rounded-full bg-green-500" />}
                        {isHighest && <span className="w-1.5 h-1.5 rounded-full bg-red-500" />}
                        <span className="font-medium text-white">{price.dex_name}</span>
                      </div>
                    </td>
                    <td className="px-3 py-2.5">
                      <span
                        className="px-1.5 py-0.5 rounded text-[10px] font-medium"
                        style={{
                          backgroundColor: `${chainColors[price.chain_name] ?? '#6366f1'}20`,
                          color: chainColors[price.chain_name] ?? '#6366f1',
                        }}
                      >
                        {price.chain_name}
                      </span>
                    </td>
                    <td className="px-3 py-2.5 text-right font-mono">
                      <span className={isLowest ? 'text-green-400 font-semibold' : isHighest ? 'text-red-400 font-semibold' : 'text-gray-300'}>
                        ${price.price_usd.toFixed(6)}
                      </span>
                    </td>
                    <td className="px-3 py-2.5 text-right font-mono text-gray-400">
                      ${(price.liquidity_usd / 1000).toFixed(0)}K
                    </td>
                    <td className="px-3 py-2.5 text-right">
                      <span className={`font-mono ${spreadVsAvg > 0.5 ? 'text-green-400' : spreadVsAvg < -0.5 ? 'text-red-400' : 'text-gray-500'}`}>
                        {spreadVsAvg > 0 ? '+' : ''}{spreadVsAvg.toFixed(2)}%
                      </span>
                    </td>
                    <td className="px-3 py-2.5 text-center">
                      <button
                        onClick={() => onExecuteBest?.(price, highest)}
                        className="px-2 py-1 text-[10px] font-medium bg-indigo-600/20 text-indigo-400 
                                   hover:bg-indigo-600/30 rounded-lg transition-colors"
                      >
                        Trade
                      </button>
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
