#!/usr/bin/env node
/**
 * Capability probe for the backend ERC-4337 proxy (POST /api/pimlico/rpc/{chain}).
 *
 * Answers the question that broke gasless execution: which JSON-RPC methods does the
 * proxied Pimlico endpoint actually answer? viem's prepareUserOperation fills
 * maxFeePerGas/maxPriorityFeePerGas with a `catch { return undefined }` fallback
 * (node_modules/viem/.../prepareUserOperation.js:181-182) -- when the fee step fails
 * silently, the op is sent WITHOUT fee fields and Pimlico rejects
 * pm_getPaymasterStubData with "expected ... undefined at params[0].userOp.maxFeePerGas".
 *
 * Usage: node scripts/pimlico-probe.mjs [chainId]
 */
const API_BASE = process.env.API_BASE ?? 'http://127.0.0.1:3001';
const chainId = Number(process.argv.find((a) => /^\d+$/.test(a)) ?? 42161);
const url = `${API_BASE}/api/pimlico/rpc/${chainId}`;

const USER_OP = {
  sender: '0x000000000000000000000000000000000000dEaD',
  nonce: '0x0',
  callData: '0x',
  factory: '0x91E60e0613810449d098b0b5Ec8b51A0FE8c8985',
  factoryData: '0x',
  callGasLimit: '0x0',
  verificationGasLimit: '0x0',
  preVerificationGas: '0x0',
  maxFeePerGas: '0x1',
  maxPriorityFeePerGas: '0x1',
  paymaster: '0x0000000071727De22E5E9d8BAf0edAc6f37da032',
  paymasterVerificationGasLimit: '0x0',
  paymasterPostOpGasLimit: '0x0',
  signature: '0x',
};
// EntryPoint v0.7 rejects the v0.6-only `initCode` key, so keep the probe op v0.7-shaped.
const EP07 = '0x0000000071727De22E5E9d8BAf0edAc6f37da032';

const PROBES = [
  ['eth_chainId', []],
  ['eth_blockNumber', []],
  ['eth_gasPrice', []],
  ['eth_maxPriorityFeePerGas', []],
  ['eth_feeHistory', ['0x1', 'latest', []]],
  ['eth_getBlockByNumber', ['latest', false]],
  ['eth_supportedEntryPoints', []],
  ['eth_estimateUserOperationGas', [USER_OP, EP07]],
  ['pimlico_getUserOperationGasPrice', []],
  ['pm_getPaymasterStubData', [USER_OP, EP07, '0xa4b1', null]],
];

console.log(`probing ${url}\n`);

const results = new Map();
for (const [method, params] of PROBES) {
  let verdict;
  let detail = '';
  try {
    // Send `params` only when non-empty: some endpoints reject an explicit empty array.
    const payload = { jsonrpc: '2.0', id: 1, method };
    if (params.length > 0) payload.params = params;
    const res = await fetch(url, {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(payload),
    });
    const text = await res.text();
    let body;
    try {
      body = JSON.parse(text);
    } catch {
      body = { raw: text };
    }
    if (body.error) {
      // -32601 = method not supported by the endpoint.
      verdict = body.error.code === -32601 ? 'UNSUPPORTED' : 'RPC ERROR';
      detail = body.error.message;
    } else if (!res.ok) {
      verdict = `HTTP ${res.status}`;
      detail = text.slice(0, 160);
    } else {
      verdict = 'SUPPORTED';
      const out = body.result;
      if (method === 'eth_feeHistory' && out) {
        detail = `baseFeePerGas[0]=${out.baseFeePerGas?.[0]} rewardCount=${out.reward?.length ?? 0}`;
      } else if (method === 'eth_maxPriorityFeePerGas') {
        detail = `${out} wei`;
      } else if (method === 'eth_getBlockByNumber') {
        detail = `number=${out?.number} baseFeePerGas=${out?.baseFeePerGas}`;
      } else if (method === 'eth_estimateUserOperationGas') {
        detail = `callGasLimit=${out?.callGasLimit} preVerificationGas=${out?.preVerificationGas}`;
      } else if (method === 'pimlico_getUserOperationGasPrice') {
        detail = `result=${JSON.stringify(out)}`;
      } else {
        detail = String(JSON.stringify(out)).slice(0, 120);
      }
    }
  } catch (e) {
    verdict = 'NETWORK FAIL';
    detail = e.message;
  }
  results.set(method, verdict);
  console.log(`${method.padEnd(30)} ${verdict.padEnd(12)} ${detail}`);
}

// The conclusion that decides the fix.
console.log('\n--- diagnosis ---');
const feeOk = ['eth_maxPriorityFeePerGas', 'eth_feeHistory'].filter((m) =>
  results.get(m) === 'SUPPORTED'
).length;
if (feeOk === 2) {
  console.log('Both EIP-1559 fee methods work through the proxy.');
  console.log('If fees were still missing, the failure is in the viem step, not the endpoint.');
} else {
  console.log(`Fee methods supported: ${feeOk}/2 (eth_maxPriorityFeePerGas, eth_feeHistory).`);
  console.log('The bundler endpoint is NOT a full node: viem estimateFeesPerGas throws,');
  console.log('prepareUserOperation swallows it (prepareUserOperation.js:181-182) and sends');
  console.log('a userOp with no maxFeePerGas/maxPriorityFeePerGas -> Pimlico -32601 validation error.');
  console.log('FIX: pass explicit maxFeePerGas + maxPriorityFeePerGas, or attach a real-node');
  console.log('publicClient, so the fee step never depends on bundler fee methods.');
}
