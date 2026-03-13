import { createPublicClient, http, parseAbi, formatUnits } from "viem";
import { base } from "viem/chains";
import { readActivityLog, ActivityEntry } from "./activity.js";

// USDC on Base mainnet
const USDC_ADDRESS = "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913" as const;
const USDC_DECIMALS = 6;

const usdcAbi = parseAbi([
  "function balanceOf(address account) external view returns (uint256)",
]);

export async function getUsdcBalance(walletAddress: string): Promise<number> {
  const client = createPublicClient({
    chain: base,
    transport: http(process.env.BASE_RPC_URL || "https://mainnet.base.org"),
  });

  const raw = await client.readContract({
    address: USDC_ADDRESS,
    abi: usdcAbi,
    functionName: "balanceOf",
    args: [walletAddress as `0x${string}`],
  });

  return parseFloat(formatUnits(raw, USDC_DECIMALS));
}

export function computeDailyRunRate(entries: ActivityEntry[]): number {
  const now = Date.now();
  const sevenDaysAgo = now - 7 * 24 * 60 * 60 * 1000;

  const recentCosts = entries
    .filter(
      (e) =>
        e.type === "submitted" &&
        e.cost_usd !== undefined &&
        new Date(e.timestamp).getTime() >= sevenDaysAgo
    )
    .map((e) => e.cost_usd ?? 0);

  if (recentCosts.length === 0) return 0;

  const totalCost = recentCosts.reduce((sum, c) => sum + c, 0);
  return totalCost / 7;
}

export function computeProjectedDaysRemaining(
  balance: number,
  dailyRunRate: number
): number {
  if (dailyRunRate <= 0) return Infinity;
  return balance / dailyRunRate;
}

export async function getBudgetStatus(
  repoRoot: string,
  walletAddress: string
): Promise<{
  balance: number;
  dailyRunRate: number;
  projectedDaysRemaining: number;
}> {
  const [balance, entries] = await Promise.all([
    getUsdcBalance(walletAddress),
    readActivityLog(repoRoot),
  ]);

  const dailyRunRate = computeDailyRunRate(entries);
  const projectedDaysRemaining = computeProjectedDaysRemaining(
    balance,
    dailyRunRate
  );

  return { balance, dailyRunRate, projectedDaysRemaining };
}
