'use client'

import { useState, useEffect, useCallback } from 'react'
import type { MevGuardConfig, MevDetectionResult, MevRiskLevel } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
}

const RISK_LEVELS: { key: MevRiskLevel; label: string; color: string; dot: string }[] = [
  { key: 'Safe', label: 'Safe', color: 'border-green-500/40 bg-green-500/10 text-green-300', dot: 'bg-green-400' },
  { key: 'LowRisk', label: 'Low', color: 'border-yellow-500/40 bg-yellow-500/10 text-yellow-300', dot: 'bg-yellow-400' },
  { key: 'MediumRisk', label: 'Medium', color: 'border-orange-500/40 bg-orange-500/10 text-orange-300', dot: 'bg-orange-400' },
  { key: 'HighRisk', label: 'High', color: 'border-red-500/40 bg-red-500/10 text-red-300', dot: 'bg-red-400' },
  { key: 'Critical', label: 'Critical', color: 'border-red-600/40 bg-red-600/10 text-red-200', dot: 'bg-red-500' },
]

const DEFAULT_CONFIG: MevGuardConfig = {
  enabled: false,
  block_sandwich: true,
  block_frontrun: true,
  block_backrun: true,
  honeypot_check: true,
  max_risk_level: 'HighRisk',
  use_flashbots: true,
  use_private_mempool: false,
  delay_seconds: 2,
}

export default function MevGuard({ rtl }: Props) {
  const [config, setConfig] = useState<MevGuardConfig>(DEFAULT_CONFIG)
  const [result, setResult] = useState<MevDetectionResult | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [analyzing, setAnalyzing] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api.getMevConfig()
      .then(setConfig)
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    if (!config.enabled) return
    const interval = setInterval(() => {
      api.mevAnalyze(1).then(setResult).catch(() => {})
    }, 10000)
    api.mevAnalyze(1).then(setResult).catch(() => {})
    return () => clearInterval(interval)
  }, [config.enabled])

  const handleSave = useCallback(async () => {
    setSaving(true)
    try {
      const updated = await api.updateMevConfig(config)
      setConfig(updated)
    } catch {
    } finally {
      setSaving(false)
    }
  }, [config])

  const handleAnalyze = useCallback(async () => {
    setAnalyzing(true)
    setError(null)
    try {
      const res = await api.mevAnalyze(1)
      setResult(res)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Analysis failed')
    } finally {
      setAnalyzing(false)
    }
  }, [])

  const toggle = (key: keyof MevGuardConfig) =>
    setConfig((prev) => ({ ...prev, [key]: !prev[key] }))

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
          <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">MEV Protection</h2>
          <p className="text-xs text-gray-500 mt-0.5">Protect your transactions from MEV attacks</p>
        </div>
        <button
          onClick={() => toggle('enabled')}
          className={`relative w-12 h-6 rounded-full transition-colors ${config.enabled ? 'bg-cyan-500' : 'bg-white/20'}`}
        >
          <span className={`absolute top-0.5 w-5 h-5 bg-white rounded-full shadow transition-transform ${config.enabled ? 'translate-x-6' : 'translate-x-0.5'}`} />
        </button>
      </div>

      {/* Sub-toggles */}
      <div className="grid grid-cols-2 gap-3">
        {([
          { key: 'block_sandwich' as keyof MevGuardConfig, label: 'Block Sandwich' },
          { key: 'block_frontrun' as keyof MevGuardConfig, label: 'Block Frontrun' },
          { key: 'block_backrun' as keyof MevGuardConfig, label: 'Block Backrun' },
          { key: 'honeypot_check' as keyof MevGuardConfig, label: 'Honeypot Check' },
        ]).map((item) => (
          <label
            key={item.key}
            className={`flex items-center justify-between rounded-xl px-4 py-3 cursor-pointer transition-all ${config[item.key] ? 'bg-white/10 border border-cyan-400/40' : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'}`}
          >
            <span className="text-sm">{item.label}</span>
            <input
              type="checkbox"
              checked={!!config[item.key]}
              onChange={() => toggle(item.key)}
              className="appearance-none w-4 h-4 rounded border-2 border-white/20 checked:border-cyan-400 checked:bg-cyan-400/30 transition-all"
            />
          </label>
        ))}
      </div>

      {/* Risk Level */}
      <div>
        <h3 className="text-xs text-gray-400 mb-3">Max Risk Level</h3>
        <div className="flex flex-wrap gap-2">
          {RISK_LEVELS.map((rl) => (
            <label
              key={rl.key}
              className={`flex items-center gap-2 px-3 py-2 rounded-xl text-xs font-medium cursor-pointer border transition-all ${
                config.max_risk_level === rl.key
                  ? rl.color + ' shadow-[0_0_10px_rgba(34,211,238,0.15)]'
                  : 'border-white/10 bg-white/[0.03] hover:bg-white/10'
              }`}
            >
              <input
                type="radio"
                name="riskLevel"
                checked={config.max_risk_level === rl.key}
                onChange={() => setConfig((p) => ({ ...p, max_risk_level: rl.key }))}
                className="sr-only"
              />
              <span className={`w-2 h-2 rounded-full ${rl.dot}`} />
              {rl.label}
            </label>
          ))}
        </div>
      </div>

      {/* Flashbots & Private Mempool */}
      <div className="grid grid-cols-2 gap-4">
        <label className="flex items-center justify-between bg-white/[0.03] border border-white/10 rounded-xl px-4 py-3 cursor-pointer hover:bg-white/10 transition-all">
          <span className="text-sm">Use Flashbots</span>
          <button
            onClick={() => toggle('use_flashbots')}
            className={`relative w-9 h-5 rounded-full transition-colors ${config.use_flashbots ? 'bg-cyan-500' : 'bg-white/20'}`}
          >
            <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${config.use_flashbots ? 'translate-x-4' : 'translate-x-0.5'}`} />
          </button>
        </label>
        <label className="flex items-center justify-between bg-white/[0.03] border border-white/10 rounded-xl px-4 py-3 cursor-pointer hover:bg-white/10 transition-all">
          <span className="text-sm">Use Private Mempool</span>
          <button
            onClick={() => toggle('use_private_mempool')}
            className={`relative w-9 h-5 rounded-full transition-colors ${config.use_private_mempool ? 'bg-cyan-500' : 'bg-white/20'}`}
          >
            <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${config.use_private_mempool ? 'translate-x-4' : 'translate-x-0.5'}`} />
          </button>
        </label>
      </div>

      {/* Delay Slider */}
      <div>
        <label className="text-xs text-gray-400 mb-1.5 block">Delay: {config.delay_seconds}s</label>
        <input
          type="range"
          min={0}
          max={10}
          step={0.5}
          value={config.delay_seconds}
          onChange={(e) => setConfig((p) => ({ ...p, delay_seconds: parseFloat(e.target.value) || 0 }))}
          className="w-full h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
        />
      </div>

      {/* Analyze Button */}
      <button
        onClick={handleAnalyze}
        disabled={analyzing}
        className="flex items-center gap-2 bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-5 py-2.5 text-sm font-semibold hover:bg-cyan-500/30 transition-all disabled:opacity-50"
      >
        {analyzing && <span className="w-4 h-4 border-2 border-cyan-300 border-t-transparent rounded-full animate-spin" />}
        {analyzing ? 'Analyzing...' : 'Analyze Current Mempool'}
      </button>

      {error && (
        <div className="text-sm px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300">{error}</div>
      )}

      {/* Results */}
      {result && (
        <div className="space-y-4 bg-black/30 border border-white/10 rounded-xl p-4">
          {/* Risk Level Badge */}
          <div className="flex items-center justify-between">
            <span className="text-xs text-gray-400">Risk Level</span>
            {(() => {
              const rl = RISK_LEVELS.find((r) => r.key === result.risk_level)
              return rl ? (
                <span className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-xs font-semibold ${rl.color}`}>
                  <span className={`w-2 h-2 rounded-full ${rl.dot}`} />
                  {rl.label}
                </span>
              ) : (
                <span className="text-xs text-gray-400">{result.risk_level}</span>
              )
            })()}
          </div>

          {/* Probabilities */}
          {[
            { label: 'Sandwich', value: result.sandwich_probability },
            { label: 'Frontrun', value: result.frontrun_probability },
            { label: 'Backrun', value: result.backrun_probability },
          ].map((p) => (
            <div key={p.label}>
              <div className="flex justify-between text-xs mb-1">
                <span className="text-gray-400">{p.label}</span>
                <span className="text-gray-300">{(p.value * 100).toFixed(1)}%</span>
              </div>
              <div className="h-2 bg-white/10 rounded-full overflow-hidden">
                <div
                  className="h-full bg-gradient-to-r from-yellow-500 to-red-500 rounded-full transition-all"
                  style={{ width: `${Math.min(p.value * 100, 100)}%` }}
                />
              </div>
            </div>
          ))}

          {/* Detected Bots */}
          {result.detected_bots.length > 0 && (
            <div>
              <span className="text-xs text-gray-400 mb-1 block">Detected Bots</span>
              <div className="flex flex-wrap gap-1.5">
                {result.detected_bots.map((bot, i) => (
                  <span key={i} className="text-[11px] px-2 py-0.5 rounded-full bg-red-500/20 border border-red-500/30 text-red-300">
                    {bot}
                  </span>
                ))}
              </div>
            </div>
          )}

          {/* Recommended Action */}
          {result.recommended_action && (
            <div className="text-xs text-cyan-300 bg-cyan-500/10 border border-cyan-500/30 rounded-lg px-3 py-2">
              {result.recommended_action}
            </div>
          )}

          {/* Score */}
          <div className="text-[11px] text-gray-500">
            Score: {result.score.toFixed(2)} | Pending TX: {result.pending_tx_count}
          </div>
        </div>
      )}

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
