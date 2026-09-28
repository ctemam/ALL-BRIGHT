import type { Metadata } from 'next';
import { Inter } from 'next/font/google';
import './globals.css';
import { Providers } from './providers';

const inter = Inter({ subsets: ['latin'], variable: '--font-inter' });

export const metadata: Metadata = {
  title: 'Zero-Cap Arbitrage | Zero-Capital DeFi Arbitrage Scanner',
  description:
    'Real-time cross-chain DEX arbitrage scanner. Execute trades with 0 capital and 0 upfront gas via Flashbots & ERC-4337 paymasters.',
  keywords: [
    'arbitrage',
    'defi',
    'flash-loan',
    'cross-chain',
    'paraswap',
    'flashbots',
  ],
};

export default function RootLayout({
  children,
}: {
  children: React.ReactNode;
}) {
  return (
    <html lang="en" className="dark">
      <body className={`${inter.variable} font-sans antialiased`}>
        <Providers>{children}</Providers>
      </body>
    </html>
  );
}
