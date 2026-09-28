'use client'

import { useState, useEffect } from 'react'
import type {
  LLMConfig as LLMConfigType,
  LLMAdviceRequest,
  LLMAdviceResponse,
  OpportunityDetail,
} from '@/lib/api'
import { api } from '@/lib/api'

const PROVIDERS = [
  { value: 'OpenAI', models: ['gpt-4o', 'gpt-4o-mini', 'gpt-4-turbo'] },
  { value: 'Anthropic', models: ['claude-3-5-sonnet-20241022', 'claude-3-opus-20240229', 'claude-3-haiku-20240307'] },
  { value: 'Groq', models: ['llama-3.1-70b-versatile', 'llama-3.1-8b-instant', 'mixtral-8x7b-32768'] },
  { value: 'Ollama (Local)', models: ['llama3.2', 'llama3.1', 'mistral'] },
  { value: 'DeepSeek', models: ['deepseek-chat', 'deepseek-reasoner'] },
  { value: 'Custom', models: ['custom-model'] },
]

const DEFAULT_SYSTEM_PROMPT =
  "You are an expert DeFi arbitrage analyst with deep knowledge of cross-chain DEX mechanics, flash loans, MEV strategies, and risk assessment. Analyze the given arbitrage opportunity and provide clear, actionable advice."

function getDefaultModel(provider: string): string {
  return PROVIDERS.find((p) => p.value === provider)?.models[0] ?? 'gpt-4o'
}

function getModelPlaceholder(provider: string): string {
  return `e.g., ${getDefaultModel(provider)}`
}

interface Props {
  rtl?: boolean
  selectedOpportunity?: OpportunityDetail | null
}

export default function LLMConfig({ rtl, selectedOpportunity }: Props) {
  const [configured, setConfigured] = useState(false)
  const [activeConfig, setActiveConfig] = useState<LLMConfigType | null>(null)
  const [loading, setLoading] = useState(true)

  const [provider, setProvider] = useState('OpenAI')
  const [apiKey, setApiKey] = useState('')
  const [model, setModel] = useState(getDefaultModel('OpenAI'))
  const [temperature, setTemperature] = useState(0.3)
  const [maxTokens, setMaxTokens] = useState(2048)
  const [systemPrompt, setSystemPrompt] = useState(DEFAULT_SYSTEM_PROMPT)
  const [saving, setSaving] = useState(false)
  const [saveMsg, setSaveMsg] = useState<string | null>(null)

  const [enabled, setEnabled] = useState(true)
  const [testing, setTesting] = useState(false)
  const [testMsg, setTestMsg] = useState<string | null>(null)

  const [question, setQuestion] = useState('')
  const [advising, setAdvising] = useState(false)
  const [advice, setAdvice] = useState<LLMAdviceResponse | null>(null)
  const [adviceError, setAdviceError] = useState<string | null>(null)

  const [configOpen, setConfigOpen] = useState(true)
  const [adviceOpen, setAdviceOpen] = useState(true)

  useEffect(() => {
    api
      .getLlmConfig()
      .then((res) => {
        if ('configured' in res && res.configured === false) {
          setConfigured(false)
        } else {
          const cfg = res as LLMConfigType
          setConfigured(true)
          setActiveConfig(cfg)
          setProvider(cfg.provider)
          setModel(cfg.model)
          setTemperature(cfg.temperature)
          setMaxTokens(cfg.max_tokens)
          setSystemPrompt(cfg.system_prompt)
        }
      })
      .catch(() => {})
      .finally(() => setLoading(false))
  }, [])

  const handleSave = async () => {
    setSaving(true)
    setSaveMsg(null)
    try {
      const payload: LLMConfigType = {
        provider,
        api_key: apiKey,
        model,
        temperature,
        max_tokens: maxTokens,
        system_prompt: systemPrompt,
      }
      await api.updateLlmConfig(payload)
      setConfigured(true)
      setActiveConfig(payload)
      setSaveMsg('AI Advisor configured successfully')
      setTimeout(() => setSaveMsg(null), 3000)
    } catch (err) {
      setSaveMsg(err instanceof Error ? err.message : 'Failed to save config')
    } finally {
      setSaving(false)
    }
  }

  const handleTestConnection = async () => {
    setTesting(true)
    setTestMsg(null)
    try {
      const payload: LLMConfigType = {
        provider: activeConfig!.provider,
        api_key: activeConfig!.api_key,
        model: activeConfig!.model,
        temperature: activeConfig!.temperature,
        max_tokens: activeConfig!.max_tokens,
        system_prompt: activeConfig!.system_prompt,
      }
      await api.updateLlmConfig(payload)
      setTestMsg('Connection successful')
    } catch (err) {
      setTestMsg(err instanceof Error ? err.message : 'Connection failed')
    } finally {
      setTesting(false)
      setTimeout(() => setTestMsg(null), 3000)
    }
  }

  const handleGetAdvice = async () => {
    if (!selectedOpportunity) return
    setAdvising(true)
    setAdvice(null)
    setAdviceError(null)
    try {
      const req: LLMAdviceRequest = {
        opportunity: selectedOpportunity,
        market_context: '',
        user_question: question || 'Should I execute this trade?',
      }
      const res = await api.getLlmAdvice(req)
      setAdvice(res)
    } catch (err) {
      setAdviceError(err instanceof Error ? err.message : 'Failed to get advice')
    } finally {
      setAdvising(false)
    }
  }

  const handleProviderChange = (val: string) => {
    setProvider(val)
    setModel(getDefaultModel(val))
  }

  if (loading) {
    return (
      <div
        className="flex items-center justify-center h-64 bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6"
        dir={rtl ? 'rtl' : 'ltr'}
      >
        <div className="w-8 h-8 border-2 border-cyan-400 border-t-transparent rounded-full animate-spin" />
      </div>
    )
  }

  return (
    <div
      className="bg-white/5 backdrop-blur-xl border border-white/10 rounded-2xl p-6 space-y-6 text-gray-200"
      dir={rtl ? 'rtl' : 'ltr'}
    >
      {!configured ? (
        // ── Not Configured: Configuration Form ──
        <div className="space-y-5">
          <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">
            Configure AI Advisor
          </h2>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Provider</label>
            <select
              value={provider}
              onChange={(e) => handleProviderChange(e.target.value)}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
            >
              {PROVIDERS.map((p) => (
                <option key={p.value} value={p.value}>
                  {p.value}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">API Key</label>
            <input
              type="password"
              value={apiKey}
              onChange={(e) => setApiKey(e.target.value)}
              placeholder="sk-..."
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
            />
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Model Name</label>
            <input
              type="text"
              value={model}
              onChange={(e) => setModel(e.target.value)}
              placeholder={getModelPlaceholder(provider)}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
            />
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">
              Temperature ({temperature.toFixed(1)})
            </label>
            <input
              type="range"
              min={0}
              max={1}
              step={0.1}
              value={temperature}
              onChange={(e) => setTemperature(parseFloat(e.target.value))}
              className="w-full h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
            />
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">Max Tokens</label>
            <input
              type="number"
              value={maxTokens}
              min={256}
              max={16384}
              step={256}
              onChange={(e) => setMaxTokens(parseInt(e.target.value) || 2048)}
              className="w-28 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
            />
          </div>

          <div>
            <label className="text-xs text-gray-400 mb-1.5 block">System Prompt</label>
            <textarea
              value={systemPrompt}
              onChange={(e) => setSystemPrompt(e.target.value)}
              rows={4}
              className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors resize-y"
            />
          </div>

          <button
            onClick={handleSave}
            disabled={saving || !apiKey}
            className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-6 py-2.5 text-sm font-medium hover:bg-white/20 transition-all disabled:opacity-50"
          >
            {saving && (
              <span className="w-4 h-4 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />
            )}
            Save
          </button>

          {saveMsg && (
            <div
              className={`text-sm px-4 py-2 rounded-lg ${
                saveMsg.includes('successfully')
                  ? 'bg-green-500/10 border border-green-500/30 text-green-300'
                  : 'bg-red-500/10 border border-red-500/30 text-red-300'
              }`}
            >
              {saveMsg}
            </div>
          )}
        </div>
      ) : (
        // ── Configured: Active Panel ──
        <div className="space-y-5">
          <div className="flex items-center justify-between">
            <div>
              <h2 className="text-sm font-semibold uppercase tracking-widest text-gray-400">
                AI Advisor Active
              </h2>
              <p className="text-xs text-gray-500 mt-0.5">
                {activeConfig?.provider} &middot; {activeConfig?.model}
              </p>
            </div>
            <div className="flex items-center gap-3">
              <button
                onClick={() => setEnabled((e) => !e)}
                className={`relative w-10 h-5 rounded-full transition-colors ${
                  enabled ? 'bg-cyan-500' : 'bg-white/20'
                }`}
              >
                <span
                  className={`absolute top-0.5 w-4 h-4 bg-white rounded-full shadow transition-transform ${
                    enabled ? 'translate-x-5' : 'translate-x-0.5'
                  }`}
                />
              </button>
              <button
                onClick={handleTestConnection}
                disabled={testing}
                className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-4 py-2 text-xs font-medium hover:bg-white/20 transition-all disabled:opacity-50"
              >
                {testing && (
                  <span className="w-3 h-3 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />
                )}
                Test Connection
              </button>
            </div>
          </div>

          {testMsg && (
            <div
              className={`text-sm px-4 py-2 rounded-lg ${
                testMsg.includes('successful')
                  ? 'bg-green-500/10 border border-green-500/30 text-green-300'
                  : 'bg-red-500/10 border border-red-500/30 text-red-300'
              }`}
            >
              {testMsg}
            </div>
          )}

          {/* Re‑configure collapsible */}
          <div>
            <button
              onClick={() => setConfigOpen((o) => !o)}
              className="flex items-center gap-2 text-xs text-gray-400 hover:text-gray-200 transition-colors"
            >
              <span className={`transition-transform duration-200 ${configOpen ? 'rotate-90' : ''}`}>▶</span>
              Re-configure
            </button>
            {configOpen && (
              <div className="mt-3 space-y-4">
                <div>
                  <label className="text-xs text-gray-400 mb-1.5 block">Provider</label>
                  <select
                    value={provider}
                    onChange={(e) => handleProviderChange(e.target.value)}
                    className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors appearance-none"
                  >
                    {PROVIDERS.map((p) => (
                      <option key={p.value} value={p.value}>
                        {p.value}
                      </option>
                    ))}
                  </select>
                </div>

                <div>
                  <label className="text-xs text-gray-400 mb-1.5 block">API Key</label>
                  <input
                    type="password"
                    value={apiKey}
                    onChange={(e) => setApiKey(e.target.value)}
                    placeholder="sk-..."
                    className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
                  />
                </div>

                <div>
                  <label className="text-xs text-gray-400 mb-1.5 block">Model Name</label>
                  <input
                    type="text"
                    value={model}
                    onChange={(e) => setModel(e.target.value)}
                    placeholder={getModelPlaceholder(provider)}
                    className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
                  />
                </div>

                <div>
                  <label className="text-xs text-gray-400 mb-1.5 block">
                    Temperature ({temperature.toFixed(1)})
                  </label>
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step={0.1}
                    value={temperature}
                    onChange={(e) => setTemperature(parseFloat(e.target.value))}
                    className="w-full h-1.5 bg-white/10 rounded-full appearance-none cursor-pointer accent-cyan-400"
                  />
                </div>

                <div>
                  <label className="text-xs text-gray-400 mb-1.5 block">Max Tokens</label>
                  <input
                    type="number"
                    value={maxTokens}
                    min={256}
                    max={16384}
                    step={256}
                    onChange={(e) => setMaxTokens(parseInt(e.target.value) || 2048)}
                    className="w-28 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
                  />
                </div>

                <div>
                  <label className="text-xs text-gray-400 mb-1.5 block">System Prompt</label>
                  <textarea
                    value={systemPrompt}
                    onChange={(e) => setSystemPrompt(e.target.value)}
                    rows={4}
                    className="w-full bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors resize-y"
                  />
                </div>

                <button
                  onClick={handleSave}
                  disabled={saving || !apiKey}
                  className="flex items-center gap-2 bg-white/10 border border-white/20 rounded-xl px-6 py-2.5 text-sm font-medium hover:bg-white/20 transition-all disabled:opacity-50"
                >
                  {saving && (
                    <span className="w-4 h-4 border-2 border-gray-300 border-t-transparent rounded-full animate-spin" />
                  )}
                  Update
                </button>

                {saveMsg && (
                  <div
                    className={`text-sm px-4 py-2 rounded-lg ${
                      saveMsg.includes('successfully')
                        ? 'bg-green-500/10 border border-green-500/30 text-green-300'
                        : 'bg-red-500/10 border border-red-500/30 text-red-300'
                    }`}
                  >
                    {saveMsg}
                  </div>
                )}
              </div>
            )}
          </div>

          {/* AI Advice Section */}
          {selectedOpportunity && (
            <div className="border-t border-white/10 pt-5">
              <button
                onClick={() => setAdviceOpen((o) => !o)}
                className="flex items-center gap-2 text-sm font-semibold text-gray-300 hover:text-gray-200 transition-colors"
              >
                <span className={`transition-transform duration-200 ${adviceOpen ? 'rotate-90' : ''}`}>▶</span>
                Get AI Advice
              </button>

              {adviceOpen && (
                <div className="mt-4 space-y-4">
                  <div className="flex items-center gap-3">
                    <input
                      type="text"
                      value={question}
                      onChange={(e) => setQuestion(e.target.value)}
                      placeholder="Should I execute this trade?"
                      className="flex-1 bg-white/5 border border-white/10 rounded-lg px-3 py-2 text-sm text-gray-200 focus:outline-none focus:border-cyan-400/50 transition-colors"
                    />
                    <button
                      onClick={handleGetAdvice}
                      disabled={advising}
                      className="flex items-center gap-2 bg-cyan-500/20 border border-cyan-500/40 text-cyan-300 rounded-xl px-5 py-2 text-sm font-semibold hover:bg-cyan-500/30 transition-all disabled:opacity-50"
                    >
                      {advising && (
                        <span className="w-4 h-4 border-2 border-cyan-300 border-t-transparent rounded-full animate-spin" />
                      )}
                      {advising ? 'Thinking...' : 'Get Advice'}
                    </button>
                  </div>

                  {adviceError && (
                    <div className="text-sm px-4 py-2 rounded-lg bg-red-500/10 border border-red-500/30 text-red-300">
                      {adviceError}
                    </div>
                  )}

                  {advice && (
                    <div className="space-y-4">
                      {/* Advice Quote */}
                      <div className="bg-white/[0.03] border border-white/10 rounded-xl p-4 italic text-sm text-gray-300 leading-relaxed">
                        &ldquo;{advice.advice}&rdquo;
                      </div>

                      {/* Confidence */}
                      <div className="flex items-center gap-3">
                        <span className="text-xs text-gray-400">Confidence:</span>
                        <span
                          className={`text-xs font-semibold px-2.5 py-0.5 rounded-full ${
                            advice.confidence === 'high'
                              ? 'bg-green-500/20 text-green-300 border border-green-500/30'
                              : advice.confidence === 'yellow'
                              ? 'bg-yellow-500/20 text-yellow-300 border border-yellow-500/30'
                              : 'bg-red-500/20 text-red-300 border border-red-500/30'
                          }`}
                        >
                          {advice.confidence}
                        </span>
                      </div>

                      {/* Recommend Execute */}
                      <div className="flex items-center gap-2">
                        <span className="text-xs text-gray-400">Recommend Execute?</span>
                        {advice.recommend_execute ? (
                          <span className="flex items-center gap-1 text-xs font-semibold text-green-300">
                            <span className="w-2 h-2 rounded-full bg-green-400" />
                            Yes
                          </span>
                        ) : (
                          <span className="flex items-center gap-1 text-xs font-semibold text-red-300">
                            <span className="w-2 h-2 rounded-full bg-red-400" />
                            No
                          </span>
                        )}
                      </div>

                      {/* Reasoning */}
                      <div>
                        <span className="text-xs text-gray-400 mb-2 block">Reasoning</span>
                        <ul className="space-y-1">
                          {advice.reasoning.map((r, i) => (
                            <li key={i} className="flex items-start gap-2 text-xs text-gray-300">
                              <span className="text-cyan-400 mt-0.5">&#8226;</span>
                              {r}
                            </li>
                          ))}
                        </ul>
                      </div>

                      {/* Risk Factors */}
                      <div>
                        <span className="text-xs text-gray-400 mb-2 block">Risk Factors</span>
                        <ul className="space-y-1">
                          {advice.risk_factors.map((rf, i) => (
                            <li key={i} className="flex items-start gap-2 text-xs text-red-300/80">
                              <span className="text-red-400 mt-0.5">&#8226;</span>
                              {rf}
                            </li>
                          ))}
                        </ul>
                      </div>
                    </div>
                  )}
                </div>
              )}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
