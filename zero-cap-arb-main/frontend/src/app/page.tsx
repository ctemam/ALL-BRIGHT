'use client';

import { useState, useCallback } from 'react';
import { WalletConnect } from '@/components/WalletConnect';
import { RadarDashboard } from '@/components/RadarDashboard';
import { TransactionLog } from '@/components/TransactionLog';
import CryptoBubbleChart from '@/components/CryptoBubbleChart';
import LiquidityMap from '@/components/LiquidityMap';
import BotPanel from '@/components/BotPanel';
import LLMConfig from '@/components/LLMConfig';
import OpportunityCard from '@/components/OpportunityCard';
import { api, DashboardData } from '@/lib/api';
import { useQuery } from '@tanstack/react-query';

type Tab = 'dashboard' | 'radar' | 'bot' | 'llm' | 'liquidity' | 'bubbles';

const TABS: { key: Tab; label_en: string; label_ar: string; icon: string }[] = [
  { key: 'dashboard', label_en: 'Dashboard', label_ar: 'لوحة التحكم', icon: 'M3 12l2-2m0 0l7-7 7 7M5 10v10a1 1 0 001 1h3m10-11l2 2m-2-2v10a1 1 0 01-1 1h-3m-6 0a1 1 0 001-1v-4a1 1 0 011-1h2a1 1 0 011 1v4a1 1 0 001 1m-6 0h6' },
  { key: 'radar', label_en: 'Radar', label_ar: 'رادار', icon: 'M13 10V3L4 14h7v7l9-11h-7z' },
  { key: 'bot', label_en: 'Bot', label_ar: 'البوت', icon: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z' },
  { key: 'llm', label_en: 'AI Advisor', label_ar: 'المستشار الذكي', icon: 'M9.75 17L9 20l-1 1h8l-1-1-.75-3M3 13h18M5 17h14a2 2 0 002-2V5a2 2 0 00-2-2H5a2 2 0 00-2 2v10a2 2 0 002 2z' },
  { key: 'liquidity', label_en: 'Liquidity', label_ar: 'السيولة', icon: 'M19.428 15.428a2 2 0 00-1.022-.547l-2.387-.477a6 6 0 00-3.86.517l-.318.158a6 6 0 01-3.86.517L6.05 15.21a2 2 0 00-1.806.547M8 4h8l-1 1v5.172a2 2 0 00.586 1.414l5 5c1.26 1.26.367 3.414-1.415 3.414H4.828c-1.782 0-2.674-2.154-1.414-3.414l5-5A2 2 0 009 10.172V5L8 4z' },
  { key: 'bubbles', label_en: 'Bubbles', label_ar: 'الفقاعات', icon: 'M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z' },
];

export default function Home() {
  const [rtl, setRtl] = useState(false);
  const [activeTab, setActiveTab] = useState<Tab>('dashboard');
  const [selectedOppId, setSelectedOppId] = useState<string | null>(null);

  const toggleRtl = useCallback(() => {
    const next = !rtl;
    setRtl(next);
    document.documentElement.dir = next ? 'rtl' : 'ltr';
  }, [rtl]);

  const { data: dashboard } = useQuery({
    queryKey: ['dashboard'],
    queryFn: () => api.getDashboard(),
    refetchInterval: 15_000,
  });

  const t = (en: string, ar: string) => rtl ? ar : en;

  return (
    <div className="min-h-screen bg-[#0f0f13]" dir={rtl ? 'rtl' : 'ltr'}>
      <div className="fixed inset-0 overflow-hidden pointer-events-none">
        <div className="absolute -top-40 -right-40 w-96 h-96 bg-indigo-500/10 rounded-full blur-3xl" />
        <div className="absolute -bottom-40 -left-40 w-96 h-96 bg-purple-500/10 rounded-full blur-3xl" />
        <div className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 w-[800px] h-[800px] bg-indigo-600/5 rounded-full blur-3xl" />
      </div>

      <div className="relative z-10">
        <header className="sticky top-0 z-50 glass border-b border-white/5">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="flex items-center justify-between h-16">
              <div className="flex items-center gap-3">
                <div className="w-8 h-8 rounded-lg bg-gradient-to-br from-indigo-500 to-purple-600 flex items-center justify-center">
                  <svg className="w-4 h-4 text-white" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 10V3L4 14h7v7l9-11h-7z" />
                  </svg>
                </div>
                <div>
                  <h1 className="text-sm font-bold text-white tracking-tight">
                    {t('Zero-Cap Arbitrage', 'صفقة رأس مال صفر')}
                  </h1>
                  <p className="text-[10px] text-gray-500 -mt-0.5">
                    {t('0 Capital \u2022 0 Upfront Gas \u2022 Ultra-Low Latency', '0 رأس مال \u2022 0 غاز مقدم \u2022 زمن استجابة فائق السرعة')}
                  </p>
                </div>
              </div>

              <div className="flex items-center gap-4">
                <div className="hidden sm:flex items-center gap-3 text-xs text-gray-500">
                  <span className="flex items-center gap-1">
                    <span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />
                    {t('6 Chains', '6 سلاسل')}
                  </span>
                  <span className="flex items-center gap-1">
                    <span className="w-1.5 h-1.5 rounded-full bg-blue-500 animate-pulse" />
                    {t('50+ DEXes', '50+ تبادل')}
                  </span>
                </div>
                <button
                  onClick={toggleRtl}
                  className="px-2 py-1 text-xs font-medium text-gray-400 hover:text-white border border-white/10 rounded-lg hover:border-white/20 transition-colors"
                >
                  {rtl ? 'EN' : 'عربي'}
                </button>
                <WalletConnect />
              </div>
            </div>

            {/* Tab Navigation */}
            <nav className="flex gap-1 pb-2 overflow-x-auto scrollbar-none">
              {TABS.map((tab) => (
                <button
                  key={tab.key}
                  onClick={() => setActiveTab(tab.key)}
                  className={`flex items-center gap-2 px-4 py-2 text-xs font-medium rounded-lg transition-all whitespace-nowrap ${
                    activeTab === tab.key
                      ? 'bg-indigo-500/20 text-indigo-300 border border-indigo-500/30'
                      : 'text-gray-400 hover:text-white hover:bg-white/5 border border-transparent'
                  }`}
                >
                  <svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d={tab.icon} />
                  </svg>
                  {t(tab.label_en, tab.label_ar)}
                </button>
              ))}
            </nav>
          </div>
        </header>

        <main className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8 py-8">
          {/* ─── DASHBOARD TAB ─────────────────────────── */}
          {activeTab === 'dashboard' && (
            <div className="space-y-6">
              {/* Stats Bar */}
              <div className="grid grid-cols-2 sm:grid-cols-4 gap-4">
                <div className="glass-card p-4">
                  <p className="text-xs text-gray-400">{t('Total Opportunities', 'إجمالي الفرص')}</p>
                  <p className="text-2xl font-bold text-white mt-1">{dashboard?.total_opportunities_found ?? '—'}</p>
                </div>
                <div className="glass-card p-4">
                  <p className="text-xs text-gray-400">{t('24h Profit', 'ربح 24 ساعة')}</p>
                  <p className="text-2xl font-bold text-green-400 mt-1">
                    ${dashboard?.total_profit_24h_usd?.toFixed(2) ?? '—'}
                  </p>
                </div>
                <div className="glass-card p-4">
                  <p className="text-xs text-gray-400">{t('Tokens Tracked', 'رموز متتبعة')}</p>
                  <p className="text-2xl font-bold text-white mt-1">{dashboard?.bubbles?.length ?? '—'}</p>
                </div>
                <div className="glass-card p-4">
                  <p className="text-xs text-gray-400">{t('Total Liquidity', 'إجمالي السيولة')}</p>
                  <p className="text-2xl font-bold text-purple-400 mt-1">
                    ${dashboard ? (dashboard.liquidity_map.total_liquidity_usd / 1e6).toFixed(1) + 'M' : '—'}
                  </p>
                </div>
              </div>

              {/* Opportunities List */}
              <div>
                <h3 className="text-lg font-bold text-white mb-4">
                  {t('Live Opportunities', 'الفرص المباشرة')}
                </h3>
                <div className="grid gap-4 md:grid-cols-2">
                  {dashboard?.opportunities.map((opp) => (
                    <OpportunityCard
                      key={opp.id}
                      opportunity={opp}
                      rtl={rtl}
                      isSelected={selectedOppId === opp.id}
                      onSelect={setSelectedOppId}
                    />
                  ))}
                  {(!dashboard?.opportunities || dashboard.opportunities.length === 0) && (
                    <div className="glass-card p-8 text-center text-gray-500 col-span-2">
                      <p>{t('No opportunities found. Run a scan to discover arbitrage.', 'لم يتم العثور على فرص. قم بتشغيل المسح لاكتشاف المراجحة.')}</p>
                    </div>
                  )}
                </div>
              </div>

              {/* Mini Liquidity & Bubbles preview */}
              <div className="grid gap-6 lg:grid-cols-2">
                {dashboard && <LiquidityMap data={dashboard.liquidity_map} rtl={rtl} />}
                {dashboard && <CryptoBubbleChart data={dashboard.bubbles} rtl={rtl} />}
              </div>

              <TransactionLog />
            </div>
          )}

          {/* ─── RADAR TAB ────────────────────────────── */}
          {activeTab === 'radar' && (
            <div className="space-y-6">
              <RadarDashboard />
              {dashboard && (
                <div className="grid gap-4 md:grid-cols-2">
                  {dashboard.opportunities.map((opp) => (
                    <OpportunityCard
                      key={opp.id}
                      opportunity={opp}
                      rtl={rtl}
                      isSelected={selectedOppId === opp.id}
                      onSelect={setSelectedOppId}
                    />
                  ))}
                </div>
              )}
              <TransactionLog />
            </div>
          )}

          {/* ─── BOT TAB ──────────────────────────────── */}
          {activeTab === 'bot' && (
            <BotPanel rtl={rtl} />
          )}

          {/* ─── AI ADVISOR TAB ───────────────────────── */}
          {activeTab === 'llm' && (
            <LLMConfig
              rtl={rtl}
              selectedOpportunity={
                selectedOppId && dashboard
                  ? dashboard.opportunities.find((o) => o.id === selectedOppId) ?? null
                  : null
              }
            />
          )}

          {/* ─── LIQUIDITY TAB ────────────────────────── */}
          {activeTab === 'liquidity' && (
            <div>
              {dashboard && <LiquidityMap data={dashboard.liquidity_map} rtl={rtl} />}
            </div>
          )}

          {/* ─── BUBBLES TAB ──────────────────────────── */}
          {activeTab === 'bubbles' && (
            <div>
              {dashboard && <CryptoBubbleChart data={dashboard.bubbles} rtl={rtl} />}
            </div>
          )}
        </main>

        <footer className="border-t border-white/5 py-6 mt-12">
          <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
            <div className="flex items-center justify-between text-xs text-gray-600">
              <p>Zero-Cap Arbitrage v0.1.0 &bull; {t('Not financial advice', 'ليست نصيحة مالية')}</p>
              <div className="flex items-center gap-4">
                <a href="https://paraswap.io" target="_blank" rel="noopener noreferrer" className="hover:text-gray-400 transition-colors">
                  Powered by Velora (ex-ParaSwap)
                </a>
                <span>|</span>
                <a href="https://flashbots.net" target="_blank" rel="noopener noreferrer" className="hover:text-gray-400 transition-colors">
                  Flashbots
                </a>
                <span>|</span>
                <a href="https://pimlico.io" target="_blank" rel="noopener noreferrer" className="hover:text-gray-400 transition-colors">
                  Pimlico
                </a>
              </div>
            </div>
          </div>
        </footer>
      </div>
    </div>
  );
}
