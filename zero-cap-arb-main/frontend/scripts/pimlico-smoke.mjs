#!/usr/bin/env node
/**
 * ERC-4337 / Pimlico smoke test for the zero-cap backend proxy.
 *
 * Exercises the REAL gasless flow end-to-end (no browser, no MetaMask):
 *   1. GET  /api/pimlico/status/{chain}   — bundler configured + live entry points
 *   2. toSimpleSmartAccount (permissionless.js, EntryPoint v0.7, counterfactual address)
 *   3. createSmartAccountClient → prepareUserOperation:
 *      eth_estimateUserOperationGas + pm_sponsorUserOperation through the backend
 *      /api/pimlico/rpc/{chain} proxy (API key never leaves the backend)
 *   4. with --send: personal_sign of the userOpHash + eth_sendUserOperation +
 *      eth_getUserOperationReceipt (a real bundled transaction).
 *
 * Usage:
 *   node scripts/pimlico-smoke.mjs [chainId] [--send]
 *   node scripts/pimlico-smoke.mjs 42161           # estimate + sponsor only (no state change)
 *   node scripts/pimlico-smoke.mjs 42161 --send    # submits a real (harmless no-op) UserOperation
 *
 * The owner key is read from backend/.env (PRIVATE_KEY) — the EOA that owns the smart
 * account. The default call is a no-op (0-value call to 0x...dEaD).
 */
import { readFileSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { http, encodeFunctionData, createPublicClient } from 'viem';
import { estimateFeesPerGas } from 'viem/actions';
import { privateKeyToAccount } from 'viem/accounts';
import { mainnet, arbitrum, optimism, polygon, bsc, avalanche } from 'viem/chains';
import { createPaymasterClient } from 'viem/account-abstraction';
import { toSimpleSmartAccount } from 'permissionless/accounts';
import { createSmartAccountClient } from 'permissionless';

const __dirname = dirname(fileURLToPath(import.meta.url));
const CHAINS = { 1: mainnet, 42161: arbitrum, 10: optimism, 137: polygon, 56: bsc, 43114: avalanche };

const chainId = Number(process.argv.find((a) => /^\d+$/.test(a)) ?? 42161);
const doSend = process.argv.includes('--send');
const chain = CHAINS[chainId];
if (!chain) {
  console.error(`Unsupported chain ${chainId}. Supported: ${Object.keys(CHAINS).join(', ')}`);
  process.exit(2);
}

// Read backend/.env (dotenvy-style: KEY=VALUE lines), overridden by process.env.
function readBackendEnv() {
  const envPath = resolve(__dirname, '..', '..', 'backend', '.env');
  const out = {};
  try {
    for (const line of readFileSync(envPath, 'utf8').split(/\r?\n/)) {
      const m = /^([A-Z0-9_]+)=(.*)$/.exec(line.trim());
      if (m) out[m[1]] = m[2].replace(/^["']|["']$/g, '');
    }
  } catch {
    /* fall back to process.env */
  }
  return out;
}
const env = { ...readBackendEnv(), ...process.env };

const API_BASE = env.API_BASE ?? 'http://127.0.0.1:3001';
const PRIVATE_KEY = env.PRIVATE_KEY;
if (!PRIVATE_KEY) {
  console.error('PRIVATE_KEY not set (backend/.env or environment)');
  process.exit(2);
}

const hex = (bn) => `0x${BigInt(bn).toString(16)}`;

console.log(`chain=${chainId} (${chain.name})  send=${doSend}  backend=${API_BASE}`);

// 1. Bundler status via backend (checks PIMLICO_API_KEY and live entry points).
const status = await (await fetch(`${API_BASE}/api/pimlico/status/${chainId}`)).json();
console.log('status:', JSON.stringify(status));
if (!status.configured) {
  console.error('PIMLICO_API_KEY not configured in backend/.env');
  process.exit(2);
}

// 2. Counterfactual SimpleAccount owned by PRIVATE_KEY (EntryPoint v0.7).
const owner = privateKeyToAccount(PRIVATE_KEY.startsWith('0x') ? PRIVATE_KEY : `0x${PRIVATE_KEY}`);
const publicClient = createPublicClient({ chain, transport: http() });
const account = await toSimpleSmartAccount({ client: publicClient, owner, index: 0n });
console.log(`owner (EOA):        ${owner.address}`);
console.log(`smart account:      ${account.address}`);
console.log(`entry point:        ${account.entryPoint.address} (v${account.entryPoint.version})`);
const onChain = await publicClient.getBytecode({ address: account.address });
console.log(`account deployed:   ${onChain !== undefined && onChain !== '0x'}`);

// 3. Harmless no-op call: execute(0x...dEaD, 0, 0x).
const executeAbi = [
  {
    type: 'function',
    name: 'execute',
    stateMutability: 'payable',
    inputs: [
      { name: 'dest', type: 'address' },
      { name: 'value', type: 'uint256' },
      { name: 'func', type: 'bytes' },
    ],
    outputs: [],
  },
];
const calls = [
  {
    to: '0x000000000000000000000000000000000000dEaD',
    value: 0n,
    data: encodeFunctionData({
      abi: executeAbi,
      functionName: 'execute',
      args: ['0x000000000000000000000000000000000000dEaD', 0n, '0x'],
    }),
  },
];

// 4. Bundler + paymaster through the backend proxy (API key stays server-side).
const bundlerUrl = `${API_BASE}/api/pimlico/rpc/${chainId}`;
const paymaster = createPaymasterClient({ transport: http(bundlerUrl) });
const client = createSmartAccountClient({
  account,
  chain,
  paymaster,
  bundlerTransport: http(bundlerUrl),
  userOperation: {
    // The bundler proxy is bundler-only and rejects eth_maxPriorityFeePerGas /
    // eth_feeHistory, so viem's default fee step silently yields no fees
    // (prepareUserOperation.js:181-182). Prefer Pimlico's own gas-price oracle: on L2s
    // the execution node reports maxPriorityFeePerGas = 0, which eth_sendUserOperation
    // rejects with "-32602: maxPriorityFeePerGas must be at least N".
    estimateFeesPerGas: async ({ bundlerClient }) => {
      try {
        const res = await bundlerClient.request({
          method: 'pimlico_getUserOperationGasPrice',
          params: [],
        });
        if (res?.standard?.maxFeePerGas && res?.standard?.maxPriorityFeePerGas) {
          return {
            maxFeePerGas: BigInt(res.standard.maxFeePerGas),
            maxPriorityFeePerGas: BigInt(res.standard.maxPriorityFeePerGas),
          };
        }
      } catch {
        // no Pimlico extension — fall back to the execution node below
      }
      const fees = await estimateFeesPerGas(publicClient, { type: 'eip1559' });
      return {
        maxFeePerGas: 2n * fees.maxFeePerGas,
        maxPriorityFeePerGas: 2n * fees.maxPriorityFeePerGas,
      };
    },
  },
});

// 5a. Estimate + sponsor (no submission): proves eth_estimateUserOperationGas and
//     pm_sponsorUserOperation work through the proxy with the configured key.
const prepared = await client.prepareUserOperation({ account, calls });
console.log('prepared user operation:');
console.log(
  JSON.stringify(
    {
      sender: prepared.sender ?? account.address,
      nonce: hex(prepared.nonce ?? 0n),
      callGasLimit: hex(prepared.callGasLimit ?? 0n),
      verificationGasLimit: hex(prepared.verificationGasLimit ?? 0n),
      preVerificationGas: hex(prepared.preVerificationGas ?? 0n),
      maxFeePerGas: hex(prepared.maxFeePerGas ?? 0n),
      maxPriorityFeePerGas: hex(prepared.maxPriorityFeePerGas ?? 0n),
      paymaster: prepared.paymaster ?? null,
      paymasterVerificationGasLimit:
        prepared.paymasterVerificationGasLimit != null
          ? hex(prepared.paymasterVerificationGasLimit)
          : null,
      paymasterPostOpGasLimit:
        prepared.paymasterPostOpGasLimit != null
          ? hex(prepared.paymasterPostOpGasLimit)
          : null,
      factory: prepared.factory ?? null,
    },
    null,
    2
  )
);
console.log(
  prepared.paymaster
    ? 'SPONSORED: paymaster attached — gasless path is active for this account.'
    : 'NO PAYMASTER: Pimlico returned no sponsorship for this op (no policy covers it); a submitted op would need the account to prefund gas (AA21 otherwise).'
);

if (!doSend) {
  console.log('\nOK (estimate + sponsor). Re-run with --send to submit the UserOperation.');
  process.exit(0);
}

// 5b. Sign + submit for real (eth_sendUserOperation) and wait for the receipt.
const { id: userOpHash } = await client.sendCalls({ calls });
console.log(`userOpHash: ${userOpHash}`);
const receipt = await client.waitForUserOperationReceipt({ hash: userOpHash });
console.log(`bundled tx: ${receipt.receipt.transactionHash}`);
console.log(`success:    ${receipt.success}${receipt.reason ? ` reason=${receipt.reason}` : ''}`);
console.log(`actualGasCost: ${receipt.actualGasCost} wei  block: ${receipt.receipt.blockNumber}`);
console.log(
  receipt.success
    ? 'OK — real ERC-4337 UserOperation bundled on-chain.'
    : 'UserOperation mined but the call reverted (honest failure).'
);
