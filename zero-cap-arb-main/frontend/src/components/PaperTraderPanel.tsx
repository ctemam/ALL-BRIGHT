'use client'

import { useState, useEffect, useCallback } from 'react'
import type { BacktestResult, PaperTrade } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
}

export default function PaperTraderPanel({ rtl }: Props) {
  const [running, setRunning] = useState(false)
  const [loading, setLoading] = useState(true)
  const [backtesting, setBacktesting] = useState(false)
  const [data, setData] = useState<BacktestResult | null>(null)
  const [error, setError] = useState<string | null>(null)

  const fmtNumber = (n: number, decimals = 2) =>
    (n ?? 0).toFixed(decimals)

  const fetchStatus = useCallback(async () => {
    try {
      const res = await api.paperTradingStatus()
      setData(res)
      setRunning(res.trades?.some((t) => t.status === 'Simulated') ?? false)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to fetch status')
    }
  }, [])

  useEffect(() => {
    fetchStatus().finally(() => setLoading(false))
  }, [fetchStatus])

  useEffect(() => {
    if (!running) return
    const interval = setInterval(fetchStatus, 5000)
    return () => clearInterval(interval)
  }, [running, fetchStatus])

  const handleStart = useCallback(async () => {
    try {
      await api.paperTradingStart()
      setRunning(true)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to start')
    }
  }, [])

  const handleStop = useCallback(async () => {
    try {
      const res = await api.paperTradingStop()
      setData(res)
      setRunning(false)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to stop')
    }
  }, [])

  const handleBacktest = useCallback(async () => {
    setBacktesting(true)
    setError(null)
    try {
      const res = await api.paperRunBacktest()
      setData(res)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Backtest failed')
    } finally {
      setBacktesting(false)
    }
  }, [])

  const handleReset = useCallback(async () => {
    setError(null)
    try {
      await api.paperReset()
      setData(null)
      setRunning(false)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Reset failed')
    }
  }, [])

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6" dir={rtl ? 'rtl' : 'ltr'}>
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  const equityCurve = data?.equity_curve ?? []
  const maxBalance = Math.max(...equityCurve.map((p) => p.balance), 1)
  const trades = data?.trades ?? []

  return (
    <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-6 text-gray-200" dir={rtl ? 'rtl' : 'ltr'}>
      {/* Controls */}
      <div className="flex flex-wrap items-center gap-3">
        {running ? (
          <button onClick={handleStop} className="flex items-center gap-2 bg-red-500/20 border border-red-500/40 text-red-300 rounded-xl px-5 py-2.5 text-sm font-semibold hover:bg-red-500/30 transition-all">
            Stop
          </button>
        ) : (
          <button onClick={handleStart} className="flex items-center gap-2 bg-green-500/20 border border-green-500/40 text-green-300 rounded-xl px-5 py-2.5 text-sm font-semibold hover:bg-green-500/30 transition-all">
            Start
          </button>
        )}
        <button onClick={handleReset} className="bg-white/10 border border-white/20 rounded-xl px-4 py-2.5 text-sm font-medium hover:bg-white/20 transition-all">
          Reset
        </button>
        <button
          onClick={handleBacktest}
          disabled={backtesting}
          className="flex items-center gap-2 bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-5 py-2.5 text-sm font-semibold hover:bg-cyan-500/30 transition-all disabled:opacity-50"
        >
          {backtesting && <span className="w-4 h-4 border-2 border-cyan-300 border-t-transparent rounded-full animate-spin" />}
          {backtesting ? 'Running...' : 'Run 50-Trade Backtest'}
        </button>
      </div>

      {error && (
        <div className="text-sm px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300">{error}</div>
      )}

      {/* Live Stats */}
      {data && (
        <>
          <div className="grid grid-cols-2 sm:grid-cols-3 lg:grid-cols-6 gap-4">
            {[
              { label: 'Balance', value: `$${fmtNumber(data.final_balance)}`, color: 'text-cyan-300' },
              { label: 'Total Trades', value: String(data.total_trades), color: 'text-gray-200' },
              { label: 'Win Rate', value: `${fmtNumber(data.win_rate_pct)}%`, color: 'text-green-400' },
              { label: 'Profit Factor', value: fmtNumber(data.profit_factor), color: 'text-cyan-300' },
              { label: 'Sharpe Ratio', value: fmtNumber(data.sharpe_ratio), color: 'text-yellow-400' },
              { label: 'Max Drawdown', value: `${fmtNumber(data.max_drawdown_pct)}%`, color: 'text-red-400' },
            ].map((s) => (
              <div key={s.label} className="bg-white/[0.03] border border-white/10 rounded-xl p-3 text-center">
                <div className="text-xs text-gray-500 mb-1">{s.label}</div>
                <div className={`text-lg font-bold ${s.color}`}>{s.value}</div>
              </div>
            ))}
          </div>

          {/* Equity Curve */}
          <div>
            <h3 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-3">Equity Curve</h3>
            <div className="flex items-end gap-[2px] h-24 bg-black/30 rounded-xl p-2 overflow-x-auto">
              {equityCurve.length === 0 && (
                <div className="text-xs text-gray-500 italic w-full text-center self-center">No equity data</div>
              )}
              {equityCurve.map((point, i) => {
                const height = maxBalance > 0 ? (point.balance / maxBalance) * 100 : 0
                return (
                  <div
                    key={i}
                    title={`$${fmtNumber(point.balance)}`}
                    className="flex-1 min-w-[4px] bg-gradient-to-t from-cyan-500/60 to-cyan-400/80 rounded-t hover:opacity-80 transition-opacity"
                    style={{ height: `${Math.max(height, 1)}%` }}
                  />
                )
              })}
            </div>
          </div>

          {/* Trade History */}
          <div>
            <h3 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-3">
              Trade History ({trades.length})
            </h3>
            <div className="max-h-64 overflow-y-auto rounded-xl bg-black/30 border border-white/10">
              {trades.length === 0 ? (
                <div className="text-xs text-gray-500 italic p-4">No trades yet</div>
              ) : (
                <table className="w-full text-xs">
                  <thead>
                    <tr className="border-b border-white/10 text-gray-500">
                      <th className="text-left p-3">ID</th>
                      <th className="text-right p-3">Profit</th>
                      <th className="text-right p-3">ROI</th>
                      <th className="text-center p-3">Result</th>
                    </tr>
                  </thead>
                  <tbody>
                    {trades.slice().reverse().map((t) => (
                      <tr key={t.id} className="border-b border-white/5 hover:bg-white/5">
                        <td className="p-3 font-mono text-gray-400">{t.id.slice(0, 8)}</td>
                        <td className={`p-3 text-right font-medium ${t.profit_usd >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                          ${fmtNumber(t.profit_usd)}
                        </td>
                        <td className={`p-3 text-right ${t.roi_pct >= 0 ? 'text-green-400' : 'text-red-400'}`}>
                          {fmtNumber(t.roi_pct)}%
                        </td>
                        <td className="p-3 text-center">
                          <span
                            className={`px-2 py-0.5 rounded-full text-[10px] font-semibold ${
                              t.result === 'Win'
                                ? 'bg-green-500/20 text-green-300 border border-green-500/30'
                                : t.result === 'Loss'
                                ? 'bg-red-500/20 text-red-300 border border-red-500/30'
                                : 'bg-yellow-500/20 text-yellow-300 border border-yellow-500/30'
                            }`}
                          >
                            {t.result ?? 'N/A'}
                          </span>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              )}
            </div>
          </div>
        </>
      )}
    </div>
  )
}
