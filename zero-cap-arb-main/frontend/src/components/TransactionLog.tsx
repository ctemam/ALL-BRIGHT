'use client';

import { useState, useCallback, useEffect } from 'react';

interface TxRecord {
  id: string;
  token: string;
  buyChain: string;
  sellChain: string;
  spread: number;
  profit: number;
  flashLoan: string;
  gasStrategy: string;
  hash?: string;
  status: 'pending' | 'success' | 'failed';
  timestamp: number;
  error?: string;
}

const STORAGE_KEY = 'zero-cap-arb-tx-history';

function loadHistory(): TxRecord[] {
  if (typeof window === 'undefined') return [];
  try {
    const raw = localStorage.getItem(STORAGE_KEY);
    return raw ? JSON.parse(raw) : [];
  } catch {
    return [];
  }
}

export function TransactionLog({ newTx }: { newTx?: Omit<TxRecord, 'id' | 'timestamp'> }) {
  const [txs, setTxs] = useState<TxRecord[]>(loadHistory);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(txs));
  }, [txs]);

  const addTx = useCallback(
    (partial: Omit<TxRecord, 'id' | 'timestamp'>) => {
      const tx: TxRecord = {
        ...partial,
        id: `${Date.now()}-${Math.random().toString(36).slice(2, 8)}`,
        timestamp: Date.now(),
      };
      setTxs((prev) => [tx, ...prev].slice(0, 50));
    },
    []
  );

  // Expose addTx globally for the execution panel
  useEffect(() => {
    (window as Record<string, unknown>).__addTx = addTx;
    return () => {
      delete (window as Record<string, unknown>).__addTx;
    };
  }, [addTx]);

  const clearHistory = useCallback(() => {
    setTxs([]);
    localStorage.removeItem(STORAGE_KEY);
  }, []);

  if (txs.length === 0) return null;

  return (
    <div className="glass-card">
      <button
        onClick={() => setExpanded(!expanded)}
        className="w-full flex items-center justify-between p-4 text-left"
      >
        <div className="flex items-center gap-2">
          <svg className="w-4 h-4 text-gray-400" fill="none" viewBox="0 0 24 24" stroke="currentColor">
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 5H7a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2V7a2 2 0 00-2-2h-2M9 5a2 2 0 002 2h2a2 2 0 002-2M9 5a2 2 0 012-2h2a2 2 0 012 2m-6 9l2 2 4-4" />
          </svg>
          <h3 className="text-sm font-semibold text-gray-300">
            Transaction History ({txs.length})
          </h3>
        </div>
        <div className="flex items-center gap-3">
          <span className="text-xs text-gray-500">
            {txs.filter((t) => t.status === 'success').length} succeeded
          </span>
          <svg
            className={`w-4 h-4 text-gray-400 transition-transform ${expanded ? 'rotate-180' : ''}`}
            fill="none" viewBox="0 0 24 24" stroke="currentColor"
          >
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          </svg>
        </div>
      </button>

      {expanded && (
        <div className="px-4 pb-4 space-y-2 max-h-80 overflow-y-auto">
          {txs.map((tx) => (
            <div
              key={tx.id}
              className={`flex items-center gap-3 p-2 rounded-lg text-xs ${
                tx.status === 'success'
                  ? 'bg-green-500/5'
                  : tx.status === 'failed'
                  ? 'bg-red-500/5'
                  : 'bg-yellow-500/5'
              }`}
            >
              <div
                className={`w-2 h-2 rounded-full ${
                  tx.status === 'success'
                    ? 'bg-green-500'
                    : tx.status === 'failed'
                    ? 'bg-red-500'
                    : 'bg-yellow-500 animate-pulse'
                }`}
              />
              <div className="flex-1">
                <div className="flex items-center gap-2">
                  <span className="font-semibold text-white">{tx.token}</span>
                  <span className="text-gray-500">
                    {tx.buyChain} &rarr; {tx.sellChain}
                  </span>
                  <span className="text-green-400 font-mono">
                    {tx.spread.toFixed(2)}%
                  </span>
                </div>
                <div className="text-gray-500 mt-0.5">
                  <span>Profit: ${tx.profit.toFixed(2)}</span>
                  <span className="mx-1">|</span>
                  <span>{tx.flashLoan}</span>
                  <span className="mx-1">|</span>
                  <span>{tx.gasStrategy}</span>
                  {tx.hash && (
                    <>
                      <span className="mx-1">|</span>
                      <span className="font-mono text-indigo-400">
                        {tx.hash.slice(0, 10)}...
                      </span>
                    </>
                  )}
                </div>
              </div>
              <span className="text-[10px] text-gray-600">
                {new Date(tx.timestamp).toLocaleTimeString()}
              </span>
            </div>
          ))}

          <button
            onClick={clearHistory}
            className="w-full text-center text-xs text-gray-500 hover:text-gray-400 pt-2 transition-colors"
          >
            Clear history
          </button>
        </div>
      )}
    </div>
  );
}
