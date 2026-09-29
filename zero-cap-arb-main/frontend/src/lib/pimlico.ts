import { http, type Address, type Chain, type Hex } from 'viem';
import { estimateFeesPerGas } from 'viem/actions';
import { createPaymasterClient, type UserOperationReceipt } from 'viem/account-abstraction';
import { toSimpleSmartAccount } from 'permissionless/accounts';
import { createSmartAccountClient } from 'permissionless';
import { getPublicClient } from 'wagmi/actions';
import { SUPPORTED_CHAINS, wagmiConfig } from '@/lib/wagmi';

/**
 * Pimlico (ERC-4337) client-side helpers.
 *
 * The Pimlico API key lives only in `backend/.env`: every bundler/paymaster JSON-RPC
 * call is sent to the backend proxy `POST /api/pimlico/rpc/{chainId}`, which injects
 * `PIMLICO_API_KEY` before forwarding to `https://api.pimlico.io/v2/...`.
 *
 * The smart account is the canonical eth-infinitism **SimpleAccount** on EntryPoint
 * v0.7 (permissionless.js defaults: factory `0x91E60e0613810449d098b0b5Ec8b51A0FE8c8985`,
 * deterministic CREATE2 address, deployed by the bundler on the first UserOperation).
 */
const API_BASE =
  process.env.NEXT_PUBLIC_API_URL ??
  process.env.NEXT_PUBLIC_API_BASE ??
  'http://localhost:3001';

/** Owner type accepted by permissionless.js `toSimpleSmartAccount`. */
export type SmartAccountOwner = Parameters<typeof toSimpleSmartAccount>[0]['owner'];

export interface PimlicoStatus {
  configured: boolean;
  chain_id: number;
  entry_point_v06: string;
  entry_point_v07: string;
  entry_point_v08: string;
  entry_points?: string[];
  entry_points_error?: string;
}

/** Bundler + paymaster transport URL (through the backend proxy). */
export function pimlicoRpcUrl(chainId: number): string {
  return `${API_BASE}/api/pimlico/rpc/${chainId}`;
}

/** Whether the backend has PIMLICO_API_KEY and which entry points are live on the chain. */
export async function getPimlicoStatus(chainId: number): Promise<PimlicoStatus> {
  const res = await fetch(`${API_BASE}/api/pimlico/status/${chainId}`);
  if (!res.ok) {
    throw new Error(`Pimlico status check failed: HTTP ${res.status}`);
  }
  return (await res.json()) as PimlicoStatus;
}

function getChain(chainId: number): Chain {
  const chain = SUPPORTED_CHAINS.find((c) => c.id === chainId);
  if (!chain) {
    throw new Error(`Chain ${chainId} is not supported by this app`);
  }
  return chain;
}

export interface SmartAccountHandle {
  chain: Chain;
  account: Awaited<ReturnType<typeof toSimpleSmartAccount>>;
  /**
   * Real execution RPC (a full node, NOT the bundler). Required because the Pimlico
   * endpoint is bundler-only and rejects `eth_maxPriorityFeePerGas` / `eth_feeHistory`,
   * so fee data can only be fetched from an execution node.
   */
  publicClient: NonNullable<ReturnType<typeof getPublicClient>>;
}

/**
 * Counterfactual SimpleAccount owned by the connected wallet (EntryPoint v0.7).
 * `account.address` is valid before deployment — the bundler deploys it with the
 * first UserOperation's `factory`/`factoryData`.
 */
export async function createSmartAccount(
  chainId: number,
  owner: SmartAccountOwner
): Promise<SmartAccountHandle> {
  const chain = getChain(chainId);
  const publicClient = getPublicClient(wagmiConfig, {
    chainId: chainId as (typeof SUPPORTED_CHAINS)[number]['id'],
  });
  if (!publicClient) {
    throw new Error(`No RPC configured for chain ${chainId}`);
  }
  const account = await toSimpleSmartAccount({
    client: publicClient,
    owner,
    index: 0n,
  });
  return { chain, account, publicClient };
}

export interface GaslessCall {
  to: Address;
  value?: bigint;
  data: Hex;
}

export interface GaslessResult {
  /** ERC-4337 UserOperation hash returned by `eth_sendUserOperation`. */
  userOpHash: Hex;
  /** Bundled receipt: contains the real on-chain transaction hash and success flag. */
  receipt: UserOperationReceipt;
  /** Smart account address (counterfactual until the first op deploys it). */
  accountAddress: Address;
}

/**
 * Build, estimate, sponsor, sign and submit an ERC-4337 UserOperation, then wait for
 * the bundled on-chain receipt:
 *
 * 1. `eth_estimateUserOperationGas` (bundler)
 * 2. `pm_sponsorUserOperation` (Pimlico paymaster — gasless when a sponsorship policy
 *    exists on the Pimlico project; otherwise the op must prefund from the account)
 * 3. `eth_sendUserOperation` (bundler) — signature is `personal_sign` of the userOpHash
 * 4. `eth_getUserOperationReceipt` — real transaction hash + success flag
 *
 * All calls go through the backend `/api/pimlico/rpc/{chainId}` proxy.
 */
export async function sendGaslessTransaction(args: {
  chainId: number;
  owner: SmartAccountOwner;
  calls: GaslessCall[];
}): Promise<GaslessResult> {
  const { chain, account, publicClient } = await createSmartAccount(args.chainId, args.owner);
  const transport = http(pimlicoRpcUrl(args.chainId));

  const paymaster = createPaymasterClient({ transport });
  const bundlerClient = createSmartAccountClient({
    account,
    chain,
    paymaster,
    bundlerTransport: transport,
    userOperation: {
      // The Pimlico endpoint is bundler-only: it rejects `eth_maxPriorityFeePerGas` and
      // `eth_feeHistory`. viem's default fee step catches that failure and returns
      // `undefined` (prepareUserOperation.js:181-182), so the UserOperation is sent with
      // NO maxFeePerGas/maxPriorityFeePerGas and Pimlico fails pm_getPaymasterStubData
      // with "expected ... undefined at params[0].userOp.maxFeePerGas".
      // This hook takes priority over that fallback.
      estimateFeesPerGas: async ({ bundlerClient }) => {
        // Preferred: Pimlico's own gas-price oracle. Required on L2s, where the
        // execution node reports maxPriorityFeePerGas = 0 and `eth_sendUserOperation`
        // then fails with "-32602: maxPriorityFeePerGas must be at least N".
        try {
          // `pimlico_getUserOperationGasPrice` is a Pimlico extension and is not part of
          // viem's BundlerRpcSchema, so the request is typed explicitly.
          const request = bundlerClient.request as (args: {
            method: string;
            params: unknown[];
          }) => Promise<{ standard?: { maxFeePerGas: Hex; maxPriorityFeePerGas: Hex } }>;
          const res = await request.call(bundlerClient, {
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
          // Endpoint does not implement the Pimlico extension — fall through.
        }
        // Fallback: estimate from the real execution node, keeping the 2x buffer that
        // bundler fee prechecks require.
        const fees = await estimateFeesPerGas(publicClient, { chain, type: 'eip1559' });
        return {
          maxFeePerGas: 2n * fees.maxFeePerGas,
          maxPriorityFeePerGas: 2n * fees.maxPriorityFeePerGas,
        };
      },
    },
  });

  const { id } = await bundlerClient.sendCalls({
    calls: args.calls.map((c) => ({ to: c.to, value: c.value, data: c.data })),
  });
  // permissionless types `id` as plain string; viem expects a Hex hash downstream.
  const userOpHash = id as Hex;

  const receipt = await bundlerClient.waitForUserOperationReceipt({ hash: userOpHash });

  return { userOpHash, receipt, accountAddress: account.address };
}
