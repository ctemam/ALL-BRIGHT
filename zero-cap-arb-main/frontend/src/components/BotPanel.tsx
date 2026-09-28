'use client'

import { useState, useEffect, useRef, useCallback } from 'react'
import type {
  BotConfig,
  BotStatus,
  BotMode,
  FlashLoanSource,
  GasStrategy,
  ArbitrageType,
  BotLogEntry,
} from '@/lib/api'
import { api } from '@/lib/api'
import PaperTraderPanel from './PaperTraderPanel'
import PortfolioManager from './PortfolioManager'
import MevGuard from './MevGuard'
import AlertsConfig from './AlertsConfig'
import RulesBuilder from './RulesBuilder'
import ProfitSplitter from './ProfitSplitter'
import GasBidder from './GasBidder'

const MODES: { key: BotMode; label: string; desc: string }[] = [
  { key: 'manual', label: 'Manual', desc: 'Bot finds opportunities, you review & approve each trade' },
  { key: 'semi-auto', label: 'Semi-Auto', desc: 'Bot auto-executes small trades, asks for approval on large ones' },
  { key: 'auto', label: 'Auto', desc: 'Full autonomous execution based on your strategy filters' },
]

const ARBITRAGE_TYPES: { key: ArbitrageType; label: string; icon: string; desc: string }[] = [
  { key: 'simple', label: 'Simple', icon: '⇄', desc: 'Basic DEX arbitrage between two pools' },
  { key: 'triangular', label: 'Triangular', icon: '△', desc: 'Three-currency cycle arbitrage' },
  { key: 'crossChain', label: 'Cross-Chain', icon: '⛓', desc: 'Arbitrage across different chains' },
  { key: 'mint', label: 'Mint', icon: '🪙', desc: 'Mint/burn based arbitrage' },
  { key: 'jitLiquidity', label: 'JIT Liquidity', icon: '⚡', desc: 'Just-in-time liquidity arbitrage' },
]

const CHAINS: { id: number; name: string; label: string; currency: string }[] = [
  { id: 1, name: 'ethereum', label: 'Ethereum', currency: 'ETH' },
  { id: 42161, name: 'arbitrum', label: 'Arbitrum', currency: 'ETH' },
  { id: 10, name: 'optimism', label: 'Optimism', currency: 'ETH' },
  { id: 137, name: 'polygon', label: 'Polygon', currency: 'MATIC' },
  { id: 56, name: 'bsc', label: 'BSC', currency: 'BNB' },
  { id: 43114, name: 'avalanche', label: 'Avalanche', currency: 'AVAX' },
]

const FLASH_LOAN_SOURCES: { key: FlashLoanSource; label: string; fee: string }[] = [
  { key: 'spark', label: 'Spark', fee: '0.00%' },
  { key: 'aaveV3', label: 'Aave V3', fee: '0.05%' },
  { key: 'radiantV2', label: 'Radiant V2', fee: '0.04%' },
]

const GAS_STRATEGIES: { key: GasStrategy; label: string }[] = [
  { key: 'flashbots', label: 'Flashbots' },
  { key: 'pimlico', label: 'Pimlico' },
  { key: 'zerodev', label: 'ZeroDev' },
]

const LEVEL_COLORS: Record<string, string> = {
  info: 'text-cyan-300',
  warn: 'text-yellow-400',
  error: 'text-red-400',
  debug: 'text-gray-500',
  success: 'text-green-400',
}

const DEFAULT_CONFIG: BotConfig = {
  mode: 'manual',
  arbitrageTypes: ['simple'],
  minNetProfit: 10,
  maxSlippage: 0.5,
  maxGasPrice: 50,
  scanInterval: 10,
  maxConcurrentTrades: 3,
  flashLoanSource: 'spark',
  gasStrategy: 'flashbots',
  enabledChains: [1],
}

const PRO_TABS = [
  { key: 'paper', label: 'Paper Trading', icon: '📊' },
  { key: 'portfolio', label: 'Portfolio', icon: '📁' },
  { key: 'mev', label: 'MEV Guard', icon: '🛡️' },
  { key: 'alerts', label: 'Alerts', icon: '🔔' },
  { key: 'rules', label: 'Rules', icon: '⚙️' },
  { key: 'splitter', label: 'Splitter', icon: '💸' },
  { key: 'gas', label: 'Gas Bidder', icon: '⛽' },
]

export default function BotPanel({ rtl }: { rtl?: boolean }) {
  const [config, setConfig] = useState<BotConfig>(DEFAULT_CONFIG)
  const [status, setStatus] = useState<BotStatus | null>(null)
  const [logs, setLogs] = useState<BotLogEntry[]>([])
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [logsOpen, setLogsOpen] = useState(false)
  const [proTab, setProTab] = useState('paper')
  const logsEndRef = useRef<HTMLDivElement>(null)

  useEffect(() => {
    Promise.all([api.getBotConfig(), api.getBotStatus()])
      .then(([cfg, st]) => {
        setConfig(cfg)
        setStatus(st)
      })
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  useEffect(() => {
    if (!logsOpen) return
    api.getBotLogs().then(setLogs).catch(() => {})
  }, [logsOpen])

  useEffect(() => {
    logsEndRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [logs])

  const toggleArbitrage = useCallback((type: ArbitrageType) => {
    setConfig((prev) => {
      const set = new Set(prev.arbitrageTypes)
      set.has(type) ? set.delete(type) : set.add(type)
      return { ...prev, arbitrageTypes: Array.from(set) }
    })
  }, [])

  const toggleChain = useCallback((chainId: number) => {
    setConfig((prev) => {
      const set = new Set(prev.enabledChains)
      set.has(chainId) ? set.delete(chainId) : set.add(chainId)
      return { ...prev, enabledChains: Array.from(set) }
    })
  }, [])

  const updateConfig = useCallback(async () => {
    setSaving(true)
    try {
      const updated = await api.updateBotConfig(config)
      setConfig(updated)
    } catch {
    } finally {
      setSaving(false)
    }
  }, [config])

  const startBot = useCallback(async () => {
    setSaving(true)
    try {
      const st = await api.startBot()
      setStatus(st)
    } catch {
    } finally {
      setSaving(false)
    }
  }, [])

  const stopBot = useCallback(async () => {
    setSaving(true)
    try {
      const st = await api.stopBot()
      setStatus(st)
    } catch {
    } finally {
      setSaving(false)
    }
  }, [])

  const fmtTime = (ts: string | number) =>
    new Intl.DateTimeFormat('en-US', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
    }).format(new Date(ts))

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6" dir={rtl ? 'rtl' : 'ltr'}>
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  return (
    <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-8 text-gray-200" dir={rtl ? 'rtl' : 'ltr'}>
      {/* Mode Selector */}
      <section>
        <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-4">Mode</h2>
        <div className="grid grid-cols-3 gap-4">
          {MODES.map((m) => (
            <button
              key={m.key}
              onClick={() => setConfig((p) => ({ ...p, mode: m.key }))}
              className={`relative rounded-xl p-4 text-left transition-all duration-200 ${
                config.mode === m.key
                  ? 'bg-white/10 border-2 border-cyan-400 shadow-[0_0_20px_rgba(34,211,238,0.3)]'
                  : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'
              }`}
            >
              <div className="text-lg font-bold mb-1">{m.label}</div>
              <div className="text-xs text-gray-400 leading-relaxed">{m.desc}</div>
            </button>
          ))}
        </div>
      </section>

      {/* Strategy Toggles */}
      <section>
        <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-4">Strategies</h2>
        <div className="grid grid-cols-2 lg:grid-cols-3 gap-3">
          {ARBITRAGE_TYPES.map((a) => {
            const active = config.arbitrageTypes.includes(a.key)
            return (
              <button
                key={a.key}
                onClick={() => toggleArbitrage(a.key)}
                className={`flex items-start gap-3 rounded-xl p-3 text-left transition-all duration-200 ${
                  active
                    ? 'bg-white/10 border border-cyan-400/60'
                    : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'
                }`}
              >
                <span className="text-xl mt-0.5">{a.icon}</span>
                <div className="flex-1 min-w-0">
                  <div className="text-sm font-medium">{a.label}</div>
                  <div className="text-[11px] text-gray-500 mt-0.5 leading-snug">{a.desc}</div>
                </div>
                <div
                  className={`w-5 h-5 rounded border-2 flex-shrink-0 mt-0.5 flex items-center justify-center transition-all ${
                    active ? 'border-cyan-400 bg-cyan-400/20' : 'border-white/20'
                  }`}
                >
                  {active && <span className="text-cyan-400 text-xs">✓</span>}
                </div>
              </button>
            )
          })}
        </div>
      </section>

      {/* Filters & Limits */}
      <section>
        <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-4">Filters &amp; Limits</h2>
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-5">
          {([
            { label: 'Min Net Profit ($)', key: 'minNetProfit', min: 0, max: 1000, step: 1 },
            { label: 'Max Slippage (%)', key: 'maxSlippage', min: 0, max: 10, step: 0.1 },
            { label: 'Max Gas Price (Gwei)', key: 'maxGasPrice', min: 1, max: 500, step: 1 },
            { label: 'Scan Interval (seconds)', key: 'scanInterval', min: 1, max: 300, step: 1 },
            { label: 'Max Concurrent Trades', key: 'maxConcurrentTrades', min: 1, max: 20, step: 1 },
          ] as const).map((field) => (
            <div key={field.key}>
              <label className="text-xs text-gray-400 mb-1.5 block">{field.label}</label>
              <div className="flex items-center gap-3">
                <input
                  type="number"
                  value={config[field.key] as number}
                  min={field.min}
                  max={field.max}
                  step={field.step}
                  onChange={(e) =>
                    setConfig((p) => ({ ...p, [field.key]: parseFloat(e.target.value) || 0 }))
                  }
                  className="w-24 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
                />
                <input
                  type="range"
                  min={field.min}
                  max={field.max}
                  step={field.step}
                  value={config[field.key] as number}
                  onChange={(e) =>
                    setConfig((p) => ({ ...p, [field.key]: parseFloat(e.target.value) || 0 }))
                  }
                  className="flex-1 h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
                />
              </div>
            </div>
          ))}
        </div>
      </section>

      {/* Gas & Flash Loan Strategy */}
      <div className="grid grid-cols-1 md:grid-cols-2 gap-6">
        <section>
          <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-4">Flash Loan Source</h2>
          <div className="space-y-2">
            {FLASH_LOAN_SOURCES.map((f) => (
              <label
                key={f.key}
                className={`flex items-center gap-3 rounded-xl px-4 py-3 cursor-pointer transition-all ${
                  config.flashLoanSource === f.key
                    ? 'bg-white/10 border border-cyan-400/60'
                    : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'
                }`}
              >
                <input
                  type="radio"
                  name="flashLoan"
                  checked={config.flashLoanSource === f.key}
                  onChange={() => setConfig((p) => ({ ...p, flashLoanSource: f.key }))}
                  className="appearance-none w-4 h-4 rounded-full border-2 border-white/20 checked:border-cyan-400 checked:bg-cyan-400/30 checked:shadow-[0_0_8px_rgba(34,211,238,0.4)] transition-all"
                />
                <span className="text-sm font-medium flex-1">{f.label}</span>
                <span className="text-xs text-gray-500">Fee: {f.fee}</span>
              </label>
            ))}
          </div>
        </section>

        <section>
          <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-4">Gas Strategy</h2>
          <div className="space-y-2">
            {GAS_STRATEGIES.map((g) => (
              <label
                key={g.key}
                className={`flex items-center gap-3 rounded-xl px-4 py-3 cursor-pointer transition-all ${
                  config.gasStrategy === g.key
                    ? 'bg-white/10 border border-cyan-400/60'
                    : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'
                }`}
              >
                <input
                  type="radio"
                  name="gasStrategy"
                  checked={config.gasStrategy === g.key}
                  onChange={() => setConfig((p) => ({ ...p, gasStrategy: g.key }))}
                  className="appearance-none w-4 h-4 rounded-full border-2 border-white/20 checked:border-cyan-400 checked:bg-cyan-400/30 checked:shadow-[0_0_8px_rgba(34,211,238,0.4)] transition-all"
                />
                <span className="text-sm font-medium">{g.label}</span>
              </label>
            ))}
          </div>
        </section>
      </div>

      {/* Chain Toggles */}
      <section>
        <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-4">Chains</h2>
        <div className="flex flex-wrap gap-3">
          {CHAINS.map((c) => {
            const active = config.enabledChains.includes(c.id)
            return (
              <button
                key={c.id}
                onClick={() => toggleChain(c.id)}
                className={`flex items-center gap-2 rounded-xl px-4 py-2.5 text-sm transition-all ${
                  active
                    ? 'bg-white/10 border border-cyan-400/60 shadow-[0_0_12px_rgba(34,211,238,0.15)]'
                    : 'bg-white/[0.03] border border-white/10 hover:bg-white/10'
                }`}
              >
                <div
                  className={`w-2 h-2 rounded-full transition-colors ${active ? 'bg-cyan-400' : 'bg-white/20'}`}
                />
                <span className="font-medium">{c.label}</span>
                <span className="text-[11px] text-gray-500">
                  {c.currency} · ID {c.id}
                </span>
              </button>
            )
          })}
        </div>
      </section>

      {/* Controls Footer */}
      <section className="flex flex-wrap items-center justify-between gap-4 pt-4 border-t border-white/10">
        <div className="flex items-center gap-4">
          {status?.running ? (
            <button
              onClick={stopBot}
              disabled={saving}
              className="relative flex items-center gap-2 bg-red-500/20 border border-red-500/40 text-red-300 rounded-xl px-6 py-2.5 text-sm font-semibold hover:bg-red-500/30 transition-all disabled:opacity-50"
            >
              {saving && (
                <span className="w-4 h-4 border-2 border-red-300 border-t-transparent rounded-full animate-spin" />
              )}
              STOP BOT
            </button>
          ) : (
            <button
              onClick={startBot}
              disabled={saving}
              className="relative flex items-center gap-2 bg-green-500/20 border border-green-500/40 text-green-300 rounded-xl px-6 py-2.5 text-sm font-semibold hover:bg-green-500/30 transition-all disabled:opacity-50"
            >
              {saving && (
                <span className="w-4 h-4 border-2 border-green-300 border-t-transparent rounded-full animate-spin" />
              )}
              START BOT
            </button>
          )}

          <div className="flex items-center gap-2 text-sm">
            <span
              className={`w-2 h-2 rounded-full ${
                status?.running ? 'bg-green-400 animate-pulse shadow-[0_0_8px_rgba(74,222,128,0.5)]' : 'bg-gray-500'
              }`}
            />
            <span className="text-gray-400">{status?.running ? 'Running' : 'Stopped'}</span>
          </div>
        </div>

        <div className="flex items-center gap-5 text-sm">
          <div className="text-center">
            <div className="text-xs text-gray-500">Total</div>
            <div className="font-medium">{status?.stats?.total ?? 0}</div>
          </div>
          <div className="text-center">
            <div className="text-xs text-gray-500">Successful</div>
            <div className="font-medium text-green-400">{status?.stats?.successful ?? 0}</div>
          </div>
          <div className="text-center">
            <div className="text-xs text-gray-500">Failed</div>
            <div className="font-medium text-red-400">{status?.stats?.failed ?? 0}</div>
          </div>
          <div className="text-center">
            <div className="text-xs text-gray-500">Profit</div>
            <div className="font-medium text-cyan-300">
              ${(status?.stats?.totalProfit ?? 0).toFixed(2)}
            </div>
          </div>
        </div>

        <div className="flex items-center gap-3">
          <button
            onClick={updateConfig}
            disabled={saving}
            className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-5 py-2.5 text-sm font-medium hover:bg-white/20 transition-all disabled:opacity-50"
          >
            {saving && (
              <span className="w-4 h-4 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />
            )}
            Save Config
          </button>
        </div>
      </section>

      {/* Live Logs */}
      <section>
        <button
          onClick={() => setLogsOpen((o) => !o)}
          className="flex items-center gap-2 text-sm text-gray-400 hover:text-gray-200 transition-colors"
        >
          <span className={`transition-transform duration-200 ${logsOpen ? 'rotate-90' : ''}`}>▶</span>
          Live Logs {logs.length > 0 && `(${logs.length})`}
        </button>

        {logsOpen && (
          <div className="mt-3 max-h-64 overflow-y-auto rounded-xl bg-black/40 border border-white/10 p-3 space-y-1 font-mono text-xs">
            {logs.length === 0 && (
              <div className="text-gray-500 italic">No logs yet...</div>
            )}
            {logs.slice(-20).map((entry, i) => (
              <div key={i} className="flex items-start gap-3">
                <span className="text-gray-600 whitespace-nowrap">{fmtTime(entry.timestamp)}</span>
                <span className={`uppercase font-bold w-14 flex-shrink-0 ${LEVEL_COLORS[entry.level] || 'text-gray-300'}`}>
                  {entry.level}
                </span>
                <span className="text-gray-300 break-words">{entry.message}</span>
              </div>
            ))}
            <div ref={logsEndRef} />
          </div>
        )}
      </section>

      {/* ─── Pro Features ─────────────────────────────── */}
      <section className="mt-8 border-t border-white/10 pt-8">
        <div className="flex items-center justify-between mb-6">
          <h3 className="text-lg font-bold text-white">{rtl ? 'الميزات المتقدمة' : 'Pro Features'}</h3>
          <span className="text-[10px] uppercase tracking-widest text-indigo-400 bg-indigo-500/10 px-3 py-1 rounded-full border border-indigo-500/20">
            {rtl ? 'احترافية' : 'BOT v2.0'}
          </span>
        </div>

        {/* Sub-tab navigation */}
        <div className="flex gap-1 mb-6 overflow-x-auto scrollbar-none flex-wrap">
          {PRO_TABS.map((tab) => (
            <button
              key={tab.key}
              onClick={() => setProTab(tab.key)}
              className={`flex items-center gap-2 px-3 py-1.5 text-xs font-medium rounded-lg transition-all whitespace-nowrap ${
                proTab === tab.key
                  ? 'bg-indigo-500/20 text-indigo-300 border border-indigo-500/30'
                  : 'text-gray-400 hover:text-white hover:bg-white/5 border border-transparent'
              }`}
            >
              <span className="text-base">{tab.icon}</span>
              {tab.label}
            </button>
          ))}
        </div>

        {/* Sub-tab content */}
        {proTab === 'paper' && <PaperTraderPanel rtl={rtl} />}
        {proTab === 'portfolio' && <PortfolioManager rtl={rtl} />}
        {proTab === 'mev' && <MevGuard rtl={rtl} />}
        {proTab === 'alerts' && <AlertsConfig rtl={rtl} />}
        {proTab === 'rules' && <RulesBuilder rtl={rtl} />}
        {proTab === 'splitter' && <ProfitSplitter rtl={rtl} />}
        {proTab === 'gas' && <GasBidder rtl={rtl} />}
      </section>
    </div>
  )
}
