'use client';

import { useMemo, useState, useCallback } from 'react';
import { OpportunityDetail, api, ExecuteResult } from '@/lib/api';

type Props = {
  opportunity: OpportunityDetail;
  onExecute?: (id: string) => void;
  rtl?: boolean;
  isSelected?: boolean;
  onSelect?: (id: string) => void;
  botMode?: boolean;
};

const usd = (n: number) =>
  new Intl.NumberFormat('en-US', { style: 'currency', currency: 'USD' }).format(n);

const pct = (n: number) =>
  new Intl.NumberFormat('en-US', { style: 'percent', minimumFractionDigits: 2, maximumFractionDigits: 2 }).format(n / 100);

const relTime = (iso: string) => {
  const diff = Date.now() - new Date(iso).getTime();
  const s = Math.floor(diff / 1000);
  if (s < 60) return `${s}s ago`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  return `${h}h ago`;
};

const typeColors: Record<string, string> = {
  Simple: 'bg-blue-500/20 text-blue-300 border-blue-500/40',
  Triangular: 'bg-purple-500/20 text-purple-300 border-purple-500/40',
  CrossChain: 'bg-orange-500/20 text-orange-300 border-orange-500/40',
  Mint: 'bg-green-500/20 text-green-300 border-green-500/40',
  Jit: 'bg-red-500/20 text-red-300 border-red-500/40',
};

const ArrowUp = () => <span className="text-green-400">&#9650;</span>;
const ArrowDown = () => <span className="text-red-400">&#9660;</span>;

const OpportunityCard = ({
  opportunity: o,
  onExecute,
  rtl = false,
  isSelected = false,
  onSelect,
  botMode = false,
}: Props) => {
  const [showCosts, setShowCosts] = useState(false);

  const handleClick = useCallback(() => {
    onSelect?.(o.id);
  }, [onSelect, o.id]);

  const handleExecute = useCallback(() => {
    onExecute?.(o.id);
  }, [onExecute, o.id]);

  const {
    confidenceColor,
    profitableTag,
    spreadDir,
    netProfitColor,
    netProfitPct,
    roiPct,
    totalCost,
    spreadPct,
  } = useMemo(() => {
    const conf = o.confidenceScore ?? 0;
    const cc =
      conf >= 80
        ? 'text-green-400'
        : conf >= 50
          ? 'text-yellow-400'
          : 'text-red-400';
    const pt = o.isProfitable
      ? 'bg-green-500/20 text-green-300 border border-green-500/40'
      : 'bg-red-500/20 text-red-300 border border-red-500/40';
    const sd = o.isProfitable ? <ArrowUp /> : <ArrowDown />;
    const np = o.netProfit ?? 0;
    const npc = np >= 0 ? 'text-green-400' : 'text-red-400';
    const g = o.grossProfit ?? 0;
    const tc = o.gasCost + o.flashLoanFee + o.slippage + o.bridgeFee + o.veloraFee;
    const npp = g > 0 ? (np / g) * 100 : 0;
    const rp = tc > 0 ? (np / tc) * 100 : 0;
    const sp = o.spreadPercent ?? 0;
    return {
      confidenceColor: cc,
      profitableTag: pt,
      spreadDir: sd,
      netProfitColor: npc,
      netProfitPct: npp,
      roiPct: rp,
      totalCost: tc,
      spreadPct: sp,
    };
  }, [o]);

  const flashLoanPrimary = useMemo(() => {
    if (!o.flashLoan || o.flashLoan.length === 0) return null;
    return o.flashLoan[0];
  }, [o.flashLoan]);

  const flashLoanAlternatives = useMemo(() => {
    if (!o.flashLoan || o.flashLoan.length <= 1) return [];
    return o.flashLoan.slice(1);
  }, [o.flashLoan]);

  const steps = useMemo(() => {
    if (!o.executionSteps || o.executionSteps.length === 0) return [];
    return o.executionSteps;
  }, [o.executionSteps]);

  return (
    <div
      dir={rtl ? 'rtl' : 'ltr'}
      onClick={handleClick}
      className={`
        bg-white/5 backdrop-blur-xl border rounded-2xl p-5 cursor-pointer
        transition-all duration-300 hover:bg-white/10
        ${isSelected ? 'border-green-400 shadow-[0_0_20px_-5px_rgba(74,222,128,0.5)]' : 'border-white/10'}
      `}
    >
      {/* Header Row */}
      <div className="flex items-center justify-between mb-4">
        <div className="flex items-center gap-2">
          <span className={`text-xs font-semibold px-2.5 py-1 rounded-full border ${typeColors[o.arbType] ?? 'bg-gray-500/20 text-gray-300 border-gray-500/40'}`}>
            {o.arbType}
          </span>
          <span className="text-white font-bold">{o.tokenSymbol}</span>
          <span className="text-gray-400 text-sm truncate max-w-[120px]">{o.tokenName}</span>
        </div>
        <div className="flex items-center gap-2">
          <span className={`text-sm font-semibold ${confidenceColor}`}>
            {o.confidenceScore ?? 0}%
          </span>
          <span className={`text-xs font-medium px-2 py-0.5 rounded ${profitableTag}`}>
            {o.isProfitable ? 'Profitable' : 'Not Profitable'}
          </span>
        </div>
      </div>

      {/* Price Spread Section */}
      <div className="grid grid-cols-3 gap-3 mb-4 p-3 bg-white/5 rounded-xl">
        <div className="text-left">
          <div className="text-xs text-gray-400 mb-1">{o.buyDex} &middot; {o.buyChain}</div>
          <div className="flex items-center gap-1">
            <ArrowDown />
            <span className="text-white font-mono text-sm">{usd(o.buyPrice)}</span>
          </div>
        </div>
        <div className="text-center">
          <div className="text-xs text-gray-400 mb-1">Spread</div>
          <div className="flex items-center justify-center gap-1">
            <span className="text-white font-bold text-lg">{spreadPct.toFixed(2)}%</span>
            {spreadDir}
          </div>
        </div>
        <div className="text-right">
          <div className="text-xs text-gray-400 mb-1">{o.sellDex} &middot; {o.sellChain}</div>
          <div className="flex items-center justify-end gap-1">
            <ArrowUp />
            <span className="text-white font-mono text-sm">{usd(o.sellPrice)}</span>
          </div>
        </div>
      </div>

      {/* Spread Progress Bar */}
      <div className="mb-4">
        <div className="flex justify-between text-xs text-gray-400 mb-1">
          <span>Spread</span>
          <span>{spreadPct.toFixed(2)}% / {o.minThreshold?.toFixed(2) ?? '?'}%</span>
        </div>
        <div className="w-full bg-white/10 rounded-full h-2 overflow-hidden">
          <div
            className="h-full rounded-full bg-gradient-to-r from-indigo-500 to-purple-500 transition-all duration-500"
            style={{ width: `${Math.min((spreadPct / (o.minThreshold ?? 1)) * 100, 100)}%` }}
          />
        </div>
      </div>

      {/* Profit Breakdown */}
      <button
        onClick={(e) => { e.stopPropagation(); setShowCosts(!showCosts); }}
        className="text-xs text-gray-400 hover:text-white transition-colors mb-2 flex items-center gap-1"
      >
        {showCosts ? 'Hide' : 'View'} Costs
        <span className={`inline-block transition-transform duration-200 ${showCosts ? 'rotate-180' : ''}`}>&#9660;</span>
      </button>

      {showCosts && (
        <div className="mb-4 overflow-x-auto">
          <table className="w-full text-xs text-left">
            <thead>
              <tr className="text-gray-400 border-b border-white/10">
                <th className="py-1 pr-2">Gross Profit</th>
                <th className="py-1 pr-2">Gas</th>
                <th className="py-1 pr-2">Flash Loan Fee</th>
                <th className="py-1 pr-2">Slippage</th>
                <th className="py-1 pr-2">Bridge Fee</th>
                <th className="py-1 pr-2">Velora Fee</th>
                <th className="py-1 pr-2">Total Cost</th>
                <th className="py-1">Net Profit</th>
              </tr>
            </thead>
            <tbody>
              <tr className="text-white font-mono">
                <td className="py-1 pr-2">{usd(o.grossProfit)}</td>
                <td className="py-1 pr-2">{usd(o.gasCost)}</td>
                <td className="py-1 pr-2">{usd(o.flashLoanFee)}</td>
                <td className="py-1 pr-2">{usd(o.slippage)}</td>
                <td className="py-1 pr-2">{usd(o.bridgeFee)}</td>
                <td className="py-1 pr-2">{usd(o.veloraFee)}</td>
                <td className="py-1 pr-2">{usd(totalCost)}</td>
                <td className={`py-1 font-bold ${netProfitColor}`}>{usd(o.netProfit)}</td>
              </tr>
            </tbody>
          </table>
          <div className="flex gap-4 mt-2 text-xs">
            <span className="text-gray-400">ROI: <span className="text-white font-mono">{roiPct.toFixed(2)}%</span></span>
            <span className="text-gray-400">Net Profit %: <span className="text-white font-mono">{netProfitPct.toFixed(2)}%</span></span>
          </div>
        </div>
      )}

      {/* Flash Loan Recommendation */}
      {flashLoanPrimary && (
        <div className="mb-4 p-3 bg-indigo-500/10 border border-indigo-500/20 rounded-xl">
          <div className="text-xs text-indigo-300 font-semibold mb-1">Flash Loan Recommendation</div>
          <div className="text-white text-sm font-medium">{flashLoanPrimary.source}</div>
          <div className="flex gap-3 text-xs text-gray-400 mt-1">
            <span>Fee: {flashLoanPrimary.feePercent?.toFixed(2) ?? '?'}%</span>
            {flashLoanPrimary.feeUsd != null && <span>({usd(flashLoanPrimary.feeUsd)})</span>}
          </div>
          {flashLoanPrimary.reason && (
            <div className="text-xs text-gray-300 mt-1">{flashLoanPrimary.reason}</div>
          )}
          {flashLoanAlternatives.length > 0 && (
            <div className="mt-2 text-xs text-gray-500">
              Alternatives: {flashLoanAlternatives.map((a) => a.source).join(', ')}
            </div>
          )}
        </div>
      )}

      {/* Execution Steps */}
      {steps.length > 0 && (
        <div className="mb-4">
          <div className="text-xs text-gray-400 font-semibold mb-2">Execution Steps</div>
          <ol className="space-y-1.5">
            {steps.map((step, i) => (
              <li key={i} className="flex items-start gap-2 text-sm">
                <span className="flex-shrink-0 w-5 h-5 rounded-full bg-green-500/20 border border-green-500/40 flex items-center justify-center mt-0.5">
                  <span className="text-green-400 text-xs">&#10003;</span>
                </span>
                <span className="text-gray-300">{step}</span>
              </li>
            ))}
          </ol>
        </div>
      )}

      {/* Action Footer */}
      <div className="flex items-center justify-between pt-3 border-t border-white/10">
        {o.isProfitable && !botMode ? (
          <button
            onClick={(e) => { e.stopPropagation(); handleExecute(); }}
            className="px-5 py-2 rounded-xl text-white text-sm font-semibold bg-gradient-to-r from-indigo-600 to-purple-600 hover:from-indigo-500 hover:to-purple-500 transition-all duration-200 shadow-lg shadow-indigo-500/25"
          >
            Execute
          </button>
        ) : (
          <div />
        )}
        <div className="flex items-center gap-3">
          {o.liquidity != null && (
            <span className="text-xs text-gray-400">
              {usd(o.liquidity)} liquidity
            </span>
          )}
          {o.timestamp && (
            <span className="text-xs text-gray-500">{relTime(o.timestamp)}</span>
          )}
        </div>
      </div>
    </div>
  );
};

export default OpportunityCard;
