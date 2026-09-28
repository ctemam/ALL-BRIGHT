'use client';

import { useMemo } from 'react';
import {
  PieChart,
  Pie,
  Cell,
  ResponsiveContainer,
  Tooltip,
  Legend,
} from 'recharts';

interface RouteSegment {
  exchange: string;
  percent: number;
  src_amount: string;
  dest_amount: string;
}

interface Props {
  routes: RouteSegment[];
  srcToken?: string;
  destToken?: string;
}

const DEX_COLORS: Record<string, string> = {
  Uniswap: '#FF007A',
  'Uniswap V3': '#FF007A',
  SushiSwap: '#0B5E8A',
  Balancer: '#6F42C1',
  Curve: '#00B8A0',
  ParaSwap: '#6366F1',
  '1inch': '#0F0F0F',
  QuickSwap: '#4B9B82',
  PancakeSwap: '#FFB300',
  Dodo: '#F7B731',
  KyberSwap: '#23EBC3',
  TraderJoe: '#FF6B35',
  Camelot: '#00D4AA',
  Velodrome: '#FF0420',
  GMX: '#00FF00',
  default: '#6366F1',
};

function getDexColor(name: string): string {
  for (const [key, color] of Object.entries(DEX_COLORS)) {
    if (name.toLowerCase().includes(key.toLowerCase())) return color;
  }
  return DEX_COLORS.default;
}

const RADIAN = Math.PI / 180;

function renderCustomizedLabel({
  cx,
  cy,
  midAngle,
  innerRadius,
  outerRadius,
  percent,
}: {
  cx: number;
  cy: number;
  midAngle: number;
  innerRadius: number;
  outerRadius: number;
  percent: number;
}) {
  if (percent < 0.05) return null;
  const radius = innerRadius + (outerRadius - innerRadius) * 0.5;
  const x = cx + radius * Math.cos(-midAngle * RADIAN);
  const y = cy + radius * Math.sin(-midAngle * RADIAN);
  return (
    <text
      x={x}
      y={y}
      fill="white"
      textAnchor="middle"
      dominantBaseline="central"
      fontSize={11}
      fontWeight={600}
    >
      {(percent * 100).toFixed(0)}%
    </text>
  );
}

export function RouteVisualization({ routes, srcToken, destToken }: Props) {
  const chartData = useMemo(
    () =>
      routes.map((r) => ({
        name: r.exchange,
        value: r.percent,
        pctLabel: `${r.percent.toFixed(1)}%`,
        src: r.src_amount,
        dest: r.dest_amount,
      })),
    [routes]
  );

  if (routes.length === 0) {
    return (
      <div className="flex items-center justify-center h-48 text-gray-500 text-sm">
        No route data available
      </div>
    );
  }

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <h4 className="text-xs font-semibold text-gray-400 uppercase tracking-wider">
          Split Route Distribution
        </h4>
        {srcToken && destToken && (
          <span className="text-xs text-gray-500">
            {srcToken} &rarr; {destToken}
          </span>
        )}
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
        {/* Pie Chart */}
        <div className="h-64">
          <ResponsiveContainer width="100%" height="100%">
            <PieChart>
              <Pie
                data={chartData}
                cx="50%"
                cy="50%"
                innerRadius={60}
                outerRadius={100}
                dataKey="value"
                labelLine={false}
                label={renderCustomizedLabel}
                animationBegin={0}
                animationDuration={800}
              >
                {chartData.map((entry, i) => (
                  <Cell
                    key={i}
                    fill={getDexColor(entry.name)}
                    stroke="rgba(255,255,255,0.05)"
                    strokeWidth={1}
                  />
                ))}
              </Pie>
              <Tooltip
                contentStyle={{
                  background: '#1a1a24',
                  border: '1px solid rgba(255,255,255,0.06)',
                  borderRadius: '8px',
                  fontSize: '12px',
                }}
                itemStyle={{ color: '#e5e5e5' }}
                labelStyle={{ color: '#999' }}
                formatter={(value: number) => `${value.toFixed(1)}%`}
              />
            </PieChart>
          </ResponsiveContainer>
        </div>

        {/* Route Breakdown List */}
        <div className="space-y-2">
          {routes.map((route, i) => (
            <div
              key={i}
              className="flex items-center gap-3 p-2 rounded-lg bg-white/5"
            >
              <div
                className="w-3 h-3 rounded-full flex-shrink-0"
                style={{ backgroundColor: getDexColor(route.exchange) }}
              />
              <div className="flex-1 min-w-0">
                <div className="flex items-center justify-between">
                  <span className="text-sm font-medium text-white truncate">
                    {route.exchange}
                  </span>
                  <span className="text-sm font-mono font-semibold text-indigo-400">
                    {route.percent.toFixed(1)}%
                  </span>
                </div>
                <div className="flex items-center gap-2 text-xs text-gray-500 mt-0.5">
                  <span>In: {route.src_amount}</span>
                  <span>&rarr;</span>
                  <span>Out: {route.dest_amount}</span>
                </div>
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}
