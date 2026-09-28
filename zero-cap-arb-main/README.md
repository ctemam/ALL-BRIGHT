
# ⚡ Zero-Capital Arbitrage Web App

**Zero-Capital Arbitrage** is a next-generation DeFi arbitrage platform with a **professional autonomous trading bot**. It executes **flash-loan arbitrage**, **triangular arbitrage**, **cross-chain arbitrage**, **mint arbitrage**, and **JIT liquidity** — all with **zero upfront capital** and **zero upfront gas fees**.

A real-time radar scans **55+ DEXes across 6 chains** (Ethereum, Arbitrum, Optimism, Polygon, BSC, Avalanche) to surface every price difference, then executes the optimal trade via **Velora Market API (ex-ParaSwap)**, funded by flash loans from **Aave V3 / Radiant V2 / Spark Protocol**, and submitted through **Flashbots / MEV-Share** private bundles or **ERC-4337 paymasters**.

The **Pro Bot** adds: Paper Trading / Backtesting, Multi-Strategy Portfolio, MEV Protection (Sandwich Detection), Telegram/Discord Alerts, Conditional Execution Rules, Profit Splitter / Multi-Wallet, and Adaptive Gas Bidding.

---

## 📖 Table of Contents / جدول المحتويات

- [English](#-english)
  - [Features](#-features)
  - [Architecture Blueprint](#-architecture-blueprint)
  - [Pro Bot Blueprint](#-pro-bot-blueprint)
  - [Data Flow Schematic](#-data-flow-schematic)
  - [Component Map](#-component-map)
  - [Tech Stack](#-tech-stack)
  - [API Reference](#-api-reference)
  - [Smart Contract](#-smart-contract)
- [العربية](#-العربية)
  - [الميزات](#-الميزات)
  - [المخطط المعماري](#-المخطط-المعماري)
  - [مخطط البوت الاحترافي](#-مخطط-البوت-الاحترافي)
  - [مخطط تدفق البيانات](#-مخطط-تدفق-البيانات)
  - [خريطة المكونات](#-خريطة-المكونات)
  - [مرجع API](#-مرجع-api)
  - [العقد الذكي](#-العقد-الذكي)

---

## 🇬🇧 English

### ✨ Features

| Feature | Description |
|---------|-------------|
| **Zero-Capital Arbitrage** | Flash-loan funded trades. Borrow from Aave V3 / Radiant V2 / Spark Protocol (0% on DAI). |
| **Zero-Upfront Gas** | Flashbots/MEV-Share private bundles or ERC-4337 paymasters (Pimlico/ZeroDev). |
| **55+ DEX Scanner** | Real-time across 6 chains (Ethereum, Arbitrum, Optimism, Polygon, BSC, Avalanche). |
| **Velora Market API** | Split-route DEX aggregation (ex-ParaSwap, same base URL). |
| **6 Arbitrage Strategies** | Simple, Triangular, Cross-Chain, Mint, JIT Liquidity, Direct Swap. |
| **Paper Trading** | $10K simulated balance, 50-trade backtest, Sharpe ratio, max drawdown. |
| **Multi-Strategy Portfolio** | Weight allocation, risk management, parallel execution across 5 strategies. |
| **MEV Protection** | Sandwich/frontrun/backrun detection, honeypot check, Flashbots fallback. |
| **Telegram/Discord Alerts** | Live notifications on every trade, error, opportunity. |
| **Conditional Rules** | Custom if/then logic: "execute only if spread > 2% and profit > $20". |
| **Profit Splitter** | Auto-distribute profits across multiple wallets (e.g., 70% you, 20% reserve, 10% dev). |
| **Adaptive Gas Bidding** | Dynamic gas calculation based on profit margin + spread bonus. |
| **AI Advisor** | Connect OpenAI/Anthropic/Groq/Ollama to get trade recommendations. |
| **RTL / Arabic Support** | Full Arabic interface with RTL layout toggle. |
| **Crypto Bubble Chart** | Visual token bubbles sized by liquidity, glowing green when opportunities exist. |
| **Liquidity Map** | Horizontal bar chart showing liquidity distribution across chains & DEXes. |

---

### 🏗 Architecture Blueprint

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                            NEXT.JS 14 FRONTEND :3000                                 │
│                                                                                      │
│  ┌─────────────────┐  ┌───────────────────┐  ┌────────────────────────────────┐     │
│  │ DASHBOARD TAB   │  │ RADAR TAB         │  │ BOT TAB                        │     │
│  │                  │  │                    │  │  ┌─────────────────────────┐   │     │
│  │  Stats Cards     │  │  RadarDashboard   │  │  │ Mode: Manual/Auto      │   │     │
│  │  OpportunityCard │  │  OpportunityCard  │  │  │ Strategy Toggles       │   │     │
│  │  CryptoBubble    │  │  TransactionLog   │  │  │ Filters & Limits       │   │     │
│  │  LiquidityMap    │  │                    │  │  │ PRO FEATURES (7 tabs): │   │     │
│  └─────────────────┘  └───────────────────┘  │  │  📊 Paper Trading      │   │     │
│                                                │  │  📁 Portfolio           │   │     │
│  ┌─────────────────┐  ┌───────────────────┐   │  │  🛡️ MEV Guard          │   │     │
│  │ AI ADVISOR TAB  │  │ LIQUIDITY /       │   │  │  🔔 Alerts             │   │     │
│  │                 │  │ BUBBLES TABS      │   │  │  ⚙️ Rules              │   │     │
│  │  LLMConfig      │  │  LiquidityMap     │   │  │  💸 Splitter           │   │     │
│  │  AI Advice      │  │  CryptoBubble     │   │  │  ⛽ Gas Bidder         │   │     │
│  └─────────────────┘  └───────────────────┘   │  └─────────────────────────┘   │     │
│                                                                                      │
│  ┌─ WalletConnect (wagmi) ──┐  ┌─ useWebSocket (real-time) ─┐  ┌─ RTL Toggle ──┐    │
│  └──────────────────────────┘  └────────────────────────────┘  └───────────────┘    │
└────────────────────────────────────┬────────────────────────────────────────────────┘
                                     │ HTTP REST (JSON) / WS (WebSocket)
                                     │
┌────────────────────────────────────▼────────────────────────────────────────────────┐
│                            RUST BACKEND (Axum) :3001                                 │
│                                                                                      │
│  ┌──────────────────────────────────────────────────────────────────────────────┐   │
│  │  CORE ENGINE                                                                │   │
│  │  ┌──────────────┐  ┌────────────────┐  ┌──────────────────┐  ┌───────────┐  │   │
│  │  │ RadarScanner │  │ VeloraClient   │  │  WebSocket Hub   │  │ Chains    │  │   │
│  │  │ 6 chains ×   │  │ Market API     │  │  (real-time      │  │ + DEXes   │  │   │
│  │  │ 55 DEXes     │  │ + Delta API    │  │   broadcasts)    │  │ configs   │  │   │
│  │  └──────┬───────┘  └───────┬────────┘  └──────────────────┘  └───────────┘  │   │
│  └──────────┼─────────────────┼─────────────────────────────────────────────────┘   │
│             │                 │                                                      │
│  ┌──────────▼─────────────────▼─────────────────────────────────────────────────┐   │
│  │  PRO BOT MODULES (7)                                                         │   │
│  │                                                                               │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │   │
│  │  │ PaperTrader  │  │ Portfolio    │  │  MevGuard    │  │AlertManager  │     │   │
│  │  │ • Backtest   │  │ • Allocation │  │ • Sandwich   │  │• Telegram    │     │   │
│  │  │ • Simulation │  │ • Risk Mgmt  │  │ • Honeypot   │  │• Discord     │     │   │
│  │  │ • Metrics    │  │ • PnL Track  │  │ • Flashbots  │  │• Webhook     │     │   │
│  │  └──────────────┘  └──────────────┘  └──────────────┘  └──────────────┘     │   │
│  │  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐                       │   │
│  │  │ RulesEngine  │  │ProfitSplitter│  │  GasBidder   │                       │   │
│  │  │ • If/Then    │  │ • Multi-Wallet│  │ • Adaptive   │                       │   │
│  │  │ • Evaluation │  │ • Pie Split  │  │ • Priority   │                       │   │
│  │  │ • Actions    │  │ • Treasury   │  │ • MEV-Protect│                       │   │
│  │  └──────────────┘  └──────────────┘  └──────────────┘                       │   │
│  └─────────────────────────────────────────────────────────────────────────────┘   │
│                                                                                      │
│  ┌──────────────────────────────────────────────────────────────────────────────┐   │
│  │  Shared State (parking_lot::RwLock)                                         │   │
│  │  AppState { scanner, velora, bot_status, bot_config, llm_config,             │   │
│  │            paper_trader, portfolio_manager, mev_guard, alert_manager,         │   │
│  │            rules_engine, profit_splitter, gas_bidder }                       │   │
│  └──────────────────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────┬────────────────────────────────────────────────┘
                                     │ alloy-rs RPC
┌────────────────────────────────────▼────────────────────────────────────────────────┐
│                         SMART CONTRACTS (Solidity / Foundry)                         │
│                                                                                      │
│  ┌──────────────────────────────────────────────────────────────────────────────┐   │
│  │  ZeroRiskArb.sol                                                             │   │
│  │                                                                               │   │
│  │  ┌───────────────────────────┐  ┌───────────────────────────┐                │   │
│  │  │ Flash Loan Sources        │  │ DEX Execution             │                │   │
│  │  │ • Aave V3                 │  │ • Velora Augustus call()  │                │   │
│  │  │   flashLoanSimple()       │  │ • Split-route swaps       │                │   │
│  │  │ • Radiant V2              │  │ • Multi-hop routing       │                │   │
│  │  │   flashLoan()             │  │                           │                │   │
│  │  │ • Spark Protocol          │  │ Profit Safety             │                │   │
│  │  │   flashLoanSimple() (0%)  │  │ • balance < loan + fee +  │                │   │
│  │  └───────────────────────────┘  │   minProfit → REVERT      │                │   │
│  │                                 │ • User loses $0.00        │                │   │
│  │  ┌───────────────────────────┐  └───────────────────────────┘                │   │
│  │  │ Gas Strategies            │                                               │   │
│  │  │ • Flashbots coinbase tip  │  ┌───────────────────────────┐                │   │
│  │  │ • ERC-4337 compatibility  │  │ Foundry Tests (8 cases)   │                │   │
│  │  │ • MEV-Share support       │  │ • Fuzz + gas benchmarks   │                │   │
│  │  └───────────────────────────┘  └───────────────────────────┘                │   │
│  └──────────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

---

### 🤖 Pro Bot Blueprint

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          BOT PIPELINE                                       │
│                                                                              │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌───────┐ │
│  │ SCAN ALL │───▶│ EVALUATE │───▶│ FILTER   │───▶│ EXECUTE  │───▶│SPLIT  │ │
│  │ DEXes    │    │ RULES    │    │ MEV      │    │ via      │    │PROFIT │ │
│  │ 55 DEXes │    │ ENGINE   │    │ GUARD    │    │ FlashLoan│    │ WALLETS│ │
│  │ 6 Chains │    │ 5 Rules  │    │ Sandwich │    │ Velora   │    │ 70/20/10│ │
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘    └───────┘ │
│                                                                              │
│  ┌──────────────────────────────────────────────────────────────────────┐   │
│  │ DECISION TREE                                                       │   │
│  │                                                                      │   │
│  │  Opportunity Found?                                                  │   │
│  │  ├── No  → Wait scan_interval_secs (30s default)                    │   │
│  │  └── Yes →                                                            │   │
│  │      ├── Paper Trading Mode On? → PaperTrader.simulate_trade()      │   │
│  │      ├── Rules Engine → All rules match?                             │   │
│  │      │   ├── No  → Skip / Log / Notify                              │   │
│  │      │   └── Yes →                                                    │   │
│  │      │       ├── MEV Guard → Risk > threshold?                      │   │
│  │      │       │   ├── Yes → Use Flashbots private bundle             │   │
│  │      │       │   └── No  → Proceed                                  │   │
│  │      │       ├── Portfolio → can_trade()?                           │   │
│  │      │       │   ├── No  → Max positions reached                    │   │
│  │      │       │   └── Yes →                                           │   │
│  │      │       │       ├── Gas Bidder → recommend_gas()               │   │
│  │      │       │       ├── Execute trade via FlashLoan + Velora       │   │
│  │      │       │       ├── Profit Splitter → calculate_split()        │   │
│  │      │       │       ├── Alerts → send Telegram/Discord             │   │
│  │      │       │       └── Portfolio → record_trade()                 │   │
│  │      └── AI Advisor On? → LLM advice → user confirmation           │   │
│  └──────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

### 🔄 Data Flow Schematic

#### Complete Trade Flow

```
USER ACTION                    SYSTEM RESPONSE
────────────                   ───────────────

1. User scans token ──────────▶ RadarScanner queries 55 DEXes × 6 chains
                               │ Returns: all prices + opportunities
                               ◀──────────────────────────────

2. Comprehensive Scan ────────▶ ComprehensiveScanResponse
   (all tokens, all DEXes)     │ Net profit calculation:
                               │   gross - gas - flash loan fee - slippage
                               │ Flash loan recommendation:
                               │   Spark (0% DAI) > Radiant (0.03%) > Aave (0.05%)
                               │ Confidence score
                               ◀──────────────────────────────

3. User opens Bot tab ────────▶ BotPanel loads config + status
                               │ Pro Features sub-tabs appear:
                               │   Paper | Portfolio | MEV | Alerts | Rules | Splitter | Gas
                               ◀──────────────────────────────

4. Bot START ─────────────────▶ Bot cycle begins:
                                  1. Scan all enabled chains/DEXes
                                  2. Evaluate Rules Engine
                                  3. Check MEV Guard safety
                                  4. Check Portfolio capacity
                                  5. Get Gas recommendation
                                  6. Execute trade
                                  7. Split profit across wallets
                                  8. Send alert
                               ◀──────────────────────────────

5. Execute trade ────────────▶ Smart contract flow:
                                  FlashLoan (Spark DAO 0%)
                                  → Velora Augustus.call()
                                  → Buy cheap DEX → Sell expensive DEX
                                  → Profit? → Repay loan + send profit
                                  → No profit? → REVERT (user pays $0)
                                  → Flashbots coinbase tip
                               ◀──────────────────────────────
                                  Tx hash + profit result
```

---

### 🗺 Component Map

```
FRONTEND (components/)
├── WalletConnect.tsx          ← MetaMask + WalletConnect
├── RadarDashboard.tsx         ← Main radar scan UI (3 tabs)
├── OpportunityTable.tsx       ← Simple arbitrage table
├── AllPricesTable.tsx         ← Full DEX price matrix
├── AdvancedExecutionPanel.tsx ← 6-strategy execution engine
├── ExecutionPanel.tsx         ← Flash loan + gas strategy selectors
├── RouteVisualization.tsx     ← Recharts pie chart (Velora split routes)
├── TransactionLog.tsx         ← Persistent trade history
├── RadarAnimation.tsx         ← Animated radar sweep
├── OpportunityCard.tsx        ← Full detailed opportunity (cost breakdown + FL rec)
├── CryptoBubbleChart.tsx       ← Token bubbles sized by liquidity
├── LiquidityMap.tsx           ← Chain/DEX liquidity distribution
├── LLMConfig.tsx              ← AI advisor (OpenAI/Anthropic/Groq/etc.)
├── BotPanel.tsx               ← Bot config + 7 Pro sub-tabs
├── PaperTraderPanel.tsx       ← Paper trading / backtesting
├── PortfolioManager.tsx       ← Multi-strategy allocation
├── MevGuard.tsx               ← MEV sandwich protection
├── AlertsConfig.tsx           ← Telegram/Discord alerts
├── RulesBuilder.tsx           ← Conditional execution rules
├── ProfitSplitter.tsx         ← Multi-wallet profit distribution
└── GasBidder.tsx              ← Adaptive gas bidding

BACKEND (src/)
├── main.rs                    ← Entry point, state init, CORS
├── types.rs                   ← All shared types (3500+ lines)
├── chains.rs                  ← 6 chains × 55 DEX configs
├── radar_scanner.rs           ← Core price scanner + comprehensive scan
├── velora_client.rs           ← Velora Market API + Delta API
├── websocket.rs               ← Real-time WS broadcasts
├── api.rs                     ← 40+ REST endpoints
├── paper_trader.rs            ← Backtest + simulation engine
├── portfolio_manager.rs       ← Strategy allocation + risk mgmt
├── mev_guard.rs               ← Sandwich/frontrun/backrun detection
├── alert_manager.rs           ← Telegram/Discord/Webhook alerts
├── rules_engine.rs            ← If/Then condition evaluator
├── profit_splitter.rs         ← Multi-wallet percentage splitter
└── gas_bidder.rs              ← Dynamic gas pricing strategies

CONTRACTS (contracts/)
├── src/ZeroRiskArb.sol        ← Flash loan + Velora + profit safety
├── src/interfaces/             ← IVeloraAugustus, IFlashLoan, etc.
└── src/ZeroRiskArb.t.sol      ← Foundry tests (8 cases)
```

---

### 🛠 Tech Stack

| Layer | Technology |
|-------|-----------|
| **Frontend** | Next.js 14, React 18, TypeScript, TailwindCSS, Recharts, lucide-react |
| **Web3** | wagmi 2.x, viem 2.x, @tanstack/react-query |
| **Backend** | Rust, Axum, alloy-rs, tokio-tungstenite, serde, parking_lot, dashmap |
| **Smart Contracts** | Solidity 0.8.20, Foundry (forge, cast, anvil) |
| **Flash Loans** | Aave V3, Radiant V2, Spark Protocol (0% on DAI) |
| **DEX Aggregation** | Velora Market API (ex-ParaSwap) |
| **Gas Strategies** | Flashbots MEV-Share, Pimlico (ERC-4337), ZeroDev (ERC-4337) |
| **AI** | OpenAI, Anthropic, Groq, Ollama, DeepSeek |

---

### 📡 API Reference

| Method | Endpoint | Description |
|--------|----------|-------------|
| `GET` | `/api/health` | Health check + connected chains |
| `POST` | `/api/scan` | Scan simple arbitrage opportunities |
| `POST` | `/api/scan/comprehensive` | Scan ALL tokens across ALL DEXes + net profit + FL rec |
| `POST` | `/api/all-prices` | Full DEX price matrix |
| `POST` | `/api/all-opportunities` | All 6 arbitrage strategies |
| `POST` | `/api/liquidity` | Chain/DEX liquidity distribution |
| `GET` | `/api/bubbles` | Token bubble chart data |
| `GET` | `/api/dashboard` | Unified dashboard data |
| `GET` | `/api/chains` | List all supported chains |
| `GET` | `/api/dexes/{chainId}` | List DEXes for chain |
| `POST` | `/api/velora/price` | Velora Market price + routes |
| `POST` | `/api/velora/swap` | Velora swap transaction |
| `POST` | `/api/velora/build-tx` | Build Velora transaction |
| `POST` | `/api/velora/delta` | Submit Delta intent order |
| `POST` | `/api/execute` | Execute basic arbitrage |
| `POST` | `/api/execute/advanced` | Execute any strategy (6 types) |
| **Paper Trading** | | |
| `POST` | `/api/paper/start` | Start paper trading |
| `POST` | `/api/paper/stop` | Stop + get backtest result |
| `GET` | `/api/paper/status` | Current paper trading metrics |
| `POST` | `/api/paper/simulate` | Simulate a single trade |
| `POST` | `/api/paper/backtest` | Run 50-trade backtest |
| `POST` | `/api/paper/reset` | Reset paper account |
| **Portfolio** | | |
| `GET` | `/api/portfolio/config` | Get portfolio config |
| `POST` | `/api/portfolio/config` | Update portfolio config |
| `GET` | `/api/portfolio/status` | Get portfolio metrics |
| **MEV Guard** | | |
| `GET` | `/api/mev/config` | Get MEV guard config |
| `POST` | `/api/mev/config` | Update MEV guard config |
| `POST` | `/api/mev/analyze` | Analyze mempool for MEV risk |
| **Alerts** | | |
| `GET` | `/api/alerts/config` | Get alert channels |
| `POST` | `/api/alerts/config` | Update alert channels |
| `GET` | `/api/alerts/history` | Get alert history |
| `POST` | `/api/alerts/test` | Send test alert |
| **Rules** | | |
| `GET` | `/api/rules` | Get execution rules |
| `POST` | `/api/rules` | Update execution rules |
| `POST` | `/api/rules/evaluate` | Evaluate rules on opportunity |
| **Splitter** | | |
| `GET` | `/api/splitter/config` | Get splitter config |
| `POST` | `/api/splitter/config` | Update splitter config |
| `POST` | `/api/splitter/calculate` | Calculate split for profit |
| **Gas** | | |
| `GET` | `/api/gas/config` | Get gas bidder config |
| `POST` | `/api/gas/config` | Update gas bidder config |
| `POST` | `/api/gas/recommend` | Get gas recommendation |
| **LLM** | | |
| `GET` | `/api/llm/config` | Get LLM config |
| `POST` | `/api/llm/config` | Update LLM config |
| `POST` | `/api/llm/advise` | Get AI trading advice |
| **Bot** | | |
| `GET` | `/api/bot/config` | Get bot config |
| `POST` | `/api/bot/config` | Update bot config |
| `POST` | `/api/bot/start` | Start bot |
| `POST` | `/api/bot/stop` | Stop bot |
| `GET` | `/api/bot/status` | Get bot status |
| `GET` | `/api/bot/logs` | Get bot logs |
| `WS` | `/ws` | Real-time WebSocket |

---

### 📜 Smart Contract

**`ZeroRiskArb.sol`** — The core execution contract deployed on any EVM chain:

```
┌──────────────────────────────────────────────────────────────────┐
│                        ZeroRiskArb.sol                            │
│                                                                   │
│  executeArbitrage(                                                 │
│    flashLoanSource,   // Spark | Aave | Radiant                   │
│    flashLoanAmount,   // Amount to borrow                         │
│    tokenToBuy,         // Token address to buy                     │
│    tokenToSell,        // Token address to sell                    │
│    swapData,           // Velora Augustus calldata                 │
│    minProfit           // Minimum acceptable profit                │
│  )                                                                 │
│                                                                   │
│  ┌─────────────────────────────────────────────────────────┐      │
│  │ 1. FlashLoan provider executes flashLoan()               │      │
│  │ 2. executeOperation() callback:                          │      │
│  │    • Velora Augustus.call(swapData) → buy/sell           │      │
│  │    • Profit = balance - loan - fee                       │      │
│  │    • if profit < minProfit → revert                      │      │
│  │    • Transfer profit to user                             │      │
│  │    • Flashbots: swap 10% profit → WETH, coinbase.tip    │      │
│  │ 3. Repay loan + fee to flash loan provider               │      │
│  └─────────────────────────────────────────────────────────┘      │
│                                                                   │
│  SAFETY: if (address(this).balance < loan + fee + minProfit)      │
│          → revert()  // User loses exactly $0                     │
└──────────────────────────────────────────────────────────────────┘
```

---

## 🇸🇦 العربية

### ✨ الميزات

| الميزة | الوصف |
|--------|-------|
| **مراجحة بدون رأس مال** | اقتراض فوري من Aave V3 / Radiant V2 / Spark Protocol (0% على DAI). |
| **غاز بدون دفعة مقدمة** | Flashbots أو ERC-4337 — تدفع بس من الربح. |
| **55+ بورصة × 6 سلاسل** | إيثريوم، أربيتروم، أوبتيمزم، بوليجون، بي إن بي، أفالانش. |
| **تجميع Velora** | أفضل الأسعار عبر تقسيم الصفقة على كل البورصات. |
| **6 استراتيجيات** | بسيط، مثلثي، عبر السلاسل، صك، سيولة فورية، مبادلة مباشرة. |
| **التداول الورقي** | 10,000$ محاكاة، اختبار 50 صفقة، Sharpe ratio, Max Drawdown. |
| **محفظة متعددة الاستراتيجيات** | توزيع الوزن، إدارة المخاطر، تنفيذ متوازي. |
| **حماية MEV** | كشف الساندويتش والفرونتران، فحص الهانيبوت. |
| **إشعارات** | تيليجرام / ديسكورد — كل صفقة وربح وغلط. |
| **قواعد مخصصة** | "نفذ فقط إذا spread > 2% و profit > 20$". |
| **تقسيم الربح** | وزع الأرباح أوتوماتيك على محافظ متعددة. |
| **تسعير الغاز الذكي** | يحسب الغاز ديناميكيا حسب الربح المتوقع. |
| **مستشار ذكاء اصطناعي** | OpenAI / Anthropic / Groq / Ollama — يساعدك في القرارات. |
| **واجهة عربية** | دعم كامل للغة العربية مع RTL. |
| **الفقاعات** | عملات كفقاعات ملونة — اللي فيها فرصة تتوهج بالأخضر. |
| **خريطة السيولة** | رسم بياني لتوزيع السيولة على السلاسل والبورصات. |

---

### 🏗 المخطط المعماري

```
┌─────────────────────────────────────────────────────────────────────────────────────┐
│                             واجهة Next.js 14 :3000                                   │
│                                                                                      │
│  علامات التبويب:  [Dashboard] [Radar] [Bot]  [AI Advisor] [Liquidity] [Bubbles]      │
│                                                                                      │
│  ┌────────────────────┐  ┌──────────────┐  ┌────────────────────────────────────┐   │
│  │ Dashboard          │  │ Bot Panel    │  │ المكونات الأخرى                   │   │
│  │ • إحصائيات          │  │ • الوضع      │  │ • CryptoBubbleChart               │   │
│  │ • الفرص الحية      │  │ • الاستراتيجيات│  │ • LiquidityMap                   │   │
│  │ • خريطة السيولة    │  │ • الفلاتر    │  │ • LLMConfig                      │   │
│  │ • فقاعات العملات  │  │ • 7 ميزات Pro│  │ • OpportunityCard                 │   │
│  └────────────────────┘  └──────────────┘  └────────────────────────────────────┘   │
└────────────────────────────────────┬────────────────────────────────────────────────┘
                                     │ HTTP / WebSocket
┌────────────────────────────────────▼────────────────────────────────────────────────┐
│                            خادم Rust (Axum) :3001                                    │
│                                                                                      │
│  ┌──────────────────────────────────────────────────────────────────────────────┐   │
│  │  المحرك الأساسي                          │  متجر الحالة المشتركة              │   │
│  │  • RadarScanner: مسح 55 بورصة            │  AppState {                       │   │
│  │  • VeloraClient: تجميع الأسعار           │    scanner, velora,               │   │
│  │  • WebSocket Hub: بث فوري                │    paper_trader,                  │   │
│  │  • Chains + DEXes config                │    portfolio_manager,             │   │
│  │                                          │    mev_guard,                     │   │
│  │  وحدات البوت الاحترافية (7):             │    alert_manager,                 │   │
│  │  • PaperTrader    • PortfolioManager     │    rules_engine,                  │   │
│  │  • MevGuard       • AlertManager         │    profit_splitter,               │   │
│  │  • RulesEngine    • ProfitSplitter       │    gas_bidder                     │   │
│  │  • GasBidder                             │  }                                │   │
│  └──────────────────────────────────────────────────────────────────────────────┘   │
└────────────────────────────────────┬────────────────────────────────────────────────┘
                                     │ RPC (alloy-rs)
┌────────────────────────────────────▼────────────────────────────────────────────────┐
│                            العقود الذكية (Solidity)                                  │
│                                                                                      │
│  ┌──────────────────────────────────────────────────────────────────────────────┐   │
│  │  ZeroRiskArb.sol                                                             │   │
│  │  • القروض الفورية: Spark (0%) / Aave V3 / Radiant V2                        │   │
│  │  • التنفيذ: Velora Augustus.call() → اشتر رخيص → بيع غالي                    │   │
│  │  • الأمان: إذا الربح < الحد الأدنى → إلغاء (0$ خسارة)                       │   │
│  │  • الغاز: Flashbots coinbase tip / ERC-4337                                  │   │
│  └──────────────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

---

### 🤖 مخطط البوت الاحترافي

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                          دورة البوت                                         │
│                                                                              │
│  ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌───────┐ │
│  │ مسح كل   │───▶│ تقييم    │───▶│ فحص MEV  │───▶│ تنفيذ    │───▶│تقسيم  │ │
│  │ البورصات │    │ القواعد  │    │          │    │ الصفقة   │    │الربح  │ │
│  └──────────┘    └──────────┘    └──────────┘    └──────────┘    └───────┘ │
│                                                                              │
│  شجرة القرارات:                                                              │
│  → هل توجد فرصة؟                                                            │
│     ├── لا → انتظر 30 ثانية                                                  │
│     └── نعم →                                                               │
│         ├── التداول الورقي مفعل؟ → PaperTrader.simulate_trade()             │
│         ├── القواعد → كل القواعد تطابق؟                                     │
│         │   ├── لا → تخطي / تسجيل / إشعار                                    │
│         │   └── نعم →                                                        │
│         │       ├── MEV Guard → الخطر عالي؟ → استعمل Flashbots              │
│         │       ├── Portfolio → يمكن التداول؟ → السعة متاحة                 │
│         │       ├── Gas Bidder → احسب الغاز الأنسب                          │
│         │       ├── تنفيذ الصفقة → قرض فوري → Velora → ربح                  │
│         │       ├── Profit Splitter → وزع الأرباح                           │
│         │       └── Alerts → أرسل إشعار تيليجرام                            │
│         └── AI Advisor → استشر الذكاء الاصطناعي                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

### 🔄 مخطط تدفق البيانات

```
المستخدم                                    النظام
───────                                    ──────

1. يضغط Scan ────────────────────────────▶ RadarScanner يمسح 55 بورصة
                                           │ يعرض: كل الأسعار + الفرص
                                           ◀────────────────────────────

2. المسح الشامل ──────────────────────────▶ يشمل كل التوكنات
                                           │ يحسب: صافي الربح - الغاز - الرسوم
                                           │ يوصي: Spark > Radiant > Aave
                                           ◀────────────────────────────

3. يفتح Bot ──────────────────────────────▶ يظهر: الوضع + الاستراتيجيات
                                           │ 7 ميزات Pro: تداول ورقي، محفظة،
                                           │ حماية MEV، إشعارات، قواعد، تقسيم، غاز
                                           ◀────────────────────────────

4. Bot START ─────────────────────────────▶ الدورة: مسح ← قواعد ← MEV
                                           │ ← محفظة ← غاز ← تنفيذ ← تقسيم ← إشعار
                                           ◀────────────────────────────

5. تنفيذ صفقة ────────────────────────────▶ العقد الذكي:
                                           │ قرض فوري ← Velora ← اشتر ← بيع
                                           │ ربح؟ → سدد + اربح
                                           │ خسارة؟ → إلغاء (0$)
                                           ◀────────────────────────────
                                           │ Hash المعاملة + الربح
```

---

### 🗺 خريطة المكونات

```
الواجهة (components/)
├── WalletConnect.tsx          ← ربط المحفظة
├── BotPanel.tsx               ← لوحة البوت الرئيسية
├── PaperTraderPanel.tsx       ← التداول الورقي
├── PortfolioManager.tsx       ← المحفظة المتعددة
├── MevGuard.tsx               ← حماية MEV
├── AlertsConfig.tsx           ← الإشعارات
├── RulesBuilder.tsx           ← بناء القواعد
├── ProfitSplitter.tsx         ← تقسيم الربح
├── GasBidder.tsx              ← تسعير الغاز
├── LLMConfig.tsx              ← الذكاء الاصطناعي
├── CryptoBubbleChart.tsx      ← فقاعات العملات
├── LiquidityMap.tsx           ← خريطة السيولة
├── OpportunityCard.tsx        ← بطاقة الفرصة كاملة
└── RadarDashboard.tsx         ← لوحة المسح

الخادم (src/)
├── main.rs                    ← نقطة الدخول
├── types.rs                   ← كل الأنواع
├── chains.rs                  ← السلاسل والبورصات
├── radar_scanner.rs           ← ماسح الأسعار
├── velora_client.rs           ← عميل Velora
├── api.rs                     ← 40+ واجهة API
├── paper_trader.rs            ← محرك التداول الورقي
├── portfolio_manager.rs       ← مدير المحفظة
├── mev_guard.rs               ← حماية MEV
├── alert_manager.rs           ← مدير الإشعارات
├── rules_engine.rs            ← محرك القواعد
├── profit_splitter.rs         ← مقسم الأرباح
└── gas_bidder.rs              ← مسعّر الغاز

العقود (contracts/)
├── ZeroRiskArb.sol            ← العقد الذكي الرئيسي
├── interfaces/                ← واجهات العقود
└── ZeroRiskArb.t.sol          ← 8 اختبارات
```

---

### 🛠 تقنيات التطوير

| الطبقة | التقنية |
|--------|---------|
| **الواجهة** | Next.js 14, React 18, TypeScript, TailwindCSS |
| **الويب 3** | wagmi 2.x, viem 2.x, MetaMask, WalletConnect |
| **الخادم** | Rust, Axum, alloy-rs, tokio-tungstenite |
| **العقود** | Solidity 0.8.20, Foundry (forge, cast, anvil) |
| **القروض** | Aave V3, Radiant V2, Spark Protocol (0%) |
| **التجميع** | Velora Market API (ex-ParaSwap) |
| **الغاز** | Flashbots MEV-Share, Pimlico, ZeroDev |
| **الذكاء** | OpenAI, Anthropic, Groq, Ollama, DeepSeek |

---

### 📡 مرجع API

| الطريقة | المسار | الوصف |
|---------|--------|-------|
| `GET` | `/api/health` | فحص الصحة |
| `POST` | `/api/scan` | مسح الفرص البسيطة |
| `POST` | `/api/scan/comprehensive` | مسح شامل لكل التوكنات |
| `POST` | `/api/all-prices` | كل أسعار البورصات |
| `POST` | `/api/all-opportunities` | كل استراتيجيات المراجحة |
| `POST` | `/api/liquidity` | توزيع السيولة |
| `GET` | `/api/bubbles` | بيانات فقاعات العملات |
| `GET` | `/api/dashboard` | بيانات لوحة التحكم |
| `GET` | `/api/chains` | عرض السلاسل |
| `GET` | `/api/dexes/{chainId}` | عرض بورصات سلسلة |
| `POST` | `/api/paper/start` | بدء التداول الورقي |
| `POST` | `/api/paper/stop` | إيقاف + نتائج |
| `GET` | `/api/paper/status` | حالة التداول الورقي |
| `POST` | `/api/paper/backtest` | اختبار 50 صفقة |
| `POST` | `/api/portfolio/config` | تحديث المحفظة |
| `GET` | `/api/portfolio/status` | حالة المحفظة |
| `POST` | `/api/mev/analyze` | تحليل خطر MEV |
| `POST` | `/api/alerts/test` | إرسال إشعار اختباري |
| `POST` | `/api/rules/evaluate` | تقييم القواعد |
| `POST` | `/api/splitter/calculate` | حساب تقسيم الربح |
| `POST` | `/api/gas/recommend` | توصية الغاز |
| `POST` | `/api/bot/start` | بدء البوت |
| `POST` | `/api/bot/stop` | إيقاف البوت |
| `GET` | `/api/bot/logs` | سجل البوت |
| `WS` | `/ws` | اتصال WebSocket فوري |

---

### 📜 العقد الذكي

**`ZeroRiskArb.sol`** — عقد التنفيذ الأساسي:

```
executeArbitrage(
  flashLoanSource,  // Spark | Aave | Radiant
  flashLoanAmount,  // المبلغ المقترض
  tokenToBuy,       // عنوان العملة للشراء
  tokenToSell,      // عنوان العملة للبيع
  swapData,         // بيانات Velora Augustus
  minProfit         // أدنى ربح مقبول
)

الأمان:
  if (الرصيد < القرض + الرسوم + الربح الأدنى) → إلغاء
  → المستخدم لا يخسر شيئًا (0$)
```

---

## 🚀 Quick Start / البدء السريع

```bash
# Clone / استنساخ
git clone https://github.com/chemrah/zero-cap-arb.git
cd zero-cap-arb

# Backend / الخادم
cd backend
cargo run --release

# Frontend / الواجهة (نافذة ثانية)
cd frontend
npm install
npm run dev

# Smart Contracts / العقود (نافذة ثالثة)
cd contracts
forge build
forge test
```

Open **http://localhost:3000** — Connect MetaMask → Scan → Execute.

افتح **http://localhost:3000** — اربط MetaMask → امسح → نفذ.

---

## 📄 License / الترخيص

MIT License — freely use, modify, and distribute.

---

<p align="center">
  <b>Zero-Capital Arbitrage</b> — <i>A complete DeFi arbitrage operating system, from radar to execution to profit splitting.</i>
</p>
<p align="center" dir="rtl">
  <b>مراجحة بدون رأس مال</b> — <i>نظام تشغيل متكامل للمراجحة في التمويل اللامركزي، من الرادار إلى التنفيذ إلى تقسيم الأرباح.</i>
</p>
