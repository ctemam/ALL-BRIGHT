'use client';

import { useEffect, useRef, useCallback, useState } from 'react';

interface WsMessage {
  type: string;
  data?: unknown;
  message?: string;
  token?: string;
  [key: string]: unknown;
}

type MessageHandler = (msg: WsMessage) => void;

export function useWebSocket(onMessage?: MessageHandler) {
  const wsRef = useRef<WebSocket | null>(null);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const handlersRef = useRef<MessageHandler | undefined>(onMessage);
  handlersRef.current = onMessage;

  const connect = useCallback(() => {
    const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
    // In production, this reads API_BASE or window.location.host
    const url = `${protocol}//localhost:3001/ws`;

    try {
      const ws = new WebSocket(url);

      ws.onopen = () => {
        setConnected(true);
        setError(null);
      };

      ws.onmessage = (event) => {
        try {
          const msg = JSON.parse(event.data) as WsMessage;
          handlersRef.current?.(msg);
        } catch {
          // ignore parse errors
        }
      };

      ws.onclose = () => {
        setConnected(false);
        wsRef.current = null;
      };

      ws.onerror = () => {
        setError('WebSocket connection failed');
        setConnected(false);
      };

      wsRef.current = ws;
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Connection failed');
    }
  }, []);

  const disconnect = useCallback(() => {
    wsRef.current?.close();
    wsRef.current = null;
    setConnected(false);
  }, []);

  const send = useCallback((msg: Record<string, unknown>) => {
    if (wsRef.current?.readyState === WebSocket.OPEN) {
      wsRef.current.send(JSON.stringify(msg));
    }
  }, []);

  const subscribe = useCallback(
    (token: string) => {
      send({ type: 'subscribe', token });
    },
    [send]
  );

  const unsubscribe = useCallback(
    (token: string) => {
      send({ type: 'unsubscribe', token });
    },
    [send]
  );

  useEffect(() => {
    return () => {
      wsRef.current?.close();
    };
  }, []);

  return { connect, disconnect, send, subscribe, unsubscribe, connected, error };
}
