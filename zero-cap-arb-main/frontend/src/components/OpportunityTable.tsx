'use client';

import type { ArbitrageOpportunity } from '@/lib/api';

const chainColors: Record<string, string> = {
  Ethereum: 'bg-blue-500/20 text-blue-400',
  Arbitrum: 'bg-blue-600/20 text-blue-400',
  Optimism: 'bg-red-500/20 text-red-400',
  Polygon: 'bg-purple-500/20 text-purple-400',
  BSC: 'bg-yellow-500/20 text-yellow-400',
  Avalanche: 'bg-red-400/20 text-red-400',
};

function ChainTag({ name }: { name: string }) {
  return (
    <span
      className={`px-2 py-0.5 rounded text-xs font-medium ${
        chainColors[name] ?? 'bg-gray-500/20 text-gray-400'
      }`}
    >
      {name}
    </span>
  );
}

interface Props {
  opportunities: ArbitrageOpportunity[];
  onSelect: (opp: ArbitrageOpportunity) => void;
  selectedId: string | null;
}

export function OpportunityTable({ opportunities, onSelect, selectedId }: Props) {
  if (opportunities.length === 0) return null;

  return (
    <div className="glass-card overflow-hidden">
      <div className="p-4 border-b border-white/5">
        <h3 className="text-sm font-semibold text-gray-300">
          Arbitrage Opportunities
        </h3>
      </div>

      <div className="overflow-x-auto">
        <table className="w-full text-sm">
          <thead>
            <tr className="border-b border-white/5 text-xs text-gray-500 uppercase tracking-wider">
              <th className="text-left px-4 py-3 font-medium">Token</th>
              <th className="text-left px-4 py-3 font-medium">Buy</th>
              <th className="text-left px-4 py-3 font-medium">Sell</th>
              <th className="text-right px-4 py-3 font-medium">Buy Price</th>
              <th className="text-right px-4 py-3 font-medium">Sell Price</th>
              <th className="text-right px-4 py-3 font-medium">Spread</th>
              <th className="text-right px-4 py-3 font-medium">Est. Profit</th>
              <th className="text-right px-4 py-3 font-medium">Liq.</th>
              <th className="text-center px-4 py-3 font-medium">Action</th>
            </tr>
          </thead>
          <tbody>
            {opportunities.map((opp) => (
              <tr
                key={opp.id}
                className={`border-b border-white/5 hover:bg-white/5 transition-colors ${
                  selectedId === opp.id ? 'bg-indigo-500/10' : ''
                }`}
              >
                <td className="px-4 py-3">
                  <span className="font-mono font-semibold text-white">
                    {opp.token_symbol}
                  </span>
                </td>
                <td className="px-4 py-3">
                  <div className="flex items-center gap-2">
                    <ChainTag name={opp.buy_chain_name} />
                    <span className="text-xs text-gray-400">{opp.buy_dex}</span>
                  </div>
                </td>
                <td className="px-4 py-3">
                  <div className="flex items-center gap-2">
                    <ChainTag name={opp.sell_chain_name} />
                    <span className="text-xs text-gray-400">{opp.sell_dex}</span>
                  </div>
                </td>
                <td className="px-4 py-3 text-right font-mono text-gray-300">
                  ${opp.buy_price_usd.toFixed(6)}
                </td>
                <td className="px-4 py-3 text-right font-mono text-gray-300">
                  ${opp.sell_price_usd.toFixed(6)}
                </td>
                <td className="px-4 py-3 text-right">
                  <span
                    className={`font-mono font-semibold ${
                      opp.spread_pct > 2
                        ? 'text-green-400'
                        : opp.spread_pct > 1
                        ? 'text-yellow-400'
                        : 'text-gray-300'
                    }`}
                  >
                    {opp.spread_pct.toFixed(2)}%
                  </span>
                </td>
                <td className="px-4 py-3 text-right font-mono text-green-400 font-semibold">
                  ${opp.estimated_profit_usd.toFixed(2)}
                </td>
                <td className="px-4 py-3 text-right font-mono text-gray-400 text-xs">
                  ${(opp.liquidity_usd / 1000).toFixed(0)}K
                </td>
                <td className="px-4 py-3 text-center">
                  <button
                    onClick={() => onSelect(opp)}
                    className="px-3 py-1.5 text-xs font-medium bg-indigo-600/20 
                               text-indigo-400 hover:bg-indigo-600/30 
                               rounded-lg transition-colors"
                  >
                    Execute
                  </button>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </div>
  );
}
