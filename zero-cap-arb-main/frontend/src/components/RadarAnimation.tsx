'use client';

interface Props {
  active: boolean;
}

export function RadarAnimation({ active }: Props) {
  if (!active) return null;

  return (
    <div className="relative w-20 h-20 mx-auto">
      {/* Outer rings */}
      <div className="absolute inset-0 rounded-full border border-indigo-500/20 animate-pulse-slow" />
      <div className="absolute inset-2 rounded-full border border-indigo-500/15 animate-pulse-slow" style={{ animationDelay: '0.5s' }} />
      <div className="absolute inset-4 rounded-full border border-indigo-500/10 animate-pulse-slow" style={{ animationDelay: '1s' }} />

      {/* Scanning line */}
      <div className="absolute inset-0 rounded-full overflow-hidden">
        <div
          className="w-full h-0.5 bg-gradient-to-r from-transparent via-indigo-400 to-transparent 
                     absolute top-1/2 -translate-y-1/2 animate-scan-line"
          style={{
            boxShadow: '0 0 20px rgba(99,102,241,0.5), 0 0 60px rgba(99,102,241,0.2)',
          }}
        />
      </div>

      {/* Center dot */}
      <div className="absolute inset-[calc(50%-3px)] w-1.5 h-1.5 rounded-full bg-indigo-400 animate-glow-pulse" />
    </div>
  );
}
