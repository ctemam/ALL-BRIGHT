'use client'

import { useState, useEffect, useCallback } from 'react'
import type { SplitterConfig, SplitterWallet, ProfitSplitResult, SplitEntry } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
}

function newWallet(): SplitterWallet {
  return {
    address: '',
    label: '',
    share_pct: 0,
    enabled: true,
  }
}

export default function ProfitSplitter({ rtl }: Props) {
  const [config, setConfig] = useState<SplitterConfig>({
    wallets: [],
    enabled: false,
    min_split_profit_usd: 10,
  })
  const [splitResult, setSplitResult] = useState<ProfitSplitResult | null>(null)
  const [splitAmount, setSplitAmount] = useState(100)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [calculating, setCalculating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api.getSplitterConfig()
      .then(setConfig)
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const updateWallet = useCallback((index: number, patch: Partial<SplitterWallet>) => {
    setConfig((prev) => ({
      ...prev,
      wallets: prev.wallets.map((w, i) => (i === index ? { ...w, ...patch } : w)),
    }))
  }, [])

  const addWallet = useCallback(() => {
    setConfig((prev) => ({
      ...prev,
      wallets: [...prev.wallets, newWallet()],
    }))
  }, [])

  const deleteWallet = useCallback((index: number) => {
    setConfig((prev) => ({
      ...prev,
      wallets: prev.wallets.filter((_, i) => i !== index),
    }))
  }, [])

  const totalShare = config.wallets.reduce((sum, w) => sum + (w.enabled ? w.share_pct : 0), 0)

  const handleSave = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      const updated = await api.updateSplitterConfig(config)
      setConfig(updated)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Save failed')
    } finally {
      setSaving(false)
    }
  }, [config])

  const handleCalculate = useCallback(async () => {
    setCalculating(true)
    setError(null)
    try {
      const res = await api.calculateSplit(splitAmount)
      setSplitResult(res)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Calculation failed')
    } finally {
      setCalculating(false)
    }
  }, [splitAmount])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6" dir={rtl ? 'rtl' : 'ltr'}>
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  return (
    <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-6 text-gray-200" dir={rtl ? 'rtl' : 'ltr'}>
      {/* Master Toggle */}
      <div className="flex items-center justify-between">
        <div>
          <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">Profit Splitter</h2>
          <p className="text-xs text-gray-500 mt-0.5">Distribute profits across multiple wallets</p>
        </div>
        <button
          onClick={() => setConfig((p) => ({ ...p, enabled: !p.enabled }))}
          className={`relative w-12 h-6 rounded-full transition-colors ${config.enabled ? 'bg-cyan-500' : 'bg-white/20'}`}
        >
          <span className={`absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform ${config.enabled ? 'translate-x-6' : 'translate-x-0.5'}`} />
        </button>
      </div>

      {error && (
        <div className="text-sm px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300">{error}</div>
      )}

      {/* Wallet List */}
      <div className="space-y-3">
        {config.wallets.length === 0 && (
          <div className="text-sm text-gray-500 italic text-center py-4">No wallets configured</div>
        )}
        {config.wallets.map((wallet, i) => (
          <div key={i} className="bg-black/30 border border-white/10 rounded-xl p-4 space-y-3">
            <div className="flex items-center justify-between gap-3">
              <input
                type="text"
                value={wallet.address}
                onChange={(e) => updateWallet(i, { address: e.target.value })}
                placeholder="0x..."
                className="flex-1 bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm font-mono focus:outline-none focus:border-cyan-400/50 transition-colors"
              />
              <button
                onClick={() => updateWallet(i, { enabled: !wallet.enabled })}
                className={`relative w-9 h-5 rounded-full flex-shrink-0 transition-colors ${wallet.enabled ? 'bg-cyan-500' : 'bg-white/20'}`}
              >
                <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${wallet.enabled ? 'translate-x-4' : 'translate-x-0.5'}`} />
              </button>
              <button onClick={() => deleteWallet(i)} className="text-red-400 hover:text-red-300 text-lg leading-none flex-shrink-0">&times;</button>
            </div>
            <div className="flex items-center gap-3">
              <input
                type="text"
                value={wallet.label}
                onChange={(e) => updateWallet(i, { label: e.target.value })}
                placeholder="Label..."
                className="flex-1 bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
              />
              <span className="text-xs text-gray-400 whitespace-nowrap">{wallet.share_pct}%</span>
              <input
                type="range"
                min={0}
                max={100}
                step={1}
                value={wallet.share_pct}
                onChange={(e) => updateWallet(i, { share_pct: parseInt(e.target.value) || 0 })}
                className="w-24 h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
              />
            </div>
          </div>
        ))}
      </div>

      <button onClick={addWallet} className="bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-4 py-2 text-xs font-semibold hover:bg-cyan-500/30 transition-all">
        + Add Wallet
      </button>

      {/* Total % Indicator */}
      <div className={`flex items-center gap-2 text-sm ${totalShare === 100 ? 'text-green-400' : 'text-red-400'}`}>
        <span className={`w-2 h-2 rounded-full ${totalShare === 100 ? 'bg-green-400' : 'bg-red-400'}`} />
        Total allocation: {totalShare.toFixed(0)}% {totalShare !== 100 && '(must equal 100%)'}
      </div>

      {/* Min Split Profit */}
      <div>
        <label className="text-xs text-gray-400 mb-1.5 block">Min Split Profit ($)</label>
        <input
          type="number"
          value={config.min_split_profit_usd}
          min={0}
          onChange={(e) => setConfig((p) => ({ ...p, min_split_profit_usd: parseFloat(e.target.value) || 0 }))}
          className="w-28 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
        />
      </div>

      {/* Calculate Split */}
      <div className="border-t border-white/10 pt-4">
        <h3 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-3">Calculate Split</h3>
        <div className="flex items-center gap-3 mb-4">
          <input
            type="number"
            value={splitAmount}
            min={0}
            onChange={(e) => setSplitAmount(parseFloat(e.target.value) || 0)}
            placeholder="Profit amount..."
            className="flex-1 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
          />
          <button
            onClick={handleCalculate}
            disabled={calculating || config.wallets.length === 0}
            className="flex items-center gap-2 bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-5 py-2 text-sm font-semibold hover:bg-cyan-500/30 transition-all disabled:opacity-50"
          >
            {calculating && <span className="w-4 h-4 border-2 border-cyan-300 border-t-transparent rounded-full animate-spin" />}
            Calculate
          </button>
        </div>

        {splitResult && (
          <div className="space-y-4">
            {/* Pie Chart (div-based) */}
            <div className="flex items-center gap-4">
              <div className="relative w-28 h-28 flex-shrink-0">
                {splitResult.splits
                  .filter((s) => s.percentage > 0)
                  .map((s, idx) => {
                    const colors = ['bg-cyan-500', 'bg-purple-500', 'bg-green-500', 'bg-orange-500', 'bg-pink-500', 'bg-yellow-500']
                    const angle = (splitResult.splits.slice(0, idx).reduce((a, b) => a + b.percentage, 0) / 100) * 360
                    const pct = s.percentage / 100
                    const conicGradient = `conic-gradient(${colors[idx % colors.length]} ${angle}deg ${angle + pct * 360}deg, transparent 0)`
                    return (
                      <div key={idx} className="absolute inset-0 rounded-full" style={{ background: conicGradient }} />
                    )
                  })}
                <div className="absolute inset-4 rounded-full bg-black/60 flex items-center justify-center">
                  <span className="text-xs font-bold text-gray-200">${splitResult.total_profit_usd.toFixed(2)}</span>
                </div>
              </div>
              <div className="flex-1 space-y-1.5">
                {splitResult.splits.map((s, idx) => {
                  const colors = ['text-cyan-400', 'text-purple-400', 'text-green-400', 'text-orange-400', 'text-pink-400', 'text-yellow-400']
                  const dots = ['bg-cyan-500', 'bg-purple-500', 'bg-green-500', 'bg-orange-500', 'bg-pink-500', 'bg-yellow-500']
                  return (
                    <div key={idx} className="flex items-center gap-2 text-xs">
                      <span className={`w-2 h-2 rounded-full ${dots[idx % dots.length]}`} />
                      <span className={`flex-1 ${colors[idx % colors.length]}`}>{s.label || s.address.slice(0, 6)}</span>
                      <span className="text-gray-400">{s.percentage.toFixed(1)}%</span>
                      <span className="text-gray-300 font-medium">${s.amount_usd.toFixed(2)}</span>
                    </div>
                  )
                })}
              </div>
            </div>

            <div className="text-xs text-gray-500">
              Total: ${splitResult.total_profit_usd.toFixed(2)} | Executed: {splitResult.executed ? 'Yes' : 'No'}
            </div>
          </div>
        )}
      </div>

      {/* Save */}
      <button
        onClick={handleSave}
        disabled={saving}
        className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-5 py-2.5 text-sm font-medium hover:bg-white/20 transition-all disabled:opacity-50"
      >
        {saving && <span className="w-4 h-4 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />}
        {saving ? 'Saving...' : 'Save Config'}
      </button>
    </div>
  )
}
