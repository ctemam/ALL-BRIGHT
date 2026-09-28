# إعداد المشروع – Setup Guide

## 1. إنشاء ملفات البيئة

### الملف الأول: `backend/.env`

```bash
# أنشئ الملف
cd zero-cap-arb
cp .env.example backend/.env
```

**افتـح `backend/.env`** وحط هادا:

```env
# ─── RPC URLs ───────────────────────────────────────
# منين تجيبهم: https://infura.io أو https://alchemy.com
# سجل → Create API Key → نسخ الرابط
#
# ✅ تقدر تستعمل Infura و Alchemy بجوج!
# طريقتين:
#
# الطريقة 1: فاصلة بين كل URL
#   ETH_RPC_URL="https://eth-mainnet.g.alchemy.com/v2/KEY,https://mainnet.infura.io/v3/KEY"
#
# الطريقة 2: رقم الـ fallback (KEY_1, KEY_2, KEY_3)
#   ETH_RPC_URL=https://eth-mainnet.g.alchemy.com/v2/KEY
#   ETH_RPC_URL_1=https://mainnet.infura.io/v3/KEY
#
# كود الباك إند يجرب الأول ولا فلان تاع الثاني أوتوماتيك.

ETH_RPC_URL=https://mainnet.infura.io/v3/هنا_KEY_DYALAK
ARB_RPC_URL=https://arbitrum-mainnet.infura.io/v3/هنا_KEY_DYALAK
OP_RPC_URL=https://optimism-mainnet.infura.io/v3/هنا_KEY_DYALAK
POLY_RPC_URL=https://polygon-mainnet.infura.io/v3/هنا_KEY_DYALAK
BSC_RPC_URL=https://bsc-mainnet.infura.io/v3/هنا_KEY_DYALAK
AVAX_RPC_URL=https://avalanche-mainnet.infura.io/v3/هنا_KEY_DYALAK

# ─── Velora API (ex-ParaSwap) ─────────────────────────
# اختياري: https://developers.velora.xyz
VELORA_API_KEY=

# ─── Server ───────────────────────────────────────────
PORT=3001

# ─── Flashbots (للإنتاج) ──────────────────────────────
# https://docs.flashbots.net
FLASHBOOTS_RELAY=https://relay.flashbots.net
FLASHBOOTS_AUTH_KEY=

# ─── Pimlico / ZeroDev (للإنتاج) ──────────────────────
# https://pimlico.io  |  https://zerodev.app
PIMLICO_API_KEY=
ZERODEV_PROJECT_ID=

# ─── Wallet (للإنتاج) ─────────────────────────────────
PRIVATE_KEY=
```

---

### الملف الثاني: `frontend/.env.local`

```bash
# أنشئ الملف
cd zero-cap-arb
echo NEXT_PUBLIC_API_URL=http://localhost:3001 > frontend/.env.local
echo NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID= >> frontend/.env.local
```

**افتـح `frontend/.env.local`** وحط هادا:

```env
# ─── Backend URL ─────────────────────────────────────
NEXT_PUBLIC_API_URL=http://localhost:3001

# ─── WalletConnect Project ID ────────────────────────
# منين تجيبه: https://cloud.walletconnect.com
# سجل → New Project → نسخ Project ID
NEXT_PUBLIC_WALLETCONNECT_PROJECT_ID=هنا_PROJECT_ID
```

---

## 2. فين كاينين الملفات فالمشروع

```
zero-cap-arb/
│
├── 📄 backend/.env          ← <=== RPC URLs + Velora + Flashbots هنا
├── 📄 frontend/.env.local    ← <=== WalletConnect هنا
│
├── backend/                 ← كود Rust
├── contracts/               ← العقود الذكية
├── frontend/                ← الواجهة
└── README.md                ← الشرح الكامل
```

---

## 3. ملاحظة مهمة

هاد الملفات **(.env و .env.local)** راهم مكتوبين فـ `.gitignore` → مايطلعوش لـ GitHub. مفاتيحك بقاو آمنين.

---

## 4. بعد ما تحط المفاتيح، شغل التطبيق

```bash
# Terminal 1: Backend
cd zero-cap-arb/backend
cargo run --release

# Terminal 2: Frontend
cd zero-cap-arb/frontend
npm install
npm run dev
```

فتح `http://localhost:3000` وابدأ.
