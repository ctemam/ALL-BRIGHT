'use client'

import { useState, useEffect, useCallback } from 'react'
import type { PortfolioConfig, PortfolioStatus, ArbitrageType, StrategyAllocation } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
}

const STRATEGIES: { key: ArbitrageType; label: string; color: string }[] = [
  { key: 'Simple', label: 'Simple', color: 'from-cyan-500/30 to-cyan-400/10' },
  { key: 'Triangular', label: 'Triangular', color: 'from-purple-500/30 to-purple-400/10' },
  { key: 'CrossChain', label: 'CrossChain', color: 'from-blue-500/30 to-blue-400/10' },
  { key: 'Mint', label: 'Mint', color: 'from-green-500/30 to-green-400/10' },
  { key: 'JitLiquidity', label: 'JIT', color: 'from-orange-500/30 to-orange-400/10' },
]

const DEFAULT_STRATEGIES: StrategyAllocation[] = STRATEGIES.map((s) => ({
  strategy: s.key,
  weight_pct: 20,
  max_concurrent: 3,
  min_profit_usd: 10,
  max_daily_trades: 20,
  daily_trades: 0,
}))

export default function PortfolioManager({ rtl }: Props) {
  const [config, setConfig] = useState<PortfolioConfig>({
    strategies: DEFAULT_STRATEGIES,
    total_balance_usd: 10000,
    risk_per_trade_pct: 1,
    max_daily_loss_usd: 500,
    daily_loss: 0,
    max_open_positions: 10,
    open_positions: 0,
  })
  const [status, setStatus] = useState<PortfolioStatus | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)

  useEffect(() => {
    Promise.all([api.getPortfolioConfig(), api.getPortfolioStatus()])
      .then(([cfg, st]) => {
        setConfig(cfg)
        setStatus(st)
      })
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const updateStrategy = useCallback(
    (index: number, patch: Partial<StrategyAllocation>) => {
      setConfig((prev) => {
        const strategies = prev.strategies.map((s, i) => (i === index ? { ...s, ...patch } : s))
        return { ...prev, strategies }
      })
    },
    []
  )

  const autoSave = useCallback(async () => {
    setSaving(true)
    try {
      const updated = await api.updatePortfolioConfig(config)
      setConfig(updated)
    } catch {
    } finally {
      setSaving(false)
    }
  }, [config])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6" dir={rtl ? 'rtl' : 'ltr'}>
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  return (
    <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-6 text-gray-200" dir={rtl ? 'rtl' : 'ltr'}>
      {/* Summary */}
      <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
        {[
          { label: 'Total PnL', value: status ? `$${(status.total_pnl_usd ?? 0).toFixed(2)}` : '--', color: (status?.total_pnl_usd ?? 0) >= 0 ? 'text-green-400' : 'text-red-400' },
          { label: 'Daily PnL', value: status ? `$${(status.daily_pnl_usd ?? 0).toFixed(2)}` : '--', color: (status?.daily_pnl_usd ?? 0) >= 0 ? 'text-green-400' : 'text-red-400' },
          { label: 'Win Rate', value: status ? `${(status.win_rate_pct ?? 0).toFixed(1)}%` : '--', color: 'text-cyan-300' },
          { label: 'Open Positions', value: status ? String(status.open_trades ?? 0) : '--', color: 'text-gray-200' },
        ].map((s) => (
          <div key={s.label} className="bg-white/[0.03] border border-white/10 rounded-xl p-3 text-center">
            <div className="text-xs text-gray-500 mb-1">{s.label}</div>
            <div className={`text-lg font-bold ${s.color}`}>{s.value}</div>
          </div>
        ))}
      </div>

      {/* Strategy Cards */}
      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {config.strategies.map((strat, i) => {
          const stratDef = STRATEGIES.find((s) => s.key === strat.strategy)
          const breakdown = status?.strategy_breakdown?.find((b) => b.strategy === strat.strategy)
          return (
            <div key={strat.strategy} className={`bg-gradient-to-br ${stratDef?.color} bg-white/[0.03] border border-white/10 rounded-xl p-4 space-y-3`}>
              <div className="flex items-center justify-between">
                <span className="text-sm font-semibold">{stratDef?.label ?? strat.strategy}</span>
                <button
                  onClick={() => updateStrategy(i, { weight_pct: strat.weight_pct > 0 ? 0 : 20 })}
                  className={`relative w-9 h-5 rounded-full transition-colors ${strat.weight_pct > 0 ? 'bg-cyan-500' : 'bg-white/20'}`}
                >
                  <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${strat.weight_pct > 0 ? 'translate-x-4' : 'translate-x-0.5'}`} />
                </button>
              </div>

              <div>
                <label className="text-[11px] text-gray-500">Weight: {strat.weight_pct}%</label>
                <input
                  type="range"
                  min={0}
                  max={100}
                  step={5}
                  value={strat.weight_pct}
                  onChange={(e) => updateStrategy(i, { weight_pct: parseInt(e.target.value) || 0 })}
                  onMouseUp={autoSave}
                  onTouchEnd={autoSave}
                  className="w-full h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400 mt-1"
                />
              </div>

              <div>
                <label className="text-[11px] text-gray-500">Max Concurrent</label>
                <input
                  type="number"
                  value={strat.max_concurrent}
                  min={1}
                  max={50}
                  onChange={(e) => updateStrategy(i, { max_concurrent: parseInt(e.target.value) || 1 })}
                  onBlur={autoSave}
                  className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
                />
              </div>

              <div>
                <label className="text-[11px] text-gray-500">Min Profit ($)</label>
                <input
                  type="number"
                  value={strat.min_profit_usd}
                  min={0}
                  step={1}
                  onChange={(e) => updateStrategy(i, { min_profit_usd: parseFloat(e.target.value) || 0 })}
                  onBlur={autoSave}
                  className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
                />
              </div>

              {breakdown && (
                <div className="text-[11px] text-gray-500 pt-1 border-t border-white/10">
                  Trades: {breakdown.total_trades} | W: {breakdown.wins} L: {breakdown.losses} | PnL: ${breakdown.pnl_usd?.toFixed(2) ?? '0.00'}
                </div>
              )}
            </div>
          )
        })}
      </div>

      {/* Global Settings */}
      <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <div>
          <label className="text-xs text-gray-400 mb-1 block">Total Balance ($)</label>
          <input
            type="number"
            value={config.total_balance_usd}
            min={0}
            onChange={(e) => setConfig((p) => ({ ...p, total_balance_usd: parseFloat(e.target.value) || 0 }))}
            onBlur={autoSave}
            className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
          />
        </div>
        <div>
          <label className="text-xs text-gray-400 mb-1 block">Risk Per Trade (%)</label>
          <input
            type="number"
            value={config.risk_per_trade_pct}
            min={0.1}
            max={100}
            step={0.1}
            onChange={(e) => setConfig((p) => ({ ...p, risk_per_trade_pct: parseFloat(e.target.value) || 0 }))}
            onBlur={autoSave}
            className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
          />
        </div>
        <div>
          <label className="text-xs text-gray-400 mb-1 block">Max Daily Loss ($)</label>
          <input
            type="number"
            value={config.max_daily_loss_usd}
            min={0}
            onChange={(e) => setConfig((p) => ({ ...p, max_daily_loss_usd: parseFloat(e.target.value) || 0 }))}
            onBlur={autoSave}
            className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
          />
        </div>
      </div>

      <div className="flex items-center justify-between pt-2 border-t border-white/10">
        <div className="text-xs text-gray-500">
          Max Open Positions: {config.max_open_positions} | Current: {config.open_positions}
        </div>
        <button
          onClick={autoSave}
          disabled={saving}
          className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-5 py-2 text-sm font-medium hover:bg-white/20 transition-all disabled:opacity-50"
        >
          {saving && <span className="w-4 h-4 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />}
          {saving ? 'Saving...' : 'Save Config'}
        </button>
      </div>
    </div>
  )
}
