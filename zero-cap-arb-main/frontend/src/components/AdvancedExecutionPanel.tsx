'use client';

import { useState, useCallback, useEffect } from 'react';
import { useAccount } from 'wagmi';
import { api, type ExecuteResult, type AllOpportunitiesResponse } from '@/lib/api';

// ─── Strategy Definitions ────────────────────────────

interface StrategyDef {
  id: string;
  label: string;
  icon: string;
  mode: 'FlashLoan' | 'DirectSwap' | 'Mint';
  color: string;
  gradient: string;
  needsWallet: boolean;
  needsCapital: boolean;
  description: string;
  detailedDesc: string;
}

const STRATEGIES: StrategyDef[] = [
  {
    id: 'flash_loan',
    label: 'Flash Loan Arbitrage',
    icon: 'Z',
    mode: 'FlashLoan',
    color: 'from-indigo-600 to-purple-600',
    gradient: 'from-indigo-500/20 to-purple-500/20',
    needsWallet: true,
    needsCapital: false,
    description: '0 capital required',
    detailedDesc: 'Borrow via flash loan from Aave/Spark/Radiant. Buy on cheap DEX, sell on expensive DEX via ParaSwap. Repay loan + keep profit.',
  },
  {
    id: 'direct_swap',
    label: 'Direct Swap',
    icon: 'S',
    mode: 'DirectSwap',
    color: 'from-blue-600 to-cyan-600',
    gradient: 'from-blue-500/20 to-cyan-500/20',
    needsWallet: true,
    needsCapital: true,
    description: 'Your capital, best route',
    detailedDesc: 'User provides capital. We find the single best route via ParaSwap split routing across 50+ DEXes. Minimal slippage.',
  },
  {
    id: 'mint',
    label: 'Mint Arbitrage',
    icon: 'M',
    mode: 'Mint',
    color: 'from-green-600 to-emerald-600',
    gradient: 'from-green-500/20 to-emerald-500/20',
    needsWallet: true,
    needsCapital: false,
    description: '0 capital, mint & sell',
    detailedDesc: 'Mint DAI on Spark/Maker Protocol when mint cost is below market price. Sell on DEX for instant profit. 0 capital via flash loan.',
  },
  {
    id: 'triangular',
    label: 'Triangular Arbitrage',
    icon: 'T',
    mode: 'FlashLoan',
    color: 'from-orange-600 to-red-600',
    gradient: 'from-orange-500/20 to-red-500/20',
    needsWallet: true,
    needsCapital: false,
    description: '3-token cycle, 0 capital',
    detailedDesc: 'Cycle through 3 tokens (e.g. DAI→ETH→USDC→DAI) on a single chain where price inconsistencies create profit. Flash loan funded.',
  },
  {
    id: 'cross_chain',
    label: 'Cross-Chain Arbitrage',
    icon: 'C',
    mode: 'FlashLoan',
    color: 'from-pink-600 to-rose-600',
    gradient: 'from-pink-500/20 to-rose-500/20',
    needsWallet: true,
    needsCapital: false,
    description: 'Bridge & profit, 0 capital',
    detailedDesc: 'Bridge tokens across chains via Stargate/Across. Buy on chain A where cheap, bridge to chain B where expensive. Flash loan funded.',
  },
  {
    id: 'jit',
    label: 'JIT Liquidity / MEV',
    icon: 'J',
    mode: 'DirectSwap',
    color: 'from-yellow-600 to-amber-600',
    gradient: 'from-yellow-500/20 to-amber-500/20',
    needsWallet: true,
    needsCapital: true,
    description: 'Provide LP, earn fees',
    detailedDesc: 'Monitor mempool for large pending swaps. Provide just-in-time liquidity to capture the spread as LP fees. Capital required.',
  },
];

// ─── Flash Loan Sources ────────────────────────────────

const FLASH_SOURCES = [
  { value: 'Spark', label: 'Spark Protocol', fee: '0% on DAI / 0.05% others', priority: true },
  { value: 'AaveV3', label: 'Aave V3', fee: '0.05%', priority: false },
  { value: 'RadiantV2', label: 'Radiant V2', fee: '0.03%', priority: false },
];

const GAS_STRATEGIES = [
  { value: 'Flashbots', label: 'Flashbots / MEV-Share', desc: 'Private mempool. $0 if revert.' },
  { value: 'Pimlico', label: 'Pimlico (ERC-4337)', desc: 'Pay gas in USDC.' },
  { value: 'ZeroDev', label: 'ZeroDev (ERC-4337)', desc: 'Pay gas in profit tokens.' },
];

// ─── Component ────────────────────────────────────────

interface Props {
  opportunities: AllOpportunitiesResponse | null;
  token: string;
  onClose: () => void;
}

export function AdvancedExecutionPanel({ opportunities, token, onClose }: Props) {
  const { address, isConnected } = useAccount();
  const [selectedStrat, setSelectedStrat] = useState<StrategyDef>(STRATEGIES[0]);
  const [flashSource, setFlashSource] = useState('Spark');
  const [gasStrategy, setGasStrategy] = useState('Flashbots');
  const [amount, setAmount] = useState('1000');
  const [executing, setExecuting] = useState(false);
  const [result, setResult] = useState<ExecuteResult | null>(null);
  const [showAllStrats, setShowAllStrats] = useState(false);

  const handleExecute = useCallback(async () => {
    if (!isConnected || !address) return;
    setExecuting(true);
    setResult(null);

    try {
      const res = await api.execute({
        strategy: selectedStrat.id,
        execution_mode: selectedStrat.mode,
        flash_loan_source: selectedStrat.mode === 'FlashLoan' ? flashSource : undefined,
        gas_strategy: gasStrategy,
        user_address: address,
        token,
        amount: selectedStrat.needsCapital ? amount : undefined,
      });
      setResult(res);

      const addTx = (window as Record<string, unknown>).__addTx as ((tx: Record<string, unknown>) => void) | undefined;
      addTx?.({
        token,
        strategy: selectedStrat.label,
        mode: selectedStrat.mode,
        profit: res.estimated_profit_usd,
        gasStrategy,
        hash: res.tx_hash,
        status: 'success',
      });
    } catch (err) {
      const addTx = (window as Record<string, unknown>).__addTx as ((tx: Record<string, unknown>) => void) | undefined;
      addTx?.({
        token,
        strategy: selectedStrat.label,
        mode: selectedStrat.mode,
        profit: 0,
        gasStrategy,
        status: 'failed',
        error: err instanceof Error ? err.message : 'Failed',
      });
    } finally {
      setExecuting(false);
    }
  }, [selectedStrat, flashSource, gasStrategy, amount, address, isConnected, token]);

  return (
    <div className="glass-card overflow-hidden">
      {/* Header */}
      <div className="p-4 border-b border-white/5 flex items-center justify-between">
        <div className="flex items-center gap-3">
          <div className={`w-8 h-8 rounded-lg bg-gradient-to-br ${selectedStrat.color} flex items-center justify-center text-sm font-bold text-white`}>
            {selectedStrat.icon}
          </div>
          <div>
            <h3 className="text-sm font-semibold text-white">Advanced Execution Engine</h3>
            <p className="text-[10px] text-gray-500">{token || 'No token selected'}</p>
          </div>
        </div>
        <button onClick={onClose} className="text-gray-500 hover:text-white transition-colors">
          <svg className="w-4 h-4" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
          </svg>
        </button>
      </div>

      <div className="p-4 space-y-5">
        {/* Strategy Selector */}
        <div>
          <div className="flex items-center justify-between mb-3">
            <label className="text-xs font-semibold text-gray-400 uppercase tracking-wider">Strategy</label>
            <button onClick={() => setShowAllStrats(!showAllStrats)} className="text-[10px] text-indigo-400 hover:text-indigo-300">
              {showAllStrats ? 'Show less' : 'Show all 6 strategies'}
            </button>
          </div>

          {/* Featured: 3 main modes */}
          <div className="grid grid-cols-3 gap-2 mb-2">
            {STRATEGIES.slice(0, 3).map((s) => (
              <StrategyCard key={s.id} strat={s} selected={selectedStrat.id === s.id} onSelect={() => setSelectedStrat(s)} />
            ))}
          </div>

          {/* Expanded: all 6 */}
          {showAllStrats && (
            <div className="grid grid-cols-3 gap-2 animate-[fadeIn_0.2s_ease]">
              {STRATEGIES.slice(3).map((s) => (
                <StrategyCard key={s.id} strat={s} selected={selectedStrat.id === s.id} onSelect={() => setSelectedStrat(s)} />
              ))}
            </div>
          )}
        </div>

        {/* Strategy Detail */}
        <div className="glass rounded-xl p-3">
          <div className="flex items-start gap-3">
            <div className={`w-1.5 h-full min-h-[3rem] rounded-full bg-gradient-to-b ${selectedStrat.gradient}`} />
            <div>
              <div className="flex items-center gap-2 mb-1">
                <span className={`text-xs font-semibold ${selectedStrat.needsCapital ? 'text-yellow-400' : 'text-green-400'}`}>
                  {selectedStrat.needsCapital ? 'Requires Capital' : '0 Capital Required'}
                </span>
                <span className="text-gray-600">&bull;</span>
                <span className="text-xs text-gray-400">{selectedStrat.mode}</span>
              </div>
              <p className="text-xs text-gray-300 leading-relaxed">{selectedStrat.detailedDesc}</p>
            </div>
          </div>
        </div>

        {/* Conditional: Flash Loan Source */}
        {selectedStrat.mode === 'FlashLoan' && (
          <div>
            <label className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-2 block">Flash Loan Source</label>
            <div className="grid grid-cols-3 gap-2">
              {FLASH_SOURCES.map((fs) => (
                <button
                  key={fs.value}
                  onClick={() => setFlashSource(fs.value)}
                  className={`p-2.5 rounded-xl text-left transition-all duration-200 border ${
                    flashSource === fs.value
                      ? 'bg-indigo-600/20 border-indigo-500/50'
                      : 'bg-white/5 border-white/10 hover:bg-white/10'
                  }`}
                >
                  <p className="text-xs font-medium text-white">{fs.label}</p>
                  <p className="text-[10px] text-gray-400 mt-0.5">{fs.fee}</p>
                  {fs.priority && <span className="text-[10px] text-green-400 font-medium">Priority (0% fee)</span>}
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Conditional: Amount input for capital-needed strats */}
        {selectedStrat.needsCapital && (
          <div>
            <label className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-2 block">Capital Amount (USD)</label>
            <input
              type="number"
              value={amount}
              onChange={(e) => setAmount(e.target.value)}
              className="w-full bg-white/5 border border-white/10 rounded-xl px-4 py-2.5 text-sm text-white"
              placeholder="Enter amount in USD"
            />
          </div>
        )}

        {/* Gas Strategy */}
        <div>
          <label className="text-xs font-semibold text-gray-400 uppercase tracking-wider mb-2 block">Gas Strategy</label>
          <div className="grid grid-cols-3 gap-2">
            {GAS_STRATEGIES.map((g) => (
              <button
                key={g.value}
                onClick={() => setGasStrategy(g.value)}
                className={`p-2.5 rounded-xl text-left transition-all duration-200 border ${
                  gasStrategy === g.value
                    ? 'bg-purple-600/20 border-purple-500/50'
                    : 'bg-white/5 border-white/10 hover:bg-white/10'
                }`}
              >
                <p className="text-xs font-medium text-white">{g.label}</p>
                <p className="text-[10px] text-gray-400 mt-0.5">{g.desc}</p>
              </button>
            ))}
          </div>
        </div>

        {/* Opportunities Found */}
        {opportunities && (
          <div className="glass rounded-xl p-3">
            <p className="text-xs text-gray-400 mb-2">Opportunities Found for {token}</p>
            <div className="grid grid-cols-5 gap-2 text-center">
              <OppCount label="Simple Arb" count={opportunities.simple_arbitrages.length} color="text-indigo-400" />
              <OppCount label="Triangular" count={opportunities.triangular_arbitrages.length} color="text-orange-400" />
              <OppCount label="Cross-Chain" count={opportunities.cross_chain_arbitrages.length} color="text-pink-400" />
              <OppCount label="Mint" count={opportunities.mint_opportunities.length} color="text-green-400" />
              <OppCount label="JIT Liq" count={opportunities.jit_opportunities.length} color="text-yellow-400" />
            </div>
          </div>
        )}

        {/* Execute Button */}
        <button
          onClick={handleExecute}
          disabled={executing || !isConnected}
          className={`w-full py-3 rounded-xl font-semibold text-sm transition-all duration-200
            bg-gradient-to-r ${selectedStrat.color} 
            disabled:opacity-40 disabled:cursor-not-allowed neo-glow
            flex items-center justify-center gap-2`}
        >
          {executing ? (
            <>
              <svg className="animate-spin h-4 w-4" viewBox="0 0 24 24">
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" />
                <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" />
              </svg>
              Executing {selectedStrat.label}...
            </>
          ) : !isConnected ? (
            'Connect Wallet First'
          ) : (
            <>
              Execute {selectedStrat.label}
              {selectedStrat.needsCapital ? ` (${amount} USDC)` : ' (0 Capital)'}
            </>
          )}
        </button>

        {/* Result */}
        {result && (
          <div className="glass rounded-xl p-3 space-y-2 border border-indigo-500/20">
            <div className="flex items-center gap-2">
              <svg className="w-4 h-4 text-green-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
              </svg>
              <span className="text-sm text-gray-300">{result.message}</span>
            </div>
            <div className="grid grid-cols-3 gap-2 text-xs">
              <div className="bg-white/5 rounded-lg p-2 text-center">
                <p className="text-gray-500">Profit</p>
                <p className="text-green-400 font-semibold">${result.estimated_profit_usd?.toFixed(2)}</p>
              </div>
              <div className="bg-white/5 rounded-lg p-2 text-center">
                <p className="text-gray-500">Gas Cost</p>
                <p className="text-gray-300 font-semibold">${result.gas_cost_usd?.toFixed(2)}</p>
              </div>
              <div className="bg-white/5 rounded-lg p-2 text-center">
                <p className="text-gray-500">Tx Hash</p>
                <p className="text-indigo-400 font-mono text-[10px] truncate">{result.tx_hash?.slice(0, 10)}...</p>
              </div>
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

// ─── Sub-components ───────────────────────────────────

function StrategyCard({ strat, selected, onSelect }: { strat: StrategyDef; selected: boolean; onSelect: () => void }) {
  return (
    <button
      onClick={onSelect}
      className={`p-3 rounded-xl text-left transition-all duration-200 border ${
        selected
          ? 'border-transparent bg-gradient-to-br ' + strat.gradient + ' shadow-lg'
          : 'bg-white/5 border-white/10 hover:bg-white/10'
      }`}
    >
      <div className={`w-6 h-6 rounded-lg bg-gradient-to-br ${strat.color} flex items-center justify-center text-[10px] font-bold text-white mb-1.5`}>
        {strat.icon}
      </div>
      <p className="text-xs font-semibold text-white leading-tight">{strat.label}</p>
      <p className="text-[10px] text-gray-400 mt-0.5">{strat.description}</p>
    </button>
  );
}

function OppCount({ label, count, color }: { label: string; count: number; color: string }) {
  return (
    <div className="bg-white/5 rounded-lg p-2">
      <p className={`text-sm font-bold ${color}`}>{count}</p>
      <p className="text-[10px] text-gray-500">{label}</p>
    </div>
  );
}
