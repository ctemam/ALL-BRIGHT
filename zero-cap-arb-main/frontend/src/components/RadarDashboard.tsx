'use client';

import { useState, useCallback, useRef } from 'react';
import { api, type ArbitrageOpportunity, type AllPricesResponse, type AllOpportunitiesResponse } from '@/lib/api';
import { OpportunityTable } from './OpportunityTable';
import { AllPricesTable } from './AllPricesTable';
import { AdvancedExecutionPanel } from './AdvancedExecutionPanel';
import { ExecutionPanel } from './ExecutionPanel';
import { RadarAnimation } from './RadarAnimation';

type ViewMode = 'opportunities' | 'all_prices' | 'advanced';

export function RadarDashboard() {
  const [token, setToken] = useState('');
  const [viewMode, setViewMode] = useState<ViewMode>('opportunities');
  const [scanning, setScanning] = useState(false);
  const [scanTime, setScanTime] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  // Simple opportunities
  const [opportunities, setOpportunities] = useState<ArbitrageOpportunity[]>([]);
  const [selectedOpp, setSelectedOpp] = useState<ArbitrageOpportunity | null>(null);

  // All prices
  const [allPrices, setAllPrices] = useState<AllPricesResponse | null>(null);

  // Advanced
  const [allOpps, setAllOpps] = useState<AllOpportunitiesResponse | null>(null);

  const wsRef = useRef<WebSocket | null>(null);

  // ─── Token Address Map ──────────────────────────────
  // Common token addresses on mainnet
  const tokenAddressMap: Record<string, string> = {
    DAI: '0x6B175474E89094C44Da98b954EedeAC495271d0F',
    USDC: '0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48',
    USDT: '0xdAC17F958D2ee523a2206206994597C13D831ec7',
    WETH: '0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2',
    WBTC: '0x2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599',
    LINK: '0x514910771AF9Ca656af840dff83E8264EcF986CA',
    UNI: '0x1f9840a85d5aF5bf1D1762F925BDADdC4201F984',
    AAVE: '0x7Fc66500c84A76Ad7e9c93437bFc5Ac33E2DDaE9',
    MATIC: '0x7D1AfA7B718fb893dB30A3aBc0Cfc608AaCfeBB0',
    CRV: '0xD533a949740bb3306d119CC777fa900bA034cd52',
  };

  const resolvedAddress = tokenAddressMap[token.toUpperCase()] ?? undefined;

  // ─── Scan Handlers ──────────────────────────────────

  const handleScan = useCallback(async () => {
    if (!token.trim()) return;
    setScanning(true);
    setError(null);

    try {
      if (viewMode === 'opportunities') {
        const result = await api.scanToken(token.trim(), resolvedAddress);
        setOpportunities(result.opportunities);
        setScanTime(result.scan_time_ms);
      } else if (viewMode === 'all_prices') {
        const result = await api.getAllPrices(token.trim(), resolvedAddress);
        setAllPrices(result);
        setScanTime(result.scan_time_ms);
      } else if (viewMode === 'advanced') {
        const result = await api.getAllOpportunities(token.trim(), resolvedAddress);
        setAllOpps(result);
        setScanTime(result.scan_time_ms);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Scan failed');
    } finally {
      setScanning(false);
    }
  }, [token, viewMode, resolvedAddress]);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => { if (e.key === 'Enter') handleScan(); }, [handleScan]);

  const connectWs = useCallback(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    const ws = new WebSocket(`${protocol}//localhost:3001/ws`);
    ws.onopen = () => { if (token.trim()) ws.send(JSON.stringify({ type: 'subscribe', token: token.trim() })); };
    ws.onmessage = (event) => {
      try {
        const msg = JSON.parse(event.data);
        if (msg.type === 'scan_result') {
          setOpportunities(msg.data.opportunities ?? []);
          setAllPrices(msg.data);
          setAllOpps(msg.data);
          setScanTime(msg.data.scan_time_ms);
        }
      } catch { /* ignore */ }
    };
    ws.onclose = () => { wsRef.current = null; };
    wsRef.current = ws;
  }, [token]);

  const tabs: { key: ViewMode; label: string; icon: string }[] = [
    { key: 'opportunities', label: 'Best Opportunities', icon: 'Z' },
    { key: 'all_prices', label: 'All Prices (Matrix)', icon: 'A' },
    { key: 'advanced', label: 'Advanced Engine', icon: 'X' },
  ];

  return (
    <div className="space-y-5">
      {/* Search Bar */}
      <div className="glass-card p-5">
        <div className="flex flex-col gap-3">
          {/* Search + Scan */}
          <div className="flex items-center gap-2">
            <div className="relative flex-1">
              <div className="absolute inset-y-0 left-3 flex items-center pointer-events-none">
                <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M21 21l-6-6m2-5a7 7 0 11-14 0 7 7 0 0114 0z" />
                </svg>
              </div>
              <input
                type="text"
                value={token}
                onChange={(e) => setToken(e.target.value.toUpperCase())}
                onKeyDown={handleKeyDown}
                placeholder="Search token symbol (e.g., LINK, UNI, DAI)..."
                className="w-full pl-9 pr-4 py-2.5 bg-white/5 border border-white/10 rounded-xl text-white placeholder-gray-500 
                           focus:outline-none focus:border-indigo-500/50 focus:ring-1 focus:ring-indigo-500/20 transition-all text-sm"
              />
            </div>
            <button
              onClick={handleScan}
              disabled={scanning || !token.trim()}
              className="px-5 py-2.5 bg-gradient-to-r from-indigo-600 to-purple-600 hover:from-indigo-500 hover:to-purple-500 
                         disabled:opacity-40 disabled:cursor-not-allowed rounded-xl font-semibold text-sm transition-all neo-glow flex items-center gap-2"
            >
              {scanning ? (
                <><svg className="animate-spin h-3.5 w-3.5" viewBox="0 0 24 24"><circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" fill="none" /><path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4z" /></svg>Scanning...</>
              ) : (
                <><svg className="w-3.5 h-3.5" fill="none" viewBox="0 0 24 24" stroke="currentColor"><path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" /></svg>Scan</>
              )}
            </button>
          </div>

          {/* Tab Bar */}
          <div className="flex items-center justify-between">
            <div className="flex items-center gap-1 bg-white/5 rounded-xl p-0.5">
              {tabs.map((tab) => (
                <button
                  key={tab.key}
                  onClick={() => { setViewMode(tab.key); setSelectedOpp(null); }}
                  className={`px-3 py-1.5 rounded-lg text-xs font-medium transition-all ${
                    viewMode === tab.key
                      ? 'bg-indigo-600/30 text-indigo-300 shadow-sm'
                      : 'text-gray-500 hover:text-gray-300'
                  }`}
                >
                  {tab.label}
                </button>
              ))}
            </div>

            <div className="flex items-center gap-3 text-[10px] text-gray-500">
              <span className="flex items-center gap-1"><span className="w-1.5 h-1.5 rounded-full bg-green-500 animate-pulse" />6 Chains</span>
              <span className="flex items-center gap-1"><span className="w-1.5 h-1.5 rounded-full bg-blue-500 animate-pulse" />50+ DEXes</span>
              {scanTime && <span className="text-indigo-400">{scanTime}ms</span>}
              <button onClick={connectWs} className="text-gray-400 hover:text-white underline underline-offset-2">Live</button>
            </div>
          </div>
        </div>
      </div>

      {/* Loading animation */}
      {scanning && (
        <div className="glass-card p-8 flex flex-col items-center justify-center gap-4">
          <RadarAnimation active />
          <p className="text-sm text-gray-400">
            {viewMode === 'all_prices' ? 'Fetching prices from all 55+ DEXes...' :
             viewMode === 'advanced' ? 'Analyzing all arbitrage strategies...' :
             'Scanning for best opportunities...'}
          </p>
        </div>
      )}

      {/* Error */}
      {error && !scanning && (
        <div className="glass-card p-3 border border-red-500/20">
          <p className="text-red-400 text-xs">{error}</p>
        </div>
      )}

      {/* View: Best Opportunities */}
      {viewMode === 'opportunities' && !scanning && (
        <>
          {opportunities.length > 0 ? (
            <OpportunityTable opportunities={opportunities} onSelect={setSelectedOpp} selectedId={selectedOpp?.id ?? null} />
          ) : !error ? (
            <div className="glass-card p-10 text-center">
              <div className="w-14 h-14 mx-auto mb-3 rounded-full bg-indigo-500/10 flex items-center justify-center">
                <svg className="w-7 h-7 text-indigo-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={1.5} d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2" />
                </svg>
              </div>
              <p className="text-gray-400 text-xs">Enter a token and scan for arbitrage opportunities</p>
            </div>
          ) : null}
          {selectedOpp && <ExecutionPanel opportunity={selectedOpp} onClose={() => setSelectedOpp(null)} />}
        </>
      )}

      {/* View: All Prices */}
      {viewMode === 'all_prices' && !scanning && allPrices && (
        <AllPricesTable
          prices={allPrices.prices}
          summaries={allPrices.chain_summary}
          token={allPrices.token}
          scanTimeMs={allPrices.scan_time_ms}
          onExecuteBest={(buy, sell) => {
            setSelectedOpp({
              id: 'direct',
              token_symbol: token,
              token_address: buy.token_address,
              buy_chain_id: buy.chain_id, buy_chain_name: buy.chain_name, buy_dex: buy.dex_name, buy_price_usd: buy.price_usd,
              sell_chain_id: sell.chain_id, sell_chain_name: sell.chain_name, sell_dex: sell.dex_name, sell_price_usd: sell.price_usd,
              spread_pct: buy.price_usd > 0 ? ((sell.price_usd - buy.price_usd) / buy.price_usd) * 100 : 0,
              estimated_profit_usd: sell.price_usd - buy.price_usd,
              liquidity_usd: Math.min(buy.liquidity_usd, sell.liquidity_usd),
              timestamp: Date.now(),
            });
            setViewMode('opportunities');
          }}
        />
      )}

      {/* View: Advanced Engine */}
      {viewMode === 'advanced' && !scanning && (
        <AdvancedExecutionPanel
          opportunities={allOpps}
          token={token}
          onClose={() => setViewMode('opportunities')}
        />
      )}
    </div>
  );
}
