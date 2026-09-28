'use client'

import { useState, useEffect, useCallback } from 'react'
import type { AlertConfig, AlertMessage, AlertChannel, AlertEventType } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
}

const EVENT_TYPES: AlertEventType[] = [
  'TradeExecuted', 'OpportunityFound', 'ProfitTaken', 'LossTriggered',
  'ErrorOccurred', 'MevDetected', 'BotStarted', 'BotStopped', 'DailySummary',
]

const CHANNEL_TYPES: AlertChannel[] = ['Telegram', 'Discord', 'Webhook']

function emptyChannel(): AlertConfig {
  return {
    channel: 'Telegram',
    webhook_url: '',
    events: ['TradeExecuted', 'ProfitTaken'],
    min_profit_usd: 10,
    enabled: true,
    notify_on_error: true,
    daily_summary: true,
  }
}

export default function AlertsConfig({ rtl }: Props) {
  const [channels, setChannels] = useState<AlertConfig[]>([])
  const [history, setHistory] = useState<AlertMessage[]>([])
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [testing, setTesting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [success, setSuccess] = useState<string | null>(null)

  useEffect(() => {
    Promise.all([api.getAlertsConfig(), api.getAlertsHistory()])
      .then(([cfg, hist]) => {
        setChannels(cfg)
        setHistory(hist)
      })
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const updateChannel = useCallback((index: number, patch: Partial<AlertConfig>) => {
    setChannels((prev) => prev.map((c, i) => (i === index ? { ...c, ...patch } : c)))
  }, [])

  const addChannel = useCallback(() => {
    setChannels((prev) => [...prev, emptyChannel()])
  }, [])

  const deleteChannel = useCallback((index: number) => {
    setChannels((prev) => prev.filter((_, i) => i !== index))
  }, [])

  const toggleEvent = useCallback((index: number, event: AlertEventType) => {
    setChannels((prev) =>
      prev.map((c, i) => {
        if (i !== index) return c
        const events = c.events.includes(event)
          ? c.events.filter((e) => e !== event)
          : [...c.events, event]
        return { ...c, events }
      })
    )
  }, [])

  const handleSave = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      const updated = await api.updateAlertsConfig(channels)
      setChannels(updated)
      setSuccess('Alert config saved')
      setTimeout(() => setSuccess(null), 3000)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Save failed')
    } finally {
      setSaving(false)
    }
  }, [channels])

  const handleTest = useCallback(async () => {
    setTesting(true)
    setError(null)
    try {
      await api.testAlert()
      setSuccess('Test alert sent')
      setTimeout(() => setSuccess(null), 3000)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Test failed')
    } finally {
      setTesting(false)
    }
  }, [])

  const fmtTime = (ts: number) =>
    new Intl.DateTimeFormat('en-US', {
      hour: '2-digit',
      minute: '2-digit',
      second: '2-digit',
      month: 'short',
      day: 'numeric',
    }).format(new Date(ts))

  if (loading) {
    return (
      <div className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6" dir={rtl ? 'rtl' : 'ltr'}>
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  return (
    <div className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-6 text-gray-200" dir={rtl ? 'rtl' : 'ltr'}>
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">Alert Channels</h2>
        <div className="flex items-center gap-2">
          <button
            onClick={handleTest}
            disabled={testing || channels.length === 0}
            className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-4 py-2 text-xs font-medium hover:bg-white/20 transition-all disabled:opacity-50"
          >
            {testing && <span className="w-3 h-3 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />}
            Test Alert
          </button>
          <button onClick={addChannel} className="bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-4 py-2 text-xs font-semibold hover:bg-cyan-500/30 transition-all">
            + Add Channel
          </button>
        </div>
      </div>

      {(error || success) && (
        <div className={`text-sm px-4 py-2 rounded-lg ${
          error ? 'bg-red-500/10 border border-red-500/30 text-red-300' : 'bg-green-500/10 border border-green-500/30 text-green-300'
        }`}>
          {error || success}
        </div>
      )}

      {channels.length === 0 && (
        <div className="text-sm text-gray-500 italic text-center py-8">No alert channels configured yet</div>
      )}

      {/* Channel Cards */}
      <div className="space-y-4">
        {channels.map((ch, i) => (
          <div key={i} className="bg-black/30 border border-white/10 rounded-xl p-4 space-y-3">
            <div className="flex items-center justify-between">
              <select
                value={ch.channel}
                onChange={(e) => updateChannel(i, { channel: e.target.value as AlertChannel })}
                className="bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
              >
                {CHANNEL_TYPES.map((ct) => (
                  <option key={ct} value={ct}>{ct}</option>
                ))}
              </select>
              <div className="flex items-center gap-3">
                <button
                  onClick={() => updateChannel(i, { enabled: !ch.enabled })}
                  className={`relative w-9 h-5 rounded-full transition-colors ${ch.enabled ? 'bg-cyan-500' : 'bg-white/20'}`}
                >
                  <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${ch.enabled ? 'translate-x-4' : 'translate-x-0.5'}`} />
                </button>
                <button onClick={() => deleteChannel(i)} className="text-red-400 hover:text-red-300 text-lg leading-none">&times;</button>
              </div>
            </div>

            <input
              type={ch.channel === 'Telegram' ? 'password' : 'text'}
              value={ch.webhook_url}
              onChange={(e) => updateChannel(i, { webhook_url: e.target.value })}
              placeholder={ch.channel === 'Telegram' ? 'Bot token...' : ch.channel === 'Discord' ? 'Webhook URL...' : 'URL...'}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
            />

            {/* Event Grid */}
            <div className="grid grid-cols-3 gap-1.5">
              {EVENT_TYPES.map((ev) => (
                <label key={ev} className={`flex items-center gap-1.5 px-2 py-1.5 rounded-lg text-[11px] cursor-pointer transition-all ${
                  ch.events.includes(ev) ? 'bg-cyan-500/20 border border-cyan-500/30 text-cyan-200' : 'bg-white/[0.03] border border-white/10 text-gray-400 hover:bg-white/10'
                }`}>
                  <input
                    type="checkbox"
                    checked={ch.events.includes(ev)}
                    onChange={() => toggleEvent(i, ev)}
                    className="sr-only"
                  />
                  <span className={`w-1.5 h-1.5 rounded-full ${ch.events.includes(ev) ? 'bg-cyan-400' : 'bg-white/20'}`} />
                  {ev.replace(/([A-Z])/g, ' $1').trim()}
                </label>
              ))}
            </div>

            <div>
              <label className="text-[11px] text-gray-500">Min Profit USD: ${ch.min_profit_usd}</label>
              <input
                type="range"
                min={0}
                max={1000}
                step={5}
                value={ch.min_profit_usd}
                onChange={(e) => updateChannel(i, { min_profit_usd: parseFloat(e.target.value) || 0 })}
                className="w-full h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
              />
            </div>
          </div>
        ))}
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

      {/* Alert History */}
      <div className="border-t border-white/10 pt-4">
        <h3 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-3">
          Alert History ({history.length})
        </h3>
        <div className="max-h-48 overflow-y-auto rounded-xl bg-black/30 border border-white/10">
          {history.length === 0 ? (
            <div className="text-xs text-gray-500 italic p-4">No alerts sent yet</div>
          ) : (
            <table className="w-full text-xs">
              <thead>
                <tr className="border-b border-white/10 text-gray-500">
                  <th className="text-left p-2">Time</th>
                  <th className="text-left p-2">Event</th>
                  <th className="text-left p-2">Title</th>
                  <th className="text-center p-2">Status</th>
                </tr>
              </thead>
              <tbody>
                {history.slice().reverse().map((msg, idx) => (
                  <tr key={idx} className="border-b border-white/5 hover:bg-white/5">
                    <td className="p-2 text-gray-400 whitespace-nowrap">{fmtTime(msg.timestamp)}</td>
                    <td className="p-2 text-gray-300">{msg.event.replace(/([A-Z])/g, ' $1').trim()}</td>
                    <td className="p-2 text-gray-300 max-w-[120px] truncate">{msg.title}</td>
                    <td className="p-2 text-center">
                      <span className={`px-1.5 py-0.5 rounded-full text-[10px] font-semibold ${
                        msg.delivered ? 'bg-green-500/20 text-green-300' : 'bg-red-500/20 text-red-300'
                      }`}>
                        {msg.delivered ? 'Delivered' : 'Failed'}
                      </span>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          )}
        </div>
      </div>
    </div>
  )
}
