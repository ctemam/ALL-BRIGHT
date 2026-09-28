'use client'

import { useState, useEffect, useCallback } from 'react'
import type { ExecutionRule, RuleField, RuleOperator, RuleAction, RuleEvaluationResult } from '@/lib/api'
import { api } from '@/lib/api'

interface Props {
  rtl?: boolean
  opportunityId?: string
}

const FIELDS: RuleField[] = [
  'SpreadPct', 'NetProfitUsd', 'ConfidenceScore', 'LiquidityUsd', 'GasCostUsd', 'ChainId', 'ArbitrageType', 'TokenSymbol',
]

const OPERATORS: RuleOperator[] = [
  'GreaterThan', 'LessThan', 'Equals', 'Between', 'Contains',
]

const ACTIONS: RuleAction[] = [
  'Execute', 'Skip', 'LogOnly', 'NotifyMe', 'AskApproval',
]

function newRule(): ExecutionRule {
  return {
    id: crypto.randomUUID?.() ?? Math.random().toString(36).slice(2),
    name: '',
    enabled: true,
    field: 'SpreadPct',
    operator: 'GreaterThan',
    value: '',
    action: 'Skip',
    priority: 0,
  }
}

export default function RulesBuilder({ rtl, opportunityId }: Props) {
  const [rules, setRules] = useState<ExecutionRule[]>([])
  const [evaluation, setEvaluation] = useState<RuleEvaluationResult[] | null>(null)
  const [loading, setLoading] = useState(true)
  const [saving, setSaving] = useState(false)
  const [evaluating, setEvaluating] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    api.getRules()
      .then(setRules)
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const updateRule = useCallback((index: number, patch: Partial<ExecutionRule>) => {
    setRules((prev) => prev.map((r, i) => (i === index ? { ...r, ...patch } : r)))
  }, [])

  const addRule = useCallback(() => {
    setRules((prev) => [...prev, newRule()])
  }, [])

  const deleteRule = useCallback((index: number) => {
    setRules((prev) => prev.filter((_, i) => i !== index))
  }, [])

  const handleSave = useCallback(async () => {
    setSaving(true)
    setError(null)
    try {
      const updated = await api.updateRules(rules)
      setRules(updated)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Save failed')
    } finally {
      setSaving(false)
    }
  }, [rules])

  const handleEvaluate = useCallback(async () => {
    if (!opportunityId) return
    setEvaluating(true)
    setError(null)
    try {
      const dummy: import('@/lib/api').OpportunityDetail = {
        id: opportunityId,
        token: '',
        token_address: '',
        arbitrage_type: 'Simple',
        chain_name: '',
        chain_id: 1,
        buy_dex: null,
        sell_dex: null,
        buy_price: 0,
        sell_price: 0,
        spread_pct: 0,
        profit_breakdown: {
          gross_profit_usd: 0,
          costs: { gas_estimated_usd: 0, flash_loan_fee_usd: 0, slippage_estimated_usd: 0, bridge_fee_usd: null, velora_fee_usd: 0, total_cost_usd: 0 },
          net_profit_usd: 0,
          net_profit_pct: 0,
          roi_pct: 0,
          is_profitable: false,
        },
        flash_loan_recommendation: null,
        execution_steps: [],
        confidence_score: 0,
        liquidity_usd: 0,
        timestamp: Date.now(),
      }
      const res = await api.evaluateRules(dummy)
      setEvaluation(res.results)
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Evaluation failed')
    } finally {
      setEvaluating(false)
    }
  }, [opportunityId])

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
        <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">Execution Rules</h2>
        <button onClick={addRule} className="bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-4 py-2 text-xs font-semibold hover:bg-cyan-500/30 transition-all">
          + Add Rule
        </button>
      </div>

      {error && (
        <div className="text-sm px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300">{error}</div>
      )}

      {rules.length === 0 && (
        <div className="text-sm text-gray-500 italic text-center py-8">No rules defined yet</div>
      )}

      <div className="space-y-3">
        {rules.map((rule, i) => (
          <div key={rule.id ?? i} className="bg-black/30 border border-white/10 rounded-xl p-4 space-y-3">
            <div className="flex items-center justify-between gap-3">
              <input
                type="text"
                value={rule.name}
                onChange={(e) => updateRule(i, { name: e.target.value })}
                placeholder="Rule name..."
                className="flex-1 bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
              />
              <div className="flex items-center gap-3">
                <span className="text-[11px] text-gray-500">Priority:</span>
                <input
                  type="number"
                  value={rule.priority}
                  min={0}
                  max={999}
                  onChange={(e) => updateRule(i, { priority: parseInt(e.target.value) || 0 })}
                  className="w-16 bg-white/5 border border-white/10 rounded-lg px-2 py-1.5 text-sm text-center focus:outline-none focus:border-cyan-400/50 transition-colors"
                />
                <button
                  onClick={() => updateRule(i, { enabled: !rule.enabled })}
                  className={`relative w-9 h-5 rounded-full transition-colors ${rule.enabled ? 'bg-cyan-500' : 'bg-white/20'}`}
                >
                  <span className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${rule.enabled ? 'translate-x-4' : 'translate-x-0.5'}`} />
                </button>
                <button onClick={() => deleteRule(i)} className="text-red-400 hover:text-red-300 text-lg leading-none">&times;</button>
              </div>
            </div>

            <div className="grid grid-cols-2 sm:grid-cols-4 gap-3">
              <select
                value={rule.field}
                onChange={(e) => updateRule(i, { field: e.target.value as RuleField })}
                className="bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
              >
                {FIELDS.map((f) => (
                  <option key={f} value={f}>{f.replace(/([A-Z])/g, ' $1').trim()}</option>
                ))}
              </select>

              <select
                value={rule.operator}
                onChange={(e) => updateRule(i, { operator: e.target.value as RuleOperator })}
                className="bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
              >
                {OPERATORS.map((o) => (
                  <option key={o} value={o}>{o.replace(/([A-Z])/g, ' $1').trim()}</option>
                ))}
              </select>

              <input
                type="text"
                value={rule.value}
                onChange={(e) => updateRule(i, { value: e.target.value })}
                placeholder={rule.operator === 'Between' ? 'low,high' : 'value'}
                className="bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors"
              />

              <select
                value={rule.action}
                onChange={(e) => updateRule(i, { action: e.target.value as RuleAction })}
                className="bg-white/5 border border-white/10 rounded-lg px-3 py-1.5 text-sm focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
              >
                {ACTIONS.map((a) => (
                  <option key={a} value={a}>{a.replace(/([A-Z])/g, ' $1').trim()}</option>
                ))}
              </select>
            </div>
          </div>
        ))}
      </div>

      <div className="flex items-center gap-3 pt-2 border-t border-white/10">
        <button
          onClick={handleSave}
          disabled={saving}
          className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-5 py-2.5 text-sm font-medium hover:bg-white/20 transition-all disabled:opacity-50"
        >
          {saving && <span className="w-4 h-4 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />}
          {saving ? 'Saving...' : 'Save Rules'}
        </button>

        {opportunityId && (
          <button
            onClick={handleEvaluate}
            disabled={evaluating || rules.length === 0}
            className="flex items-center gap-2 bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-5 py-2.5 text-sm font-semibold hover:bg-cyan-500/30 transition-all disabled:opacity-50"
          >
            {evaluating && <span className="w-4 h-4 border-2 border-cyan-300 border-t-transparent rounded-full animate-spin" />}
            {evaluating ? 'Evaluating...' : 'Evaluate on Current Opportunity'}
          </button>
        )}
      </div>

      {/* Evaluation Results */}
      {evaluation && (
        <div className="border-t border-white/10 pt-4">
          <h3 className="text-sm font-semibold uppercase tracking-widest text-gray-400 mb-3">Evaluation Results</h3>
          <div className="space-y-2">
            {evaluation.map((res, idx) => (
              <div
                key={idx}
                className={`flex items-center justify-between rounded-xl px-4 py-2.5 border text-sm ${
                  res.matched
                    ? 'bg-green-500/10 border-green-500/30'
                    : 'bg-white/[0.03] border-white/10'
                }`}
              >
                <div className="flex items-center gap-3">
                  <span className={`w-2 h-2 rounded-full ${res.matched ? 'bg-green-400' : 'bg-gray-500'}`} />
                  <span className="text-gray-300">{res.rule_name}</span>
                  <span className="text-[11px] text-gray-500">{res.action}</span>
                </div>
                <span className="text-[11px] text-gray-400 max-w-[200px] truncate">{res.reason}</span>
              </div>
            ))}
          </div>
        </div>
      )}
    </div>
  )
}
