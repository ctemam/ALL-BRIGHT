'use client'

import { useState, useEffect, useCallback } from 'react'
import type { GasBidConfig, GasRecommendation, GasBidStrategy } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
}

const STRATEGIES: { key: GasBidStrategy; label: string; icon: string; desc: string }[] = [
  { key: 'Fixed', label: 'Fixed', icon: '⚙', desc: 'Use a fixed gas price' },
  { key: 'Adaptive', label: 'Adaptive', icon: '📊', desc: 'Auto-adjust based on network' },
  { key: 'Priority', label: 'Priority', icon: '🚀', desc: 'Priority fee markup' },
  { key: 'MEVProtected', label: 'MEV Protected', icon: '🛡', desc: 'MEV-safe gas strategy' },
]

const CHAINS = [
  { id: 1, name: 'Ethereum' },
  { id: 42161, name: 'Arbitrum' },
  { id: 10, name: 'Optimism' },
  { id: 137, name: 'Polygon' },
  { id: 56, name: 'BSC' },
  { id: 43114, name: 'Avalanche' },
]

const DEFAULT_CONFIG: GasBidConfig = {
  strategy: 'Fixed',
  max_gas_price_gwei: 50,
  min_gas_price_gwei: 1,
  priority_pct: 10,
  adaptive_enabled: false,
}

export default function GasBidder({ rtl }: Props) {
  const [config, setConfig] = useState<GasBidConfig>(DEFAULT_CONFIG)
  const [recommendation, setRecommendation] = useState<GasRecommendation | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [recommending, setRecommending] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const [recChainId, setRecChainId] = useState(1)
  const [recProfit, setRecProfit] = useState(100)
  const [recSpread, setRecSpread] = useState(0.5)

  useEffect(() => {
    api.getGasConfig()
      .then(setConfig)
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const handleSave = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      const updated = await api.updateGasConfig(config)
      setConfig(updated)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Save failed')
    } finally {
      setSaving(false)
    }
  }, [config])

  const handleRecommend = useCallback(async () => {
    setRecommending(true)
    setError(null)
    try {
      const res = await api.recommendGas(recChainId, recProfit, recSpread)
      setRecommendation(res)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Recommendation failed')
    } finally {
      setRecommending(false)
    }
  }, [recChainId, recProfit, recSpread])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6" dir={rtl ? 'rtl' : 'ltr'}>
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  return (
    <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-6 text-gray-200" dir={rtl ? 'rtl' : 'ltr'}>
      <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">Gas Bidding</h2>

      {error && (
        <div className="text-sm px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300">{error}</div>
      )}

      {/* Strategy Selector */}
      <div className="grid grid-cols-2 gap-3">
        {STRATEGIES.map((s) => (
          <button
            key={s.key}
            onClick={() => setConfig((p) => ({ ...p, strategy: s.key }))}
            className={`text-left rounded-xl p-4 transition-all ${
              config.strategy === s.key
                ? 'bg-white/10 border-2 border-cyan-400 shadow-[0_0_15px_rgba(34,211,238,0.2)]'
                : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'
            }`}
          >
            <div className="text-lg mb-1">{s.icon}</div>
            <div className="text-sm font-semibold">{s.label}</div>
            <div className="text-[11px] text-gray-500 mt-0.5">{s.desc}</div>
          </button>
        ))}
      </div>

      {/* Strategy-specific Fields */}
      <div className="bg-black/30 border border-white/10 rounded-xl p-4 space-y-4">
        <div className="grid grid-cols-1 sm:grid-cols-2 gap-4">
          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Max Gwei</label>
            <input
              type="number"
              value={config.max_gas_price_gwei}
              min={0}
              step={0.1}
              onChange={(e) => setConfig((p) => ({ ...p, max_gas_price_gwei: parseFloat(e.target.value) || 0 }))}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
            />
          </div>

          {(config.strategy === 'Adaptive' || config.strategy === 'MEVProtected') && (
            <div>
              <label className="text-xs text-gray-400 mb-1.5 block">Min Gwei</label>
              <input
                type="number"
                value={config.min_gas_price_gwei}
                min={0}
                step={0.1}
                onChange={(e) => setConfig((p) => ({ ...p, min_gas_price_gwei: parseFloat(e.target.value) || 0 }))}
                className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
              />
            </div>
          )}
        </div>

        {(config.strategy === 'Adaptive') && (
          <label className="flex items-center justify-between bg-white/[0.03] border border-white/10 rounded-xl px-4 py-3 cursor-pointer hover:bg-white/10 transition-all">
            <span className="text-sm">Adaptive Mode</span>
            <button
              onClick={() => setConfig((p) => ({ ...p, adaptive_enabled: !p.adaptive_enabled }))}
              className={`relative w-9 h-5 rounded-full transition-colors ${config.adaptive_enabled ? 'bg-cyan-500' : 'bg-white/20'}`}
            >
              <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${config.adaptive_enabled ? 'translate-x-4' : 'translate-x-0.5'}`} />
            </button>
          </label>
        )}

        {config.strategy === 'Priority' && (
          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Priority: {config.priority_pct}%</label>
            <input
              type="range"
              min={0}
              max={100}
              step={5}
              value={config.priority_pct}
              onChange={(e) => setConfig((p) => ({ ...p, priority_pct: parseInt(e.target.value) || 0 }))}
              className="w-full h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
            />
          </div>
        )}
      </div>

      {/* Get Recommendation */}
      <div className="border-t border-white/10 pt-4 space-y-4">
        <h3 className="text-sm font-semibold uppercase tracking-widest text-gray-400">Get Recommendation</h3>

        <div className="grid grid-cols-1 sm:grid-cols-3 gap-4">
          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Chain</label>
            <select
              value={recChainId}
              onChange={(e) => setRecChainId(parseInt(e.target.value))}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
            >
              {CHAINS.map((c) => (
                <option key={c.id} value={c.id}>{c.name}</option>
              ))}
            </select>
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Profit ($)</label>
            <input
              type="number"
              value={recProfit}
              min={0}
              onChange={(e) => setRecProfit(parseFloat(e.target.value) || 0)}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
            />
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Spread (%)</label>
            <input
              type="number"
              value={recSpread}
              min={0}
              step={0.1}
              onChange={(e) => setRecSpread(parseFloat(e.target.value) || 0)}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
            />
          </div>
        </div>

        <button
          onClick={handleRecommend}
          disabled={recommending}
          className="flex items-center gap-2 bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-5 py-2.5 text-sm font-semibold hover:bg-cyan-500/30 transition-all disabled:opacity-50"
        >
          {recommending && <span className="w-4 h-4 border-2 border-cyan-300 border-t-transparent rounded-full animate-spin" />}
          {recommending ? 'Recommending...' : 'Recommend'}
        </button>

        {recommendation && (
          <div className="bg-black/30 border border-cyan-400/30 rounded-xl p-5 space-y-4">
            <div className="text-center">
              <div className="text-xs text-gray-500 mb-1">Suggested Gwei</div>
              <div className="text-4xl font-bold text-cyan-300">{recommendation.suggested_gwei.toFixed(2)}</div>
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="bg-white/[0.03] border border-white/10 rounded-lg p-3 text-center">
                <div className="text-xs text-gray-500 mb-1">Est. Cost USD</div>
                <div className="text-lg font-semibold text-gray-200">${recommendation.estimated_cost_usd.toFixed(2)}</div>
              </div>
              <div className="bg-white/[0.03] border border-white/10 rounded-lg p-3 text-center">
                <div className="text-xs text-gray-500 mb-1">Confidence</div>
                <span className={`inline-block text-xs font-semibold px-2.5 py-0.5 rounded-full ${
                  recommendation.confidence === 'high'
                    ? 'bg-green-500/20 text-green-300 border border-green-500/30'
                    : recommendation.confidence === 'medium'
                    ? 'bg-yellow-500/20 text-yellow-300 border border-yellow-500/30'
                    : 'bg-red-500/20 text-red-300 border border-red-500/30'
                }`}>
                  {recommendation.confidence}
                </span>
              </div>
            </div>

            <div>
              <span className="text-xs text-gray-400 mb-2 block">Reasoning</span>
              <ul className="space-y-1.5">
                {recommendation.reasoning.map((r, i) => (
                  <li key={i} className="flex items-start gap-2 text-xs text-gray-300">
                    <span className="text-cyan-400 mt-0.5">&#8226;</span>
                    {r}
                  </li>
                ))}
              </ul>
            </div>

            <div className="text-xs text-gray-500">
              Strategy: {recommendation.strategy}
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
