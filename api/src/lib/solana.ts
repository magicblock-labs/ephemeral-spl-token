import {
  compileLegacyTransactionToV0,
  DELEGATION_PROGRAM_ID,
  DelegationStatus,
  delegateTransferQueueIx,
  delegateBufferPdaFromDelegatedAccountAndOwnerProgram,
  delegationMetadataPdaFromDelegatedAccount,
  delegationRecordPdaFromDelegatedAccount,
  deriveRentPda,
  deriveTransferQueue,
  delegateSpl,
  EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  ensureTransferQueueCrankIx,
  getDelegationRecord,
  getAuthToken,
  initRentPdaIx,
  initTransferQueueIx,
  magicFeeVaultPdaFromValidator,
  PERMISSION_PROGRAM_ID,
  permissionPdaFromAccount,
  transferSpl,
  undelegateIx,
  withdrawSpl, initVaultIx, initVaultAtaIx, delegateEphemeralAtaIx, deriveVault, deriveEphemeralAta, deriveVaultAta,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import { sha256 } from "@noble/hashes/sha256";
import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SendOptions,
  SystemProgram,
  Transaction,
  TransactionInstruction,
} from "@solana/web3.js";
import nacl from "tweetnacl";

import type { AppEnv } from "../env";
import { ApiError } from "./errors";
import {
  BalanceRequest,
  BalanceResponse,
  DepositRequest,
  InitializeMintRequest,
  InitializeMintResponse,
  MintInitializationRequest,
  MintInitializationResponse,
  StealthPoolRequest,
  StealthPoolResponse,
  StealthPoolStatusRequest,
  StealthPoolStatusResponse,
  TransactionResponse,
  TransferRequest,
  TransferQueueEnsureCrankRequest,
  TransferQueueEnsureCrankResponse,
  UndelegateEphemeralAtaRequest,
  UndelegateEphemeralAtaResponse,
  WithdrawRequest,
} from "../routes/spl/spl.schemas";
import {
  SendTransactionRequest,
  SendTransactionResponse,
} from "../routes/transaction.schemas";
import { getCachedAddressLookupTable, getConnection } from "./rpc-cache";

export const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);

export const TOKEN_2022_PROGRAM_ID = new PublicKey(
  "TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb",
);

export const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);

const NATIVE_MINT = new PublicKey(
  "So11111111111111111111111111111111111111112",
);

const MEMO_PROGRAM_ID = new PublicKey(
  "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
);

const DEFAULT_DEPOSIT_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEFAULT_DEPOSIT_DEVNET_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const MAINNET_USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
const DEFAULT_FALLBACK_VALIDATOR = new PublicKey(
  "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
);
const TEE_VALIDATOR = new PublicKey(
  "MTEWGuqxUpYZGFJQcp8tLN7x5v9BSeoFHYWQQ3n3xzo",
);
const DEVNET_TEE_RPC_URL = "https://devnet-tee.magicblock.app";
const MAINNET_TEE_RPC_URL = "https://mainnet-tee.magicblock.app";
const DELEGATION_ROUTER_RPC_URLS = {
  mainnet: "https://router.magicblock.app/",
  devnet: "https://devnet-router.magicblock.app/",
} as const;
const TRANSFER_QUEUE_RENT_LAMPORTS = LAMPORTS_PER_SOL / 50;
const PRIVATE_TRANSFER_MAX_DELAY_MS_LIMIT = 10n * 60n * 1000n;
const TRANSFER_QUEUE_RECENT_SIGNATURE_LIMIT = 5;
const TRANSFER_QUEUE_STALE_MS = 60_000;
const TRANSFER_QUEUE_AUTH_ERROR_FORCE_INTERVAL = 100;
const SOLANA_WIRE_TRANSACTION_SIZE_LIMIT = 1232;
const UPDATE_STEALTH_POOL_DISCRIMINATOR = 21;
const ENSURE_STEALTH_POOL_DELEGATED_DISCRIMINATOR = 22;
const DEPOSIT_AND_QUEUE_TRANSFER_DISCRIMINATOR = 16;
const DEPOSIT_AND_QUEUE_TRANSFER_LEGACY_ACCOUNT_COUNT = 12;
const DEPOSIT_AND_QUEUE_TRANSFER_GROUP_RECEIPT_INDEX = 9;
const STEALTH_POOL_SEED = Buffer.from("stealth_pool");
const STEALTH_POOL_LONG_HANDLE_HASH_DOMAIN = Buffer.from("stealth_pool_handle");
const STEALTH_POOL_SPLIT_ACROSS_KEYS_FLAG = 1 << 0;
const MAX_STEALTH_HANDLE_BYTES = 255;
const STEALTH_HANDLE_STORAGE_BYTES = 1 + MAX_STEALTH_HANDLE_BYTES;
const MAX_STEALTH_HANDLE_SEED_BYTES = 32;
const MAX_INLINE_STEALTH_HANDLE_SEED_BYTES = 2 * MAX_STEALTH_HANDLE_SEED_BYTES;
// Keep these defaults aligned with scripts/create-private-transfer-lut.js. Updating them requires a redeploy.
const PRIVATE_BASE_TO_BASE_TRANSFER_LOOKUP_TABLES = {
  mainnet: new PublicKey("54M1BrqVSg1UGTmhH44gQPsPVyuMpmcVBkaY2wYNSVZB"),
  devnet: new PublicKey("E26JGdRsdKkGe6oRU4Un24agZjBF2Bg9z1ctfZByETRo"),
} as const;
const PRIVATE_TRANSFER_SETUP_LAMPORTS = 2_039_280n;
const PRIVATE_TRANSFER_FEE_BASIS_POINTS = 10n;
const BASIS_POINTS_FACTOR = 10_000n;
const GASLESS_RELAY_FEE_MICRO_USDC = 200_000n; // 0.2 USDC/USDT
const GASLESS_STABLECOIN_MIN_AMOUNT = 500_000n; // 0.5 USDC/USDT

const validatorCache = new Map<string, Promise<PublicKey | undefined>>();
const transferQueueAuthErrorCounts = new Map<string, number>();

type SendTarget = "base" | "ephemeral";

type BlockhashResult = {
  blockhash: string;
  lastValidBlockHeight: number;
};

type RpcConfig = {
  baseRpcUrl: string;
  ephemeralRpcUrl: string;
  transferQueueCrankRpcUrl: string;
  cluster: "mainnet" | "devnet" | "custom";
  teeRpcUrl?: string;
};

type ClusterConfigEnvVar
  = "BASE_DEVNET_RPC_URL"
    | "EPHEMERAL_DEVNET_RPC_URL"
    | "EPHEMERAL_TEE_RPC_URL"
    | "EPHEMERAL_DEVNET_TEE_RPC_URL";

type RpcIdentityResponse = {
  result?: {
    identity?: string;
  };
  error?: {
    message?: string;
  };
};

type DelegationStatusRpcResponse = {
  result?: {
    isDelegated?: boolean;
    fqdn?: unknown;
  };
  error?: {
    message?: string;
  };
};

type DelegationEndpointResolution = {
  endpoint?: string;
  error?: string;
  isDelegated?: boolean;
};

type BackgroundTaskScheduler = {
  waitUntil: (promise: Promise<unknown>) => void;
};

type TransferFees = NonNullable<TransactionResponse["fees"]>;
type EnsureTransferQueueCrankOptions = {
  force?: boolean;
};
type ProjectedWritableAta = {
  role: string;
  owner: PublicKey;
  mint: PublicKey;
  ata: PublicKey;
  eata: PublicKey;
  delegationRecord: PublicKey;
};

function getBaseConnection(config: RpcConfig) {
  return getConnection(config.baseRpcUrl);
}

function getConnectionWithOptionalAuthToken(rpcUrl: string, authToken?: string) {
  if (!authToken) {
    return getConnection(rpcUrl);
  }

  const url = new URL(rpcUrl);
  url.searchParams.set("token", authToken);
  return new Connection(url.toString(), "confirmed");
}

function getEphemeralConnection(config: RpcConfig, authToken?: string) {
  return getConnectionWithOptionalAuthToken(config.ephemeralRpcUrl, authToken);
}

async function createThrowawayAuthToken(rpcUrl: string) {
  const keypair = Keypair.generate();
  const { token } = await getAuthToken(
    rpcUrl,
    keypair.publicKey,
    async message => nacl.sign.detached(message, keypair.secretKey),
  );
  return token;
}

async function resolveMintTokenProgram(config: RpcConfig, mint: PublicKey) {
  let accountInfo: { owner: PublicKey } | null;
  try {
    accountInfo = await getBaseConnection(config).getAccountInfo(mint, "confirmed");
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to resolve mint token program", {
      mint: mint.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }

  if (!accountInfo) {
    throw new ApiError(400, "MINT_NOT_FOUND", "Mint account not found");
  }

  if (
    accountInfo.owner.equals(TOKEN_PROGRAM_ID)
    || accountInfo.owner.equals(TOKEN_2022_PROGRAM_ID)
  ) {
    return accountInfo.owner;
  }

  throw new ApiError(400, "UNSUPPORTED_TOKEN_PROGRAM", "Mint owner is not a supported token program", {
    mint: mint.toBase58(),
    owner: accountInfo.owner.toBase58(),
  });
}

function createClusterConfigError(cluster: string, missingVars: ClusterConfigEnvVar[]) {
  return new ApiError(
    500,
    "CONFIG_ERROR",
    `Missing worker environment variables for cluster=${cluster}`,
    {
      issues: missingVars.map(name => ({
        path: [name],
        message: `Required for cluster=${cluster}`,
      })),
      hint: `Set ${missingVars.join(" and ")} before using cluster=${cluster}.`,
    },
  );
}

function getGaslessSponsorKeypair(env: AppEnv) {
  if (!env.GASLESS_SPONSOR_SECRET_KEY) {
    throw new ApiError(
      503,
      "SPONSOR_UNAVAILABLE",
      "Gasless transfers are not configured",
    );
  }

  let secretKey: unknown;
  try {
    secretKey = JSON.parse(env.GASLESS_SPONSOR_SECRET_KEY);
  } catch {
    throw new ApiError(
      500,
      "CONFIG_ERROR",
      "GASLESS_SPONSOR_SECRET_KEY must be a JSON-encoded secret key array",
    );
  }

  if (!Array.isArray(secretKey) || secretKey.some(value => !Number.isInteger(value))) {
    throw new ApiError(
      500,
      "CONFIG_ERROR",
      "GASLESS_SPONSOR_SECRET_KEY must be a JSON-encoded secret key array",
    );
  }

  try {
    return Keypair.fromSecretKey(Uint8Array.from(secretKey));
  } catch {
    throw new ApiError(
      500,
      "CONFIG_ERROR",
      "GASLESS_SPONSOR_SECRET_KEY is not a valid Solana secret key",
    );
  }
}

export function resolveRpcConfig(env: AppEnv, cluster?: string): RpcConfig {
  const value = (cluster ?? env.CLUSTER).trim();
  const normalized = value?.toLowerCase();
  if (!value || normalized === "mainnet") {
    return {
      baseRpcUrl: env.BASE_RPC_URL,
      ephemeralRpcUrl: env.EPHEMERAL_RPC_URL,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_RPC_URL,
      cluster: "mainnet",
    };
  }

  if (normalized === "mainnet-private") {
    const missingVars = [
      ...(!env.EPHEMERAL_TEE_RPC_URL ? ["EPHEMERAL_TEE_RPC_URL" as const] : []),
    ];

    if (missingVars.length > 0) {
      throw createClusterConfigError("mainnet-private", missingVars);
    }

    return {
      baseRpcUrl: env.BASE_RPC_URL,
      ephemeralRpcUrl: env.EPHEMERAL_TEE_RPC_URL!,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_TEE_RPC_URL!,
      cluster: "mainnet",
      teeRpcUrl: env.EPHEMERAL_TEE_RPC_URL!,
    };
  }

  if (normalized === "devnet") {
    const missingVars = [
      ...(!env.BASE_DEVNET_RPC_URL ? ["BASE_DEVNET_RPC_URL" as const] : []),
      ...(!env.EPHEMERAL_DEVNET_RPC_URL ? ["EPHEMERAL_DEVNET_RPC_URL" as const] : []),
    ];

    if (missingVars.length > 0) {
      throw createClusterConfigError("devnet", missingVars);
    }

    return {
      baseRpcUrl: env.BASE_DEVNET_RPC_URL!,
      ephemeralRpcUrl: env.EPHEMERAL_DEVNET_RPC_URL!,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL ?? env.EPHEMERAL_DEVNET_RPC_URL!,
      cluster: "devnet",
    };
  }

  if (normalized === "devnet-private") {
    const missingVars = [
      ...(!env.BASE_DEVNET_RPC_URL ? ["BASE_DEVNET_RPC_URL" as const] : []),
      ...(!env.EPHEMERAL_DEVNET_TEE_RPC_URL ? ["EPHEMERAL_DEVNET_TEE_RPC_URL" as const] : []),
    ];

    if (missingVars.length > 0) {
      throw createClusterConfigError("devnet-private", missingVars);
    }

    return {
      baseRpcUrl: env.BASE_DEVNET_RPC_URL!,
      ephemeralRpcUrl: env.EPHEMERAL_DEVNET_TEE_RPC_URL!,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL ?? env.EPHEMERAL_DEVNET_TEE_RPC_URL!,
      cluster: "devnet",
      teeRpcUrl: env.EPHEMERAL_DEVNET_TEE_RPC_URL!,
    };
  }

  if (cluster === undefined && normalized === "custom") {
    return {
      baseRpcUrl: env.BASE_RPC_URL,
      ephemeralRpcUrl: env.EPHEMERAL_RPC_URL,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_RPC_URL,
      cluster: "custom",
    };
  }

  try {
    const url = new URL(value);

    if (!["http:", "https:"].includes(url.protocol)) {
      throw new Error("invalid protocol");
    }

    return {
      baseRpcUrl: url.toString(),
      ephemeralRpcUrl: env.EPHEMERAL_RPC_URL,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_RPC_URL,
      cluster: "custom",
    };
  } catch {
    throw new ApiError(400, "INVALID_CLUSTER", "cluster must be \"mainnet\", \"devnet\", \"mainnet-private\", \"devnet-private\", or a valid http(s) URL");
  }
}

function resolvePrivateBaseToBaseTransferLookupTableAddress(env: AppEnv, cluster: "mainnet" | "devnet") {
  const configuredAddress = cluster === "mainnet"
    ? parseConfigPublicKey(
        env.PRIVATE_BASE_TO_BASE_TRANSFER_MAINNET_LOOKUP_TABLE,
        "PRIVATE_BASE_TO_BASE_TRANSFER_MAINNET_LOOKUP_TABLE",
      )
    : parseConfigPublicKey(
        env.PRIVATE_BASE_TO_BASE_TRANSFER_DEVNET_LOOKUP_TABLE,
        "PRIVATE_BASE_TO_BASE_TRANSFER_DEVNET_LOOKUP_TABLE",
      );

  return configuredAddress ?? PRIVATE_BASE_TO_BASE_TRANSFER_LOOKUP_TABLES[cluster];
}

function parsePublicKey(value: string, fieldName: string) {
  try {
    return new PublicKey(value);
  } catch {
    throw new ApiError(400, "INVALID_PUBLIC_KEY", `Invalid ${fieldName}`);
  }
}

function tryParsePublicKey(value: string) {
  try {
    return new PublicKey(value);
  } catch {
    return undefined;
  }
}

function parseAmount(
  value: string | number,
  fieldName: string,
  options?: { allowZero?: boolean },
) {
  try {
    const allowZero = options?.allowZero ?? false;
    const amount = typeof value === "number"
      ? (() => {
          if (!Number.isSafeInteger(value) || value < 0 || (!allowZero && value === 0)) {
            throw new Error("invalid amount");
          }

          return BigInt(value);
        })()
      : BigInt(value);

    if (amount < 0n || (!allowZero && amount === 0n)) {
      throw new Error("invalid amount");
    }
    return amount;
  } catch {
    throw new ApiError(
      400,
      "INVALID_AMOUNT",
      options?.allowZero
        ? `${fieldName} must be a non-negative integer string`
        : `${fieldName} must be a positive integer string`,
    );
  }
}

function parseOptionalAmount(value: string | undefined, fieldName: string) {
  if (value === undefined) {
    return undefined;
  }

  try {
    return BigInt(value);
  } catch {
    throw new ApiError(400, "INVALID_AMOUNT", `${fieldName} must be an integer string`);
  }
}

function parseConfigPublicKey(value: string | undefined, fieldName: string) {
  if (!value) {
    return undefined;
  }

  try {
    return new PublicKey(value);
  } catch {
    throw new ApiError(500, "CONFIG_ERROR", `${fieldName} must be a valid public key`);
  }
}

function redactUrls(value: string) {
  return value.replace(/https?:\/\/[^\s)]+/g, "[redacted-url]");
}

function getSanitizedErrorMessage(error: unknown) {
  return redactUrls(error instanceof Error ? error.message : String(error));
}

function getAssociatedTokenAddressSync(
  mint: PublicKey,
  owner: PublicKey,
  allowOwnerOffCurve: boolean = false,
  programId: PublicKey = TOKEN_PROGRAM_ID,
  associatedTokenProgramId: PublicKey = ASSOCIATED_TOKEN_PROGRAM_ID,
) {
  if (!allowOwnerOffCurve && !PublicKey.isOnCurve(owner.toBuffer())) {
    throw new ApiError(400, "INVALID_OWNER", `Owner public key ${owner.toBase58()} is off-curve`);
  }

  const [ata] = PublicKey.findProgramAddressSync(
    [owner.toBuffer(), programId.toBuffer(), mint.toBuffer()],
    associatedTokenProgramId,
  );

  return ata;
}

function parseTokenAmount(accountInfo: { data: Buffer | Uint8Array }) {
  const data = Buffer.isBuffer(accountInfo.data)
    ? accountInfo.data
    : Buffer.from(accountInfo.data);

  if (data.length < 72) {
    return null;
  }

  return data.readBigUInt64LE(64);
}

function createMemoInstruction(memo: string) {
  return new TransactionInstruction({
    programId: MEMO_PROGRAM_ID,
    keys: [],
    data: Buffer.from(memo, "utf8"),
  });
}

function createAssociatedTokenAccountIdempotentInstruction(
  payer: PublicKey,
  associatedToken: PublicKey,
  owner: PublicKey,
  mint: PublicKey,
  programId: PublicKey = TOKEN_PROGRAM_ID,
  associatedTokenProgramId: PublicKey = ASSOCIATED_TOKEN_PROGRAM_ID,
) {
  return new TransactionInstruction({
    programId: associatedTokenProgramId,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: associatedToken, isSigner: false, isWritable: true },
      { pubkey: owner, isSigner: false, isWritable: false },
      { pubkey: mint, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: programId, isSigner: false, isWritable: false },
    ],
    data: Buffer.from([1]),
  });
}

function createSyncNativeInstruction(
  nativeAccount: PublicKey,
  programId: PublicKey = TOKEN_PROGRAM_ID,
) {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: nativeAccount, isSigner: false, isWritable: true },
    ],
    data: Buffer.from([17]),
  });
}

function createCloseTokenAccountInstruction(
  account: PublicKey,
  destination: PublicKey,
  authority: PublicKey,
  programId: PublicKey = TOKEN_PROGRAM_ID,
) {
  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: account, isSigner: false, isWritable: true },
      { pubkey: destination, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    data: Buffer.from([9]),
  });
}

async function createNativeSolWrapInstructionsIfNeeded(
  config: RpcConfig,
  sourceOwner: PublicKey,
  mint: PublicKey,
  amount: bigint,
  payer: PublicKey,
  tokenProgram: PublicKey,
) {
  if (
    amount === 0n
    || !mint.equals(NATIVE_MINT)
    || !tokenProgram.equals(TOKEN_PROGRAM_ID)
  ) {
    return [];
  }

  const sourceAta = getAssociatedTokenAddressSync(mint, sourceOwner, false, tokenProgram);
  let wrappedBalance = 0n;

  try {
    const accountInfo = await getBaseConnection(config).getAccountInfo(sourceAta, "confirmed");
    wrappedBalance = accountInfo ? (parseTokenAmount(accountInfo) ?? 0n) : 0n;
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch wrapped SOL balance", {
      ata: sourceAta.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }

  if (wrappedBalance >= amount) {
    return [];
  }

  const wrapLamports = amount - wrappedBalance;
  return [
    createAssociatedTokenAccountIdempotentInstruction(
      payer,
      sourceAta,
      sourceOwner,
      mint,
      tokenProgram,
    ),
    SystemProgram.transfer({
      fromPubkey: sourceOwner,
      toPubkey: sourceAta,
      lamports: wrapLamports,
    }),
    createSyncNativeInstruction(sourceAta, tokenProgram),
  ];
}

async function createNativeSolRentPdaTopUpInstructionsIfNeeded(
  config: RpcConfig,
  payer: PublicKey,
  mint: PublicKey,
  amount: bigint,
  tokenProgram: PublicKey,
) {
  if (
    amount === 0n
    || !mint.equals(NATIVE_MINT)
    || !tokenProgram.equals(TOKEN_PROGRAM_ID)
  ) {
    return [];
  }

  const [rentPda] = deriveRentPda();
  const targetLamports = BigInt(TRANSFER_QUEUE_RENT_LAMPORTS);
  let currentLamports: bigint;

  try {
    currentLamports = BigInt(await getBaseConnection(config).getBalance(rentPda, "confirmed"));
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch rent PDA balance", {
      rentPda: rentPda.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }

  if (currentLamports >= targetLamports) {
    return [];
  }

  return [
    SystemProgram.transfer({
      fromPubkey: payer,
      toPubkey: rentPda,
      lamports: targetLamports - currentLamports,
    }),
  ];
}

function requireAuthToken(authToken: string | undefined, message: string) {
  if (!authToken) {
    throw new ApiError(400, "MISSING_AUTH_TOKEN", message);
  }
}

function encodeStealthHandle(handle: string) {
  const handleBytes = new TextEncoder().encode(handle);
  if (handleBytes.length === 0 || handleBytes.length > MAX_STEALTH_HANDLE_BYTES) {
    throw new ApiError(400, "INVALID_STEALTH_HANDLE", `handle must be between 1 and ${MAX_STEALTH_HANDLE_BYTES} UTF-8 bytes`);
  }

  return handleBytes;
}

function encodeStealthHandleStorage(handleBytes: Uint8Array) {
  const storage = Buffer.alloc(STEALTH_HANDLE_STORAGE_BYTES);
  storage[0] = handleBytes.length;
  storage.set(handleBytes, 1);
  return storage;
}

function deriveStealthPoolFromHandleBytes(handleBytes: Uint8Array): [PublicKey, number] {
  const handleSeed = Buffer.from(handleBytes);
  const handleSeeds = handleSeed.length <= MAX_STEALTH_HANDLE_SEED_BYTES
    ? [handleSeed]
    : [
        handleSeed.subarray(0, MAX_STEALTH_HANDLE_SEED_BYTES),
        handleSeed.length <= MAX_INLINE_STEALTH_HANDLE_SEED_BYTES
          ? handleSeed.subarray(MAX_STEALTH_HANDLE_SEED_BYTES)
          : Buffer.from(sha256(Buffer.concat([STEALTH_POOL_LONG_HANDLE_HASH_DOMAIN, handleSeed]))),
      ];
  const seeds = [STEALTH_POOL_SEED, ...handleSeeds];
  return PublicKey.findProgramAddressSync(
    seeds,
    EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  );
}

export function deriveStealthPoolFromHandle(handle: string): [PublicKey, number] {
  return deriveStealthPoolFromHandleBytes(encodeStealthHandle(handle));
}

function resolveStealthPool(handle: string) {
  const handleBytes = encodeStealthHandle(handle);
  const [stealthPool] = deriveStealthPoolFromHandleBytes(handleBytes);

  return {
    handleStorage: encodeStealthHandleStorage(handleBytes),
    stealthPool,
  };
}

function isStealthPoolAccount(accountInfo: { owner: PublicKey } | null) {
  return accountInfo !== null
    && (
      accountInfo.owner.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)
      || accountInfo.owner.equals(DELEGATION_PROGRAM_ID)
    );
}

async function assertStealthPoolExists(
  config: RpcConfig,
  handle: string,
  stealthPool: PublicKey,
) {
  let accountInfo: { owner: PublicKey } | null;
  try {
    accountInfo = await getBaseConnection(config).getAccountInfo(stealthPool, "confirmed");
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch stealth pool account", {
      handle,
      stealthPool: stealthPool.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }

  if (!isStealthPoolAccount(accountInfo)) {
    throw new ApiError(400, "STEALTH_POOL_NOT_FOUND", "Stealth handle is not initialized", {
      handle,
      stealthPool: stealthPool.toBase58(),
      owner: accountInfo?.owner.toBase58(),
    });
  }
}

async function resolveTransferDestination(config: RpcConfig, input: TransferRequest) {
  const to = tryParsePublicKey(input.to);
  if (to) {
    return to;
  }

  if (
    input.visibility !== "private"
    || input.fromBalance !== "base"
    || input.toBalance !== "base"
  ) {
    throw new ApiError(
      400,
      "INVALID_STEALTH_TRANSFER",
      "Stealth handle transfers require visibility=private, fromBalance=base, and toBalance=base",
      { to: input.to },
    );
  }

  const { stealthPool } = resolveStealthPool(input.to);
  await assertStealthPoolExists(config, input.to, stealthPool);
  return stealthPool;
}

function updateStealthPoolInstruction(
  payer: PublicKey,
  stealthPool: PublicKey,
  authority: PublicKey,
  handleStorage: Uint8Array,
  destinations: PublicKey[],
  flags: number,
) {
  if (handleStorage.length !== STEALTH_HANDLE_STORAGE_BYTES) {
    throw new ApiError(400, "INVALID_STEALTH_HANDLE", "handle storage must be 256 bytes");
  }

  const data = Buffer.alloc(1 + STEALTH_HANDLE_STORAGE_BYTES + 1 + 1 + destinations.length * 32);
  let offset = 0;
  data[offset] = UPDATE_STEALTH_POOL_DISCRIMINATOR;
  offset += 1;
  data.set(handleStorage, offset);
  offset += STEALTH_HANDLE_STORAGE_BYTES;
  data[offset] = flags;
  offset += 1;
  data[offset] = destinations.length;
  offset += 1;

  for (const destination of destinations) {
    data.set(destination.toBuffer(), offset);
    offset += 32;
  }

  return new TransactionInstruction({
    programId: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: stealthPool, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ],
    data,
  });
}

function ensureStealthPoolDelegatedInstruction(
  payer: PublicKey,
  stealthPool: PublicKey,
  authority: PublicKey,
  handleStorage: Uint8Array,
  validator?: PublicKey,
) {
  if (handleStorage.length !== STEALTH_HANDLE_STORAGE_BYTES) {
    throw new ApiError(400, "INVALID_STEALTH_HANDLE", "handle storage must be 256 bytes");
  }

  const data = Buffer.alloc(1 + STEALTH_HANDLE_STORAGE_BYTES + (validator ? 32 : 0));
  let offset = 0;
  data[offset] = ENSURE_STEALTH_POOL_DELEGATED_DISCRIMINATOR;
  offset += 1;
  data.set(handleStorage, offset);
  offset += STEALTH_HANDLE_STORAGE_BYTES;
  if (validator) {
    data.set(validator.toBuffer(), offset);
  }

  return new TransactionInstruction({
    programId: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    keys: [
      { pubkey: payer, isSigner: true, isWritable: true },
      { pubkey: stealthPool, isSigner: false, isWritable: true },
      { pubkey: permissionPdaFromAccount(stealthPool), isSigner: false, isWritable: true },
      { pubkey: EPHEMERAL_SPL_TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      {
        pubkey: delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
          stealthPool,
          EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
        ),
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: delegationRecordPdaFromDelegatedAccount(stealthPool),
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: delegationMetadataPdaFromDelegatedAccount(stealthPool),
        isSigner: false,
        isWritable: true,
      },
      { pubkey: DELEGATION_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
      { pubkey: PERMISSION_PROGRAM_ID, isSigner: false, isWritable: false },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    data,
  });
}

function createTokenTransferInstruction(
  source: PublicKey,
  destination: PublicKey,
  authority: PublicKey,
  amount: bigint,
  programId: PublicKey = TOKEN_PROGRAM_ID,
) {
  const data = Buffer.alloc(9);
  data[0] = 3; // TokenInstruction::Transfer
  data.writeBigUInt64LE(amount, 1);

  return new TransactionInstruction({
    programId,
    keys: [
      { pubkey: source, isSigner: false, isWritable: true },
      { pubkey: destination, isSigner: false, isWritable: true },
      { pubkey: authority, isSigner: true, isWritable: false },
    ],
    data,
  });
}

function isProcessPendingTransferQueueRefillInstruction(instruction: TransactionInstruction) {
  return instruction.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)
    && instruction.data.length === 1
    && instruction.data.readInt8(0) === 28;
}

function readDelegatedValidator(accountInfo: { owner: PublicKey; lamports: number; data: Buffer | Uint8Array } | null) {
  if (
    !accountInfo
    || accountInfo.lamports === 0
    || !accountInfo.owner.equals(DELEGATION_PROGRAM_ID)
    || accountInfo.data.length < 40
  ) {
    return undefined;
  }

  return new PublicKey(Buffer.from(accountInfo.data).subarray(8, 40));
}

function normalizeRpcEndpoint(value: unknown) {
  if (typeof value !== "string" || value.trim().length === 0) {
    return undefined;
  }

  try {
    const url = new URL(value.trim());

    if (!["http:", "https:"].includes(url.protocol)) {
      return undefined;
    }

    if (url.pathname === "/" && !url.search && !url.hash) {
      return url.origin;
    }

    return url.toString();
  } catch {
    return undefined;
  }
}

function getDelegationRouterRpcUrl(cluster: RpcConfig["cluster"]) {
  return cluster === "mainnet" || cluster === "devnet"
    ? DELEGATION_ROUTER_RPC_URLS[cluster]
    : undefined;
}

async function tryResolveDelegationEndpointFromRouter(
  config: RpcConfig,
  delegatedAccount: PublicKey,
): Promise<DelegationEndpointResolution> {
  const routerRpcUrl = getDelegationRouterRpcUrl(config.cluster);

  if (!routerRpcUrl) {
    return {
      error: `No delegation router configured for cluster=${config.cluster}`,
    };
  }

  try {
    const response = await fetch(routerRpcUrl, {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "getDelegationStatus",
        params: [delegatedAccount.toBase58()],
      }),
    });

    if (!response.ok) {
      return {
        error: `Delegation router returned HTTP ${response.status}`,
      };
    }

    const payload = await response.json() as DelegationStatusRpcResponse;

    if (payload.error) {
      return {
        error: payload.error.message ?? "Delegation router returned an error",
      };
    }

    const endpoint = payload.result?.isDelegated
      ? normalizeRpcEndpoint(payload.result.fqdn)
      : undefined;

    return {
      endpoint,
      isDelegated: payload.result?.isDelegated,
      error: payload.result?.isDelegated && !endpoint
        ? "Delegation router did not return a usable fqdn"
        : undefined,
    };
  } catch (error) {
    return {
      error: getSanitizedErrorMessage(error),
    };
  }
}

function getHardcodedTeeRpcEndpoint(config: RpcConfig, validator: PublicKey | undefined) {
  if (!validator?.equals(TEE_VALIDATOR)) {
    return undefined;
  }

  if (config.teeRpcUrl) {
    return config.teeRpcUrl;
  }

  if (config.cluster === "devnet") {
    return DEVNET_TEE_RPC_URL;
  }

  if (config.cluster === "mainnet") {
    return MAINNET_TEE_RPC_URL;
  }

  return undefined;
}

async function resolveUndelegateEphemeralRpcEndpoint(
  config: RpcConfig,
  delegatedAccount: PublicKey,
) {
  const routerResolution = await tryResolveDelegationEndpointFromRouter(config, delegatedAccount);

  if (routerResolution.endpoint) {
    return routerResolution.endpoint;
  }

  const delegationRecord = delegationRecordPdaFromDelegatedAccount(delegatedAccount);
  let delegationAccount: Awaited<ReturnType<Connection["getAccountInfo"]>>;
  try {
    delegationAccount = await getBaseConnection(config).getAccountInfo(delegationRecord, "confirmed");
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch delegation record", {
      delegatedAccount: delegatedAccount.toBase58(),
      delegationRecord: delegationRecord.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }

  const delegatedValidator = readDelegatedValidator(delegationAccount);
  const hardcodedEndpoint = getHardcodedTeeRpcEndpoint(config, delegatedValidator);

  if (hardcodedEndpoint) {
    return hardcodedEndpoint;
  }

  throw new ApiError(
    400,
    "EPHEMERAL_ENDPOINT_UNRESOLVED",
    "Ephemeral RPC endpoint cannot be retrieved",
    {
      cluster: config.cluster,
      delegatedAccount: delegatedAccount.toBase58(),
      delegationRecord: delegationRecord.toBase58(),
      delegatedValidator: delegatedValidator?.toBase58(),
      routerIsDelegated: routerResolution.isDelegated,
      routerError: routerResolution.error,
    },
  );
}

async function assertProjectedWritableAtaValidators(
  config: RpcConfig,
  accounts: ProjectedWritableAta[],
  validator: PublicKey,
) {
  if (accounts.length === 0) {
    return;
  }

  let delegationAccounts: Awaited<ReturnType<Connection["getMultipleAccountsInfo"]>>;
  try {
    delegationAccounts = await getBaseConnection(config).getMultipleAccountsInfo(
      accounts.map(account => account.delegationRecord),
      "confirmed",
    );
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch delegation records", {
      message: getSanitizedErrorMessage(error),
    });
  }

  const mismatchedAccounts = accounts.flatMap((account, index) => {
    const delegatedValidator = readDelegatedValidator(delegationAccounts[index]);

    if (!delegatedValidator || delegatedValidator.equals(validator)) {
      return [];
    }

    return [{
      role: account.role,
      owner: account.owner.toBase58(),
      mint: account.mint.toBase58(),
      ata: account.ata.toBase58(),
      eata: account.eata.toBase58(),
      delegationRecord: account.delegationRecord.toBase58(),
      currentValidator: delegatedValidator.toBase58(),
      selectedValidator: validator.toBase58(),
    }];
  });

  if (mismatchedAccounts.length > 0) {
    throw new ApiError(
      400,
      "EATA_VALIDATOR_MISMATCH",
      "Projected token account is delegated to another validator",
      { accounts: mismatchedAccounts },
    );
  }
}

function withPrivateTransferExactOut(
  instruction: TransactionInstruction,
  exactOut: boolean,
) {
  if (
    !instruction.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)
    || instruction.data[0] !== 25
    || instruction.data[93] !== 1
    || instruction.data[126] !== instruction.data.length - 127
  ) {
    return instruction;
  }

  return new TransactionInstruction({
    programId: instruction.programId,
    keys: instruction.keys,
    data: Buffer.concat([
      instruction.data.subarray(0, 13),
      Buffer.from([exactOut ? 1 : 0]),
      instruction.data.subarray(13),
    ]),
  });
}

function withGroupReceiptPermissionAccounts(instruction: TransactionInstruction) {
  if (
    !instruction.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)
    || instruction.data[0] !== DEPOSIT_AND_QUEUE_TRANSFER_DISCRIMINATOR
    || instruction.keys.length !== DEPOSIT_AND_QUEUE_TRANSFER_LEGACY_ACCOUNT_COUNT
  ) {
    return instruction;
  }

  const groupReceipt = instruction.keys[DEPOSIT_AND_QUEUE_TRANSFER_GROUP_RECEIPT_INDEX]?.pubkey;
  if (!groupReceipt) {
    return instruction;
  }

  return new TransactionInstruction({
    programId: instruction.programId,
    keys: [
      ...instruction.keys,
      {
        pubkey: permissionPdaFromAccount(groupReceipt),
        isSigner: false,
        isWritable: true,
      },
      {
        pubkey: PERMISSION_PROGRAM_ID,
        isSigner: false,
        isWritable: false,
      },
    ],
    data: Buffer.from(instruction.data),
  });
}

function createRandomShuttleId() {
  return crypto.getRandomValues(new Uint32Array(1))[0] & 0x7fffffff;
}

async function getValidatorFromRpc(endpoint: string) {
  let request = validatorCache.get(endpoint);

  if (!request) {
    request = (async () => {
      const response = await fetch(endpoint, {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          method: "getIdentity",
          params: [],
        }),
      });

      if (!response.ok) {
        throw new ApiError(502, "RPC_ERROR", "Failed to resolve validator identity", {
          endpoint: redactUrls(endpoint),
          status: response.status,
        });
      }

      const payload = await response.json() as RpcIdentityResponse;

      if (payload.error) {
        throw new ApiError(502, "RPC_ERROR", redactUrls(payload.error.message || "Failed to resolve validator identity"), {
          endpoint: redactUrls(endpoint),
        });
      }

      return payload.result?.identity
        ? new PublicKey(payload.result.identity)
        : undefined;
    })().catch((error) => {
      if (validatorCache.get(endpoint) === request) {
        validatorCache.delete(endpoint);
      }

      throw error;
    });

    validatorCache.set(endpoint, request);
  }

  return request;
}

async function resolveValidator(config: RpcConfig, explicitValidator?: string) {
  if (explicitValidator) {
    return parsePublicKey(explicitValidator, "validator");
  }

  try {
    return await getValidatorFromRpc(config.ephemeralRpcUrl) ?? DEFAULT_FALLBACK_VALIDATOR;
  } catch {
    return DEFAULT_FALLBACK_VALIDATOR;
  }
}

async function resolveDepositValidator(config: RpcConfig, explicitValidator?: string) {
  return resolveValidator(config, explicitValidator);
}

async function resolveRequiredValidator(config: RpcConfig, explicitValidator?: string) {
  const validator = await resolveValidator(config, explicitValidator);

  if (!validator) {
    throw new ApiError(502, "RPC_ERROR", "Failed to resolve validator identity", {
      endpoint: redactUrls(config.ephemeralRpcUrl),
    });
  }

  return validator;
}

async function getBlockhashFromConnection(
  connection: Connection,
  source: SendTarget,
): Promise<BlockhashResult> {
  try {
    const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
    return { blockhash, lastValidBlockHeight };
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch recent blockhash", {
      source,
      message: getSanitizedErrorMessage(error),
    });
  }
}

async function getBlockhash(config: RpcConfig, source: SendTarget, authToken?: string): Promise<BlockhashResult> {
  const connection = source === "base"
    ? getBaseConnection(config)
    : getEphemeralConnection(config, authToken);

  return getBlockhashFromConnection(connection, source);
}

async function getBlockhashFromRpcEndpoint(
  rpcEndpoint: string,
  source: SendTarget,
  authToken?: string,
): Promise<BlockhashResult> {
  return getBlockhashFromConnection(
    getConnectionWithOptionalAuthToken(rpcEndpoint, authToken),
    source,
  );
}

function getTransactionSendRpcEndpoint(config: RpcConfig, input: SendTransactionRequest) {
  if (input.sendRpcEndpoint !== undefined) {
    if (input.sendTo !== "ephemeral") {
      throw new ApiError(
        400,
        "INVALID_SEND_RPC_ENDPOINT",
        "sendRpcEndpoint can only be used when sendTo is ephemeral",
      );
    }

    const endpoint = normalizeRpcEndpoint(input.sendRpcEndpoint);
    if (!endpoint) {
      throw new ApiError(
        400,
        "INVALID_SEND_RPC_ENDPOINT",
        "sendRpcEndpoint must be a valid http(s) URL",
      );
    }

    return endpoint;
  }

  return input.sendTo === "base"
    ? config.baseRpcUrl
    : config.ephemeralRpcUrl;
}

function decodeTransactionBase64(value: string) {
  const transactionBase64 = value.trim();

  if (
    transactionBase64.length === 0
    || transactionBase64.length % 4 !== 0
    || !/^[A-Za-z0-9+/]+={0,2}$/.test(transactionBase64)
  ) {
    throw new ApiError(400, "INVALID_TRANSACTION", "transactionBase64 must be valid base64");
  }

  const transaction = Buffer.from(transactionBase64, "base64");
  if (transaction.length === 0) {
    throw new ApiError(400, "INVALID_TRANSACTION", "transactionBase64 must decode to transaction bytes");
  }

  if (transaction.length > SOLANA_WIRE_TRANSACTION_SIZE_LIMIT) {
    throw new ApiError(
      400,
      "TRANSACTION_TOO_LARGE",
      `Serialized transaction exceeds ${SOLANA_WIRE_TRANSACTION_SIZE_LIMIT} bytes`,
    );
  }

  return transaction;
}

export async function sendSignedTransaction(
  env: AppEnv,
  input: SendTransactionRequest,
  authToken?: string,
): Promise<SendTransactionResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const rpcEndpoint = getTransactionSendRpcEndpoint(config, input);
  const connection = input.sendTo === "base"
    ? getBaseConnection(config)
    : getConnectionWithOptionalAuthToken(rpcEndpoint, authToken);
  const confirmationRpcEndpoint = rpcEndpoint;
  const confirmationRequiresAuthToken = input.sendTo === "ephemeral";
  const transaction = decodeTransactionBase64(input.transactionBase64);
  const options: SendOptions = {
    preflightCommitment: "confirmed",
  };

  if (input.skipPreflight !== undefined) {
    options.skipPreflight = input.skipPreflight;
  }

  if (input.maxRetries !== undefined) {
    options.maxRetries = input.maxRetries;
  }

  if (input.confirm && (input.recentBlockhash === undefined || input.lastValidBlockHeight === undefined)) {
    throw new ApiError(
      400,
      "MISSING_CONFIRMATION_FIELDS",
      "recentBlockhash and lastValidBlockHeight are required when confirm is true",
    );
  }

  try {
    const signature = await connection.sendRawTransaction(transaction, options);

    if (!input.confirm) {
      return {
        signature,
        sendTo: input.sendTo,
        confirmed: false,
        confirmationRpcEndpoint,
        confirmationRequiresAuthToken,
      };
    }

    const blockhash = input.recentBlockhash!;
    const lastValidBlockHeight = input.lastValidBlockHeight!;
    const confirmation = await connection.confirmTransaction({
      signature,
      blockhash,
      lastValidBlockHeight,
    }, "confirmed");

    if (confirmation.value.err !== null) {
      throw new ApiError(400, "TRANSACTION_FAILED", "Transaction failed", {
        err: confirmation.value.err,
      });
    }

    return {
      signature,
      sendTo: input.sendTo,
      confirmed: true,
      confirmationRpcEndpoint,
      confirmationRequiresAuthToken,
    };
  } catch (error) {
    if (error instanceof ApiError) {
      throw error;
    }

    throw new ApiError(502, "RPC_ERROR", "Failed to send transaction", {
      sendTo: input.sendTo,
      message: getSanitizedErrorMessage(error),
    });
  }
}

function getRequiredSigners(feePayer: PublicKey, instructions: TransactionInstruction[]) {
  const signers = new Set<string>([feePayer.toBase58()]);

  for (const instruction of instructions) {
    for (const key of instruction.keys) {
      if (key.isSigner) {
        signers.add(key.pubkey.toBase58());
      }
    }
  }

  return [...signers];
}

function isSupportedGaslessMint(cluster: RpcConfig["cluster"], mint: PublicKey) {
  if (cluster === "custom") {
    return true;
  }

  const mintAddress = mint.toBase58();
  if (cluster === "devnet") {
    return mintAddress === DEFAULT_DEPOSIT_DEVNET_MINT;
  }

  return mintAddress === DEFAULT_DEPOSIT_MINT || mintAddress === MAINNET_USDT_MINT;
}

function privateTransferFeeAmount(amount: bigint) {
  return amount * PRIVATE_TRANSFER_FEE_BASIS_POINTS / BASIS_POINTS_FACTOR;
}

function platformTransferFeeAmount(amount: bigint, platformFeeBps: number) {
  return amount * BigInt(platformFeeBps) / BASIS_POINTS_FACTOR;
}

function isPrivateBaseToBaseTransfer(input: TransferRequest) {
  return input.visibility === "private"
    && input.fromBalance === "base"
    && input.toBalance === "base";
}

function privateTransferSetupLamports(input: TransferRequest) {
  return isPrivateBaseToBaseTransfer(input) ? PRIVATE_TRANSFER_SETUP_LAMPORTS : 0n;
}

function createTransferFees(lamports: bigint, tokens: bigint): TransferFees {
  return {
    lamports: lamports.toString(),
    tokens: tokens.toString(),
  };
}

function isTransactionTooLargeMessage(message: string) {
  const normalized = message.toLowerCase();

  return normalized.includes("transaction too large")
    || normalized.includes("too many signatures")
    || normalized.includes("too many signers")
    || normalized.includes("encoding overruns uint8array");
}

function throwTransactionBuildError(error: unknown): never {
  if (error instanceof ApiError) {
    throw error;
  }

  if (!(error instanceof Error)) {
    throw new ApiError(400, "TRANSACTION_BUILD_ERROR", "Failed to build transaction");
  }

  const message = redactUrls(error.message.trim() || "Failed to build transaction");

  if (message.includes("transferSpl route not implemented")) {
    throw new ApiError(400, "UNSUPPORTED_TRANSFER_ROUTE", message);
  }

  if (isTransactionTooLargeMessage(message)) {
    throw new ApiError(400, "TRANSACTION_TOO_LARGE", message);
  }

  throw new ApiError(400, "TRANSACTION_BUILD_ERROR", message);
}

function createUnsignedTransaction(
  instructions: TransactionInstruction[],
  feePayer: PublicKey,
  blockhash: BlockhashResult,
) {
  const transaction = new Transaction();
  transaction.feePayer = feePayer;
  transaction.recentBlockhash = blockhash.blockhash;
  transaction.add(...instructions);
  return transaction;
}

function collectLookupTableCandidateAddresses(instructions: TransactionInstruction[]) {
  const addresses = new Set<string>();

  for (const instruction of instructions) {
    addresses.add(instruction.programId.toBase58());

    for (const key of instruction.keys) {
      addresses.add(key.pubkey.toBase58());
    }
  }

  return addresses;
}

function serializeTransaction(
  kind: TransactionResponse["kind"],
  sendTo: SendTarget,
  instructions: TransactionInstruction[],
  feePayer: PublicKey,
  blockhash: BlockhashResult,
  validator?: PublicKey,
  partialSigners: Keypair[] = [],
  from?: SendTarget,
  fees?: TransferFees,
  sendRpcEndpoint?: string,
): TransactionResponse {
  const transaction = createUnsignedTransaction(instructions, feePayer, blockhash);
  if (partialSigners.length > 0) {
    transaction.partialSign(...partialSigners);
  }

  return {
    kind,
    version: "legacy",
    transactionBase64: Buffer.from(
      transaction.serialize({
        requireAllSignatures: false,
        verifySignatures: false,
      }),
    ).toString("base64"),
    sendTo,
    sendRpcEndpoint,
    from,
    recentBlockhash: blockhash.blockhash,
    lastValidBlockHeight: blockhash.lastValidBlockHeight,
    instructionCount: instructions.length,
    requiredSigners: getRequiredSigners(feePayer, instructions),
    validator: validator?.toBase58(),
    fees,
  };
}

async function trySerializePrivateBaseToBaseTransferTransactionWithLookupTable(
  env: AppEnv,
  config: RpcConfig,
  instructions: TransactionInstruction[],
  feePayer: PublicKey,
  blockhash: BlockhashResult,
  validator?: PublicKey,
  partialSigners: Keypair[] = [],
  fees?: TransferFees,
): Promise<TransactionResponse | undefined> {
  if (config.cluster === "custom") {
    return undefined;
  }

  const lookupTableAddress = resolvePrivateBaseToBaseTransferLookupTableAddress(env, config.cluster);

  try {
    const lookupTable = await getCachedAddressLookupTable(config.baseRpcUrl, lookupTableAddress, {
      validateOwner: true,
    });

    const lookupTableAddresses = new Set(
      lookupTable.state.addresses.map(address => address.toBase58()),
    );
    const candidateAddresses = collectLookupTableCandidateAddresses(instructions);
    let hasExpectedAddress = false;

    for (const address of candidateAddresses) {
      if (lookupTableAddresses.has(address)) {
        hasExpectedAddress = true;
        break;
      }
    }

    if (!hasExpectedAddress) {
      throw new Error("lookup table does not contain any transfer instruction keys");
    }

    const transaction = createUnsignedTransaction(instructions, feePayer, blockhash);
    const compiled = compileLegacyTransactionToV0({
      transaction,
      lookupTables: [lookupTable],
    });

    if (compiled.usedLookupTables.length === 0 || compiled.bytesSaved <= 0) {
      return undefined;
    }

    if (partialSigners.length > 0) {
      compiled.transaction.sign(partialSigners);
    }

    return {
      kind: "transfer",
      version: "v0",
      transactionBase64: Buffer.from(compiled.transaction.serialize()).toString("base64"),
      sendTo: "base",
      from: "base",
      recentBlockhash: blockhash.blockhash,
      lastValidBlockHeight: blockhash.lastValidBlockHeight,
      instructionCount: instructions.length,
      requiredSigners: getRequiredSigners(feePayer, instructions),
      validator: validator?.toBase58(),
      fees,
    };
  } catch (error) {
    console.warn("LUT v0 compilation failed, falling back to legacy", {
      cluster: config.cluster,
      lookupTable: lookupTableAddress.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
    return undefined;
  }
}

export async function buildDepositTransaction(env: AppEnv, input: DepositRequest) {
  try {
    const config = resolveRpcConfig(env, input.cluster);
    const owner = parsePublicKey(input.owner, "owner");
    const mint = parsePublicKey(
      input.mint ?? (config.cluster === "devnet" ? DEFAULT_DEPOSIT_DEVNET_MINT : DEFAULT_DEPOSIT_MINT),
      "mint",
    );
    const amount = parseAmount(input.amount, "amount");
    const payer = owner;
    const feePayer = owner;
    const validator = await resolveDepositValidator(config, input.validator);
    const tokenProgram = input.mint !== undefined
      ? await resolveMintTokenProgram(config, mint)
      : TOKEN_PROGRAM_ID;
    const blockhash = await getBlockhash(config, "base");
    const nativeSolWrapInstructions = await createNativeSolWrapInstructionsIfNeeded(
      config,
      owner,
      mint,
      amount,
      payer,
      tokenProgram,
    );
    const nativeSolRentPdaTopUpInstructions = await createNativeSolRentPdaTopUpInstructionsIfNeeded(
      config,
      payer,
      mint,
      amount,
      tokenProgram,
    );

    const delegateInstructions = await delegateSpl(owner, mint, amount, {
      payer,
      validator,
      tokenProgram,
      initIfMissing: input.initIfMissing,
      initVaultIfMissing: input.initVaultIfMissing,
      initAtasIfMissing: input.initAtasIfMissing,
      shuttleId: createRandomShuttleId(),
      escrowIndex: 0,
      idempotent: input.idempotent,
      private: input.private ?? true,
    });
    const instructions = [
      ...nativeSolWrapInstructions,
      ...nativeSolRentPdaTopUpInstructions,
      ...delegateInstructions,
    ];

    return serializeTransaction(
      "deposit",
      "base",
      instructions,
      feePayer,
      blockhash,
      validator,
    );
  } catch (error) {
    throwTransactionBuildError(error);
  }
}

export async function buildWithdrawTransaction(env: AppEnv, input: WithdrawRequest) {
  try {
    const config = resolveRpcConfig(env, input.cluster);
    const owner = parsePublicKey(input.owner, "owner");
    const mint = parsePublicKey(input.mint, "mint");
    const amount = parseAmount(input.amount, "amount");
    const payer = owner;
    const feePayer = owner;
    const validator = await resolveValidator(config, input.validator);
    const tokenProgram = await resolveMintTokenProgram(config, mint);
    const blockhash = await getBlockhash(config, "base");

    const instructions = await withdrawSpl(owner, mint, amount, {
      payer,
      validator,
      tokenProgram,
      initIfMissing: input.initIfMissing,
      initAtasIfMissing: input.initAtasIfMissing,
      shuttleId: createRandomShuttleId(),
      escrowIndex: input.escrowIndex,
      idempotent: input.idempotent,
    });

    if (
      input.idempotent === false
      && mint.equals(NATIVE_MINT)
      && tokenProgram.equals(TOKEN_PROGRAM_ID)
    ) {
      instructions.push(
        createCloseTokenAccountInstruction(
          getAssociatedTokenAddressSync(mint, owner, false, tokenProgram),
          owner,
          owner,
          tokenProgram,
        ),
      );
    }

    return serializeTransaction(
      "withdraw",
      "base",
      instructions,
      feePayer,
      blockhash,
      validator,
    );
  } catch (error) {
    throwTransactionBuildError(error);
  }
}

export async function buildInitializeMintTransaction(
  env: AppEnv,
  input: InitializeMintRequest,
): Promise<InitializeMintResponse> {
  try {
    const config = resolveRpcConfig(env, input.cluster);
    const payer = parsePublicKey(input.payer, "payer");
    const mint = parsePublicKey(input.mint, "mint");
    const validator = await resolveRequiredValidator(config, input.validator);
    const [transferQueue] = deriveTransferQueue(mint, validator);
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(mint);
    const [vaultEphemeralAta] = deriveEphemeralAta(vault, mint);
    const tokenProgram = await resolveMintTokenProgram(config, mint);
    const vaultAta = deriveVaultAta(mint, vault, tokenProgram);
    const blockhash = await getBlockhash(config, "base");

    const instructions = [
      initTransferQueueIx(
        payer,
        transferQueue,
        mint,
        validator,
        undefined,
        tokenProgram,
      ),
      initRentPdaIx(
        payer,
        rentPda,
      ),
      SystemProgram.transfer({
        fromPubkey: payer,
        toPubkey: rentPda,
        lamports: TRANSFER_QUEUE_RENT_LAMPORTS,
      }),
      delegateTransferQueueIx(
        transferQueue,
        payer,
        mint,
      ),
      initVaultIx(vault, mint, payer, tokenProgram),
      initVaultAtaIx(payer, vaultAta, vault, mint, tokenProgram),
      delegateEphemeralAtaIx(payer, vaultEphemeralAta, validator),
    ];

    const response = serializeTransaction(
      "initializeMint",
      "base",
      instructions,
      payer,
      blockhash,
      validator,
    );

    return {
      ...response,
      kind: "initializeMint",
      version: "legacy",
      sendTo: "base",
      recentBlockhash: blockhash.blockhash,
      lastValidBlockHeight: blockhash.lastValidBlockHeight,
      instructionCount: instructions.length,
      requiredSigners: response.requiredSigners,
      transactionBase64: response.transactionBase64,
      validator: validator.toBase58(),
      transferQueue: transferQueue.toBase58(),
      rentPda: rentPda.toBase58(),
    };
  } catch (error) {
    throwTransactionBuildError(error);
  }
}

export async function buildUndelegateEphemeralAtaTransaction(
  env: AppEnv,
  input: UndelegateEphemeralAtaRequest,
  authToken?: string,
): Promise<UndelegateEphemeralAtaResponse> {
  try {
    const config = resolveRpcConfig(env, input.cluster);
    const payer = parsePublicKey(input.payer, "payer");
    const mint = parsePublicKey(input.mint, "mint");
    await resolveMintTokenProgram(config, mint);
    const [ephemeralAta] = deriveEphemeralAta(payer, mint);
    const sendRpcEndpoint = await resolveUndelegateEphemeralRpcEndpoint(config, ephemeralAta);
    const blockhash = await getBlockhashFromRpcEndpoint(sendRpcEndpoint, "ephemeral", authToken);

    // A delegated payer cannot cover commit fees itself: pass the delegating
    // validator's Magic fee vault, with the validator identity appended as raw
    // data so the program can verify the vault PDA.
    let delegationAccounts: Awaited<ReturnType<Connection["getMultipleAccountsInfo"]>>;
    try {
      delegationAccounts = await getBaseConnection(config).getMultipleAccountsInfo(
        [
          delegationRecordPdaFromDelegatedAccount(payer),
          delegationRecordPdaFromDelegatedAccount(ephemeralAta),
        ],
        "confirmed",
      );
    } catch (error) {
      throw new ApiError(502, "RPC_ERROR", "Failed to fetch delegation records", {
        payer: payer.toBase58(),
        ephemeralAta: ephemeralAta.toBase58(),
        message: getSanitizedErrorMessage(error),
      });
    }
    const payerValidator = readDelegatedValidator(delegationAccounts[0]);

    const undelegate = undelegateIx(payer, mint);
    if (payerValidator) {
      // The commit executes on the eATA's validator, so its fee vault is the
      // one Magic charges; a payer delegated elsewhere cannot be sponsored.
      const eataValidator = readDelegatedValidator(delegationAccounts[1]);
      if (!eataValidator?.equals(payerValidator)) {
        throw new ApiError(
          400,
          "VALIDATOR_MISMATCH",
          "Payer and ephemeral ATA are delegated to different validators",
          {
            payer: payer.toBase58(),
            payerValidator: payerValidator.toBase58(),
            ephemeralAta: ephemeralAta.toBase58(),
            ephemeralAtaValidator: eataValidator?.toBase58(),
          },
        );
      }
      undelegate.keys.push({
        pubkey: magicFeeVaultPdaFromValidator(payerValidator),
        isSigner: false,
        isWritable: true,
      });
      undelegate.data = Buffer.concat([undelegate.data, payerValidator.toBuffer()]);
    }
    const instructions = [undelegate];

    const response = serializeTransaction(
      "undelegateEphemeralAta",
      "ephemeral",
      instructions,
      payer,
      blockhash,
    );

    return {
      ...response,
      kind: "undelegateEphemeralAta",
      version: "legacy",
      sendTo: "ephemeral",
      sendRpcEndpoint,
      recentBlockhash: blockhash.blockhash,
      lastValidBlockHeight: blockhash.lastValidBlockHeight,
      instructionCount: instructions.length,
      requiredSigners: response.requiredSigners,
      transactionBase64: response.transactionBase64,
    };
  } catch (error) {
    throwTransactionBuildError(error);
  }
}

export async function buildUpdateStealthPoolTransaction(
  env: AppEnv,
  input: StealthPoolRequest,
  authToken?: string,
): Promise<StealthPoolResponse> {
  try {
    requireAuthToken(authToken, "authToken is required to initialize stealth pool destinations inside the ER");

    const config = resolveRpcConfig(env, input.cluster);
    const payer = parsePublicKey(input.payer, "payer");
    const authority = parsePublicKey(input.authority, "authority");
    const destinations = input.destinations.map((destination, index) =>
      parsePublicKey(destination, `destinations[${index}]`));
    const { handleStorage, stealthPool } = resolveStealthPool(input.handle);
    const flags = input.splitAcrossKeys ? STEALTH_POOL_SPLIT_ACROSS_KEYS_FLAG : 0;
    const validator = await resolveValidator(config, input.validator);
    const setupBlockhash = await getBlockhash(config, "base");
    const initializeBlockhash = await getBlockhash(config, "ephemeral", authToken);
    const setupInstructions = [
      ensureStealthPoolDelegatedInstruction(
        payer,
        stealthPool,
        authority,
        handleStorage,
        validator,
      ),
    ];
    const initializeInstructions = [
      updateStealthPoolInstruction(
        payer,
        stealthPool,
        authority,
        handleStorage,
        destinations,
        flags,
      ),
    ];

    const setupTransaction = serializeTransaction(
      "stealthPool",
      "base",
      setupInstructions,
      payer,
      setupBlockhash,
      validator,
    );
    const response = serializeTransaction(
      "stealthPool",
      "ephemeral",
      initializeInstructions,
      payer,
      initializeBlockhash,
      validator,
    );

    return {
      ...response,
      kind: "stealthPool",
      setupTransaction,
      stealthPool: stealthPool.toBase58(),
    };
  } catch (error) {
    throwTransactionBuildError(error);
  }
}

export async function getStealthPoolStatus(
  env: AppEnv,
  input: StealthPoolStatusRequest,
): Promise<StealthPoolStatusResponse> {
  const config = resolveRpcConfig(env, input.cluster);

  const { stealthPool } = resolveStealthPool(input.handle);
  const connection = getBaseConnection(config);

  try {
    const accountInfo = await connection.getAccountInfo(stealthPool, "confirmed");
    const exists = isStealthPoolAccount(accountInfo);

    return {
      stealthPool: stealthPool.toBase58(),
      exists,
    };
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch stealth pool account", {
      message: getSanitizedErrorMessage(error),
    });
  }
}

export async function buildTransferTransaction(env: AppEnv, input: TransferRequest, authToken?: string) {
  try {
    const config = resolveRpcConfig(env, input.cluster);
    const from = parsePublicKey(input.from, "from");
    const to = await resolveTransferDestination(config, input);
    const mint = parsePublicKey(input.mint, "mint");
    const allowZeroAmount = input.visibility === "private"
      && input.fromBalance === "base"
      && input.toBalance === "ephemeral";
    const amount = parseAmount(input.amount, "amount", {
      allowZero: allowZeroAmount,
    });
    const shuttleId = createRandomShuttleId();

    const minDelayMs = parseOptionalAmount(input.minDelayMs, "minDelayMs");
    const maxDelayMs = parseOptionalAmount(input.maxDelayMs, "maxDelayMs");
    const clientRefId = parseOptionalAmount(input.clientRefId, "clientRefId");
    const split = input.split;
    const exactOut = input.exactOut ?? true;
    const platformFeeBps = input.platformFeeBps ?? 0;
    let platformFeeAccount: PublicKey | undefined;

    if (!Number.isSafeInteger(platformFeeBps) || platformFeeBps < 0 || platformFeeBps > 10_000) {
      throw new ApiError(400, "INVALID_PLATFORM_FEE", "platformFeeBps must be an integer between 0 and 10000");
    }

    if (platformFeeBps > 0) {
      if (input.platformFeeAccount === undefined) {
        throw new ApiError(400, "INVALID_PLATFORM_FEE", "platformFeeAccount is required when platformFeeBps is greater than 0");
      }

      if (input.fromBalance !== "base") {
        throw new ApiError(400, "INVALID_PLATFORM_FEE", "platform fees are supported only when fromBalance is \"base\"");
      }

      platformFeeAccount = parsePublicKey(input.platformFeeAccount, "platformFeeAccount");
    }

    const platformFee = platformTransferFeeAmount(amount, platformFeeBps);
    if (!exactOut && platformFee >= amount) {
      throw new ApiError(400, "INVALID_PLATFORM_FEE", "platform fee must be less than amount when exactOut is false");
    }

    const transferAmount = exactOut ? amount : amount - platformFee;

    if (minDelayMs !== undefined && minDelayMs < 0n) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "minDelayMs must be non-negative");
    }

    if (maxDelayMs !== undefined && maxDelayMs < 0n) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "maxDelayMs must be non-negative");
    }

    if (clientRefId !== undefined && clientRefId < 0n) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "clientRefId must be non-negative");
    }

    if (
      minDelayMs !== undefined
      && maxDelayMs !== undefined
      && maxDelayMs < minDelayMs
    ) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "maxDelayMs must be greater than or equal to minDelayMs");
    }

    const maxDelayMsForValidation = maxDelayMs ?? minDelayMs;

    // Temporary cap while private transfer delay windows stay limited.
    if (
      input.visibility === "private"
      && maxDelayMsForValidation !== undefined
      && maxDelayMsForValidation > PRIVATE_TRANSFER_MAX_DELAY_MS_LIMIT
    ) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "maxDelayMs must be less than or equal to 600000");
    }

    if (
      split !== undefined
      && (!Number.isSafeInteger(split) || split <= 0 || split > 15)
    ) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "split must be an integer between 1 and 15");
    }

    if (split !== undefined && BigInt(split) > transferAmount) {
      throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "split cannot exceed transfer amount");
    }

    const useGasless = input.gasless === true && PublicKey.isOnCurve(from.toBuffer());

    if (useGasless && !isSupportedGaslessMint(config.cluster, mint)) {
      throw new ApiError(
        400,
        "INVALID_GASLESS_TRANSFER_MINT",
        "gasless is supported only for approved stablecoin mints",
      );
    }

    if (useGasless && amount < GASLESS_STABLECOIN_MIN_AMOUNT) {
      throw new ApiError(
        400,
        "INVALID_GASLESS_TRANSFER_AMOUNT",
        `gasless amount must be at least ${Number(GASLESS_STABLECOIN_MIN_AMOUNT) / 1_000_000} USDC/USDT`,
      );
    }

    const sponsor = useGasless ? getGaslessSponsorKeypair(env) : undefined;
    const payer = sponsor?.publicKey ?? from;
    const feePayer = sponsor?.publicKey ?? from;
    const privateTransferFee = isPrivateBaseToBaseTransfer(input) ? privateTransferFeeAmount(transferAmount) : 0n;
    const fees = createTransferFees(
      privateTransferSetupLamports(input),
      privateTransferFee + platformFee + (sponsor ? GASLESS_RELAY_FEE_MICRO_USDC : 0n),
    );

    const shouldResolveValidator = input.validator
      || input.visibility === "private"
      || input.fromBalance === "base"
      || input.initVaultIfMissing;

    const validator = shouldResolveValidator
      ? await resolveValidator(config, input.validator)
      : undefined;

    const tokenProgram = await resolveMintTokenProgram(config, mint);
    if (validator && isPrivateBaseToBaseTransfer(input)) {
      const sourceAta = getAssociatedTokenAddressSync(mint, from, true, tokenProgram);
      const [sourceEata] = deriveEphemeralAta(from, mint);
      await assertProjectedWritableAtaValidators(
        config,
        [{
          role: "source",
          owner: from,
          mint,
          ata: sourceAta,
          eata: sourceEata,
          delegationRecord: delegationRecordPdaFromDelegatedAccount(sourceEata),
        }],
        validator,
      );
    }

    const gaslessFeeInstructions = sponsor
      ? [
          createTokenTransferInstruction(
            getAssociatedTokenAddressSync(mint, from, true, tokenProgram),
            getAssociatedTokenAddressSync(mint, sponsor.publicKey, true, tokenProgram),
            from,
            GASLESS_RELAY_FEE_MICRO_USDC,
            tokenProgram,
          ),
        ]
      : [];

    const sendTo: SendTarget = input.fromBalance === "ephemeral" ? "ephemeral" : "base";
    const blockhash = await getBlockhash(config, sendTo, authToken);
    const nativeSolWrapAmount = transferAmount + platformFee + (
      isPrivateBaseToBaseTransfer(input) && exactOut ? privateTransferFee : 0n
    );
    const nativeSolWrapInstructions = input.visibility === "private" && input.fromBalance === "base"
      ? await createNativeSolWrapInstructionsIfNeeded(
          config,
          from,
          mint,
          nativeSolWrapAmount,
          payer,
          tokenProgram,
        )
      : [];
    const nativeSolRentPdaTopUpInstructions = input.visibility === "private" && input.fromBalance === "base"
      ? await createNativeSolRentPdaTopUpInstructionsIfNeeded(
          config,
          payer,
          mint,
          transferAmount,
          tokenProgram,
        )
      : [];
    const platformFeeInstructions = platformFee > 0n && platformFeeAccount
      ? [
          createTokenTransferInstruction(
            getAssociatedTokenAddressSync(mint, from, false, tokenProgram),
            platformFeeAccount,
            from,
            platformFee,
            tokenProgram,
          ),
        ]
      : [];

    const transferInstructions = await transferSpl(from, to, mint, transferAmount, {
      visibility: input.visibility,
      fromBalance: input.fromBalance,
      toBalance: input.toBalance,
      payer,
      validator,
      tokenProgram,
      initIfMissing: input.initIfMissing,
      initAtasIfMissing: input.initAtasIfMissing,
      initVaultIfMissing: input.initVaultIfMissing,
      shuttleId,
      privateTransfer: input.visibility === "private"
        ? {
            minDelayMs,
            maxDelayMs,
            clientRefId,
            split,
            exactOut,
          }
        : undefined,
    });
    const normalizedTransferInstructions = transferInstructions.map(instruction =>
      withGroupReceiptPermissionAccounts(withPrivateTransferExactOut(instruction, exactOut)));
    // Gasless private base->base already adds a relay-fee token transfer. Dropping
    // the opportunistic queue-refill ix keeps the full transaction under Solana's
    // packet limit while preserving the actual private transfer instruction.
    const effectiveTransferInstructions = sponsor
      && input.visibility === "private"
      && input.fromBalance === "base"
      && input.toBalance === "base"
      && normalizedTransferInstructions.length > 0
      ? normalizedTransferInstructions.filter(ix => !isProcessPendingTransferQueueRefillInstruction(ix))
      : normalizedTransferInstructions;

    const instructions = [
      ...nativeSolWrapInstructions,
      ...nativeSolRentPdaTopUpInstructions,
      ...gaslessFeeInstructions,
      ...platformFeeInstructions,
      ...effectiveTransferInstructions,
      ...(input.memo !== undefined ? [createMemoInstruction(input.memo)] : []),
    ];

    if (
      !input.legacy
      && input.visibility === "private"
      && input.fromBalance === "base"
      && input.toBalance === "base"
    ) {
      const versionedResponse = await trySerializePrivateBaseToBaseTransferTransactionWithLookupTable(
        env,
        config,
        instructions,
        feePayer,
        blockhash,
        validator,
        sponsor ? [sponsor] : [],
        fees,
      );

      if (versionedResponse) {
        return versionedResponse;
      }
    }
    return serializeTransaction(
      "transfer",
      sendTo,
      instructions,
      feePayer,
      blockhash,
      validator,
      sponsor ? [sponsor] : [],
      input.fromBalance,
      fees,
      sendTo === "ephemeral" ? config.ephemeralRpcUrl : undefined,
    );
  } catch (error) {
    throwTransactionBuildError(error);
  }
}

async function getBalanceInternal(
  env: AppEnv,
  input: BalanceRequest,
  location: SendTarget,
  authToken?: string,
): Promise<BalanceResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const owner = parsePublicKey(input.address, "address");
  const mint = parsePublicKey(input.mint, "mint");
  const tokenProgram = await resolveMintTokenProgram(config, mint);
  const ata = getAssociatedTokenAddressSync(mint, owner, true, tokenProgram);
  const connection = location === "base"
    ? getBaseConnection(config)
    : getEphemeralConnection(config, authToken);

  try {
    const accountInfo = await connection.getAccountInfo(ata, "confirmed");
    const balance = accountInfo ? (parseTokenAmount(accountInfo) ?? 0n) : 0n;

    return {
      address: owner.toBase58(),
      mint: mint.toBase58(),
      ata: ata.toBase58(),
      location,
      balance: balance.toString(),
    };
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch token balance", {
      location,
      message: getSanitizedErrorMessage(error),
    });
  }
}

export function getBaseBalance(env: AppEnv, input: BalanceRequest) {
  return getBalanceInternal(env, input, "base");
}

export async function getPrivateBalance(env: AppEnv, input: BalanceRequest, authToken?: string) {
  const config = resolveRpcConfig(env, input.cluster);
  const owner = parsePublicKey(input.address, "address");
  const mint = parsePublicKey(input.mint, "mint");
  const tokenProgram = await resolveMintTokenProgram(config, mint);
  const ata = getAssociatedTokenAddressSync(mint, owner, true, tokenProgram);
  const [eata] = deriveEphemeralAta(owner, mint);
  const zeroBalanceResponse: BalanceResponse = {
    address: owner.toBase58(),
    mint: mint.toBase58(),
    ata: ata.toBase58(),
    location: "ephemeral",
    balance: "0",
  };

  try {
    const delegationRecord = await getDelegationRecord(getBaseConnection(config), eata);
    if (delegationRecord.status !== DelegationStatus.Delegated) {
      return zeroBalanceResponse;
    }

    const validator = await resolveRequiredValidator(config);
    if (!delegationRecord.validator.equals(validator)) {
      throw new ApiError(400, "EATA_DELEGATED_ELSEWHERE", "eATA is delegated to a different validator", {
        eata: eata.toBase58(),
        delegatedValidator: delegationRecord.validator.toBase58(),
        selectedValidator: validator.toBase58(),
      });
    }

    const accountInfo = await getEphemeralConnection(config, authToken).getAccountInfo(ata, "confirmed");
    const balance = accountInfo ? (parseTokenAmount(accountInfo) ?? 0n) : 0n;

    return {
      address: owner.toBase58(),
      mint: mint.toBase58(),
      ata: ata.toBase58(),
      location: "ephemeral",
      balance: balance.toString(),
    };
  } catch (error) {
    if (error instanceof ApiError) {
      throw error;
    }

    throw new ApiError(502, "RPC_ERROR", "Failed to fetch token balance", {
      location: "ephemeral",
      message: getSanitizedErrorMessage(error),
    });
  }
}

function getNewestSuccessfulSignatureTimestampMs(signatures: Array<{ blockTime?: number | null; err?: unknown }>) {
  let newestSignatureTimestampMs: number | undefined;

  for (const signature of signatures) {
    if (signature.err !== null && signature.err !== undefined) {
      continue;
    }

    if (signature.blockTime === null || signature.blockTime === undefined) {
      continue;
    }

    const signatureTimestampMs = signature.blockTime * 1000;

    if (newestSignatureTimestampMs === undefined || signatureTimestampMs > newestSignatureTimestampMs) {
      newestSignatureTimestampMs = signatureTimestampMs;
    }
  }

  return newestSignatureTimestampMs;
}

function isAuthFilteredRpcError(error: unknown) {
  const message = getSanitizedErrorMessage(error).toLowerCase();
  return message.includes("missing token")
    || message.includes("invalidtoken")
    || message.includes("invalid token")
    || message.includes("401 unauthorized")
    || message.includes("access denied")
    || message.includes("unauthorized");
}

function shouldForceTransferQueueCrankAfterAuthError(transferQueue: PublicKey) {
  const key = transferQueue.toBase58();
  const count = (transferQueueAuthErrorCounts.get(key) ?? 0) + 1;
  transferQueueAuthErrorCounts.set(key, count);

  if (count % TRANSFER_QUEUE_AUTH_ERROR_FORCE_INTERVAL !== 0) {
    return false;
  }

  transferQueueAuthErrorCounts.set(key, 0);
  return true;
}

async function ensureTransferQueueCrankRunning(
  config: RpcConfig,
  transferQueue: PublicKey,
  validator: PublicKey,
  options: EnsureTransferQueueCrankOptions = {},
): Promise<string | undefined> {
  const force = options.force === true;
  const activityConnection = getEphemeralConnection(config);
  let activityWasAuthFiltered = false;
  try {
    const signatures = await activityConnection.getSignaturesForAddress(
      transferQueue,
      { limit: TRANSFER_QUEUE_RECENT_SIGNATURE_LIMIT },
      "confirmed",
    );
    transferQueueAuthErrorCounts.delete(transferQueue.toBase58());
    const newestSignatureTimestampMs = getNewestSuccessfulSignatureTimestampMs(signatures);
    const newestSignatureAgeMs = newestSignatureTimestampMs === undefined
      ? undefined
      : Date.now() - newestSignatureTimestampMs;

    if (
      !force
      && newestSignatureAgeMs !== undefined
      && newestSignatureAgeMs < TRANSFER_QUEUE_STALE_MS
    ) {
      console.warn("ensureTransferQueueCrankRunning (early return): ", newestSignatureTimestampMs, newestSignatureAgeMs, TRANSFER_QUEUE_STALE_MS);
      return;
    }
    console.warn("ensureTransferQueueCrankRunning (ensuring): ", newestSignatureTimestampMs, newestSignatureAgeMs, TRANSFER_QUEUE_STALE_MS);
  } catch (error) {
    if (!isAuthFilteredRpcError(error)) {
      throw error;
    }

    if (!force && !shouldForceTransferQueueCrankAfterAuthError(transferQueue)) {
      return;
    }

    activityWasAuthFiltered = true;
    console.warn("Forcing transfer queue crank after auth-filtered activity checks", {
      transferQueue: transferQueue.toBase58(),
      validator: validator.toBase58(),
    });
  }

  const crankAuthToken = activityWasAuthFiltered && config.transferQueueCrankRpcUrl === config.ephemeralRpcUrl
    ? await createThrowawayAuthToken(config.transferQueueCrankRpcUrl)
    : undefined;
  const crankConnection = getConnectionWithOptionalAuthToken(config.transferQueueCrankRpcUrl, crankAuthToken);
  const payer = Keypair.generate();
  const magicFeeVault = magicFeeVaultPdaFromValidator(validator);
  const transaction = new Transaction().add(
    ensureTransferQueueCrankIx(
      payer.publicKey,
      transferQueue,
      magicFeeVault,
    ),
  );
  transaction.feePayer = payer.publicKey;

  const {
    context: blockhashContext,
    value: { blockhash, lastValidBlockHeight },
  } = await crankConnection.getLatestBlockhashAndContext("confirmed");
  const epochInfo = await crankConnection.getEpochInfo({
    commitment: "confirmed",
    minContextSlot: blockhashContext.slot,
  });
  const currentBlockHeight = epochInfo.blockHeight;
  if (currentBlockHeight === undefined) {
    throw new Error("Transfer queue crank failed to fetch current block height");
  }

  if (currentBlockHeight > lastValidBlockHeight) {
    throw new Error(`Transfer queue crank blockhash expired before send: currentBlockHeight=${currentBlockHeight}, lastValidBlockHeight=${lastValidBlockHeight}`);
  }

  console.log("Preparing transfer queue crank", {
    transferQueue: transferQueue.toBase58(),
    validator: validator.toBase58(),
    crankRpcUrl: config.transferQueueCrankRpcUrl,
    crankAuth: crankAuthToken ? "throwaway" : "none",
    blockhash,
    blockhashContextSlot: blockhashContext.slot,
    currentBlockHeight,
    lastValidBlockHeight,
  });

  transaction.recentBlockhash = blockhash;
  transaction.lastValidBlockHeight = lastValidBlockHeight;
  transaction.sign(payer);

  const signature = await crankConnection.sendRawTransaction(transaction.serialize(), {
    skipPreflight: true,
    preflightCommitment: "confirmed",
  });
  console.log("Sent transfer queue crank", {
    transferQueue: transferQueue.toBase58(),
    validator: validator.toBase58(),
    crankRpcUrl: config.transferQueueCrankRpcUrl,
    crankAuth: crankAuthToken ? "throwaway" : "none",
    signature,
    blockhash,
    blockhashContextSlot: blockhashContext.slot,
    currentBlockHeight,
    lastValidBlockHeight,
  });
  const confirmation = await crankConnection.confirmTransaction({
    signature,
    blockhash,
    lastValidBlockHeight,
  }, "confirmed");

  if (confirmation.value.err !== null) {
    throw new Error(`Transfer queue crank transaction failed: ${JSON.stringify(confirmation.value.err)}`);
  }

  return signature;
}

function scheduleTransferQueueCrank(
  backgroundScheduler: BackgroundTaskScheduler | undefined,
  config: RpcConfig,
  transferQueue: PublicKey,
  validator: PublicKey,
) {
  console.warn("scheduleTransferQueueCrank: ", backgroundScheduler ? "backgroundScheduler exists" : "backgroundScheduler doesn't exist");
  if (!backgroundScheduler) {
    return;
  }

  backgroundScheduler.waitUntil(
    ensureTransferQueueCrankRunning(config, transferQueue, validator).catch((error) => {
      console.error("Failed to ensure transfer queue crank", {
        transferQueue: transferQueue.toBase58(),
        validator: validator.toBase58(),
        message: getSanitizedErrorMessage(error),
      });
    }),
  );
}

export async function ensureTransferQueueCrank(
  env: AppEnv,
  input: TransferQueueEnsureCrankRequest,
): Promise<TransferQueueEnsureCrankResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const mint = parsePublicKey(input.mint, "mint");
  const validator = await resolveRequiredValidator(config, input.validator);
  const [transferQueue] = deriveTransferQueue(mint, validator);

  let accountInfo: { owner: PublicKey } | null;
  try {
    accountInfo = await getBaseConnection(config).getAccountInfo(transferQueue, "confirmed");
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch transfer queue account", {
      transferQueue: transferQueue.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }

  if (accountInfo === null || !accountInfo.owner.equals(DELEGATION_PROGRAM_ID)) {
    throw new ApiError(400, "TRANSFER_QUEUE_NOT_INITIALIZED", "Transfer queue is not initialized and delegated", {
      mint: mint.toBase58(),
      validator: validator.toBase58(),
      transferQueue: transferQueue.toBase58(),
      owner: accountInfo?.owner.toBase58(),
    });
  }

  try {
    const crankSignature = await ensureTransferQueueCrankRunning(
      config,
      transferQueue,
      validator,
      { force: true },
    );

    if (!crankSignature) {
      throw new Error("Forced transfer queue crank did not produce a signature");
    }

    return {
      mint: mint.toBase58(),
      validator: validator.toBase58(),
      transferQueue: transferQueue.toBase58(),
      crankSignature,
    };
  } catch (error) {
    if (error instanceof ApiError) {
      throw error;
    }

    throw new ApiError(502, "RPC_ERROR", "Failed to ensure transfer queue crank", {
      mint: mint.toBase58(),
      validator: validator.toBase58(),
      transferQueue: transferQueue.toBase58(),
      message: getSanitizedErrorMessage(error),
    });
  }
}

export async function getMintInitializationStatus(
  env: AppEnv,
  input: MintInitializationRequest,
  backgroundScheduler?: BackgroundTaskScheduler,
): Promise<MintInitializationResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const mint = parsePublicKey(input.mint, "mint");
  const validator = await resolveRequiredValidator(config, input.validator);
  const [transferQueue] = deriveTransferQueue(mint, validator);
  const connection = getBaseConnection(config);

  try {
    const accountInfo = await connection.getAccountInfo(transferQueue, "confirmed");
    const initialized = accountInfo !== null
      && accountInfo.owner.equals(DELEGATION_PROGRAM_ID);

    console.warn("getMintInitializationStatus: ", accountInfo ? `${transferQueue.toBase58()} exists` : `${transferQueue.toBase58()} doesn't exist`, initialized);

    if (initialized) {
      scheduleTransferQueueCrank(backgroundScheduler, config, transferQueue, validator);
    }

    return {
      mint: mint.toBase58(),
      validator: validator.toBase58(),
      transferQueue: transferQueue.toBase58(),
      initialized,
    };
  } catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch transfer queue account", {
      message: getSanitizedErrorMessage(error),
    });
  }
}
