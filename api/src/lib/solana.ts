import {
  delegateTransferQueueIx,
  deriveRentPda,
  deriveTransferQueue,
  delegateSpl,
  ensureTransferQueueCrankIx,
  initRentPdaIx,
  initTransferQueueIx,
  magicFeeVaultPdaFromValidator,
  transferSpl,
  withdrawSpl, initVaultIx, initVaultAtaIx, delegateEphemeralAtaIx, deriveVault, deriveEphemeralAta, deriveVaultAta,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import {
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemProgram,
  Transaction,
  TransactionInstruction,
} from "@solana/web3.js";

import type { AppEnv } from "../env";
import { ApiError } from "./errors";

export const TOKEN_PROGRAM_ID = new PublicKey(
  "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
);

const ASSOCIATED_TOKEN_PROGRAM_ID = new PublicKey(
  "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
);

const NOOP_PROGRAM_ID = new PublicKey(
  "noopb9bkMVfRPU8AsbpTUg8AQkHtKwMYZiFUjNRtMmV",
);
const MEMO_PROGRAM_ID = new PublicKey(
  "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
);

const DEFAULT_DEPOSIT_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEFAULT_DEPOSIT_DEVNET_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const DEFAULT_FALLBACK_VALIDATOR = new PublicKey(
  "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
);
const TRANSFER_QUEUE_RENT_LAMPORTS = LAMPORTS_PER_SOL / 50;
const PRIVATE_TRANSFER_MAX_DELAY_MS_LIMIT = 10n * 60n * 1000n;
const TRANSFER_QUEUE_RECENT_SIGNATURE_LIMIT = 5;
const TRANSFER_QUEUE_STALE_MS = 60_000;

const connectionCache = new Map<string, Connection>();
const validatorCache = new Map<string, Promise<PublicKey | undefined>>();

type SendTarget = "base" | "ephemeral";

type BlockhashResult = {
  blockhash: string;
  lastValidBlockHeight: number;
};

type RpcConfig = {
  baseRpcUrl: string;
  ephemeralRpcUrl: string;
  cluster: "mainnet" | "devnet" | "custom";
};

type TransactionResponse = {
  kind: "deposit" | "withdraw" | "transfer" | "initializeMint";
  version: "legacy";
  transactionBase64: string;
  sendTo: SendTarget;
  recentBlockhash: string;
  lastValidBlockHeight: number;
  instructionCount: number;
  requiredSigners: string[];
  validator?: string;
};

type DepositInput = {
  owner: string;
  mint?: string;
  amount: string | number;
  cluster?: string;
  validator?: string;
  initIfMissing?: boolean;
  initVaultIfMissing?: boolean;
  initAtasIfMissing?: boolean;
  idempotent?: boolean;
};

type WithdrawInput = {
  owner: string;
  mint: string;
  amount: string | number;
  cluster?: string;
  validator?: string;
  initIfMissing?: boolean;
  initAtasIfMissing?: boolean;
  escrowIndex?: number;
  idempotent?: boolean;
};

type InitializeMintTransactionInput = {
  payer: string;
  mint: string;
  cluster?: string;
  validator?: string;
};

type TransferInput = {
  from: string;
  to: string;
  mint: string;
  amount: string | number;
  cluster?: string;
  fromBalance: "base" | "ephemeral";
  toBalance: "base" | "ephemeral";
  visibility: "public" | "private";
  validator?: string;
  initIfMissing?: boolean;
  initAtasIfMissing?: boolean;
  initVaultIfMissing?: boolean;
  memo?: string;
  minDelayMs?: string;
  maxDelayMs?: string;
  clientRefId?: string;
  split?: number;
};

type BalanceInput = {
  address: string;
  mint: string;
  cluster?: string;
};

type MintInitializationInput = {
  mint: string;
  validator?: string;
  cluster?: string;
};

type BalanceResponse = {
  address: string;
  mint: string;
  ata: string;
  location: SendTarget;
  balance: string;
};

type MintInitializationResponse = {
  mint: string;
  validator: string;
  transferQueue: string;
  initialized: boolean;
};

type InitializeMintTransactionResponse = TransactionResponse & {
  kind: "initializeMint";
  validator: string;
  transferQueue: string;
  rentPda: string;
};

type RpcIdentityResponse = {
  result?: {
    identity?: string;
  };
  error?: {
    message?: string;
  };
};

type BackgroundTaskScheduler = {
  waitUntil: (promise: Promise<unknown>) => void;
};

function getConnection(endpoint: string) {
  let connection = connectionCache.get(endpoint);

  if (!connection) {
    connection = new Connection(endpoint, "confirmed");
    connectionCache.set(endpoint, connection);
  }

  return connection;
}

function getBaseConnection(config: RpcConfig) {
  return getConnection(config.baseRpcUrl);
}

function getEphemeralConnection(config: RpcConfig, authToken?: string) {
  if (!authToken) {
    return getConnection(config.ephemeralRpcUrl);
  }

  const url = new URL(config.ephemeralRpcUrl);
  url.searchParams.set("token", authToken);
  return new Connection(url.toString(), "confirmed");
}

function createClusterConfigError(missingVars: Array<"BASE_DEVNET_RPC_URL" | "EPHEMERAL_DEVNET_RPC_URL">) {
  return new ApiError(
    500,
    "CONFIG_ERROR",
    "Missing worker environment variables for cluster=devnet",
    {
      issues: missingVars.map((name) => ({
        path: [name],
        message: "Required for cluster=devnet",
      })),
      hint: "Set BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL before using cluster=devnet.",
    },
  );
}

export function resolveRpcConfig(env: AppEnv, cluster?: string): RpcConfig {
  const value = cluster?.trim();
  const normalized = value?.toLowerCase();

  if (!value || normalized === "mainnet") {
    return {
      baseRpcUrl: env.BASE_RPC_URL,
      ephemeralRpcUrl: env.EPHEMERAL_RPC_URL,
      cluster: "mainnet",
    };
  }

  if (normalized === "devnet") {
    const missingVars = [
      ...(!env.BASE_DEVNET_RPC_URL ? ["BASE_DEVNET_RPC_URL" as const] : []),
      ...(!env.EPHEMERAL_DEVNET_RPC_URL ? ["EPHEMERAL_DEVNET_RPC_URL" as const] : []),
    ];

    if (missingVars.length > 0) {
      throw createClusterConfigError(missingVars);
    }

    return {
      baseRpcUrl: env.BASE_DEVNET_RPC_URL!,
      ephemeralRpcUrl: env.EPHEMERAL_DEVNET_RPC_URL!,
      cluster: "devnet",
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
      cluster: "custom",
    };
  }
  catch {
    throw new ApiError(400, "INVALID_CLUSTER", "cluster must be \"mainnet\", \"devnet\", or a valid http(s) URL");
  }
}

function parsePublicKey(value: string, fieldName: string) {
  try {
    return new PublicKey(value);
  }
  catch {
    throw new ApiError(400, "INVALID_PUBLIC_KEY", `Invalid ${fieldName}`);
  }
}

function parseAmount(value: string | number, fieldName: string) {
  try {
    const amount = typeof value === "number"
      ? (() => {
        if (!Number.isSafeInteger(value) || value <= 0) {
          throw new Error("non-positive");
        }

        return BigInt(value);
      })()
      : BigInt(value);

    if (amount <= 0n) {
      throw new Error("non-positive");
    }
    return amount;
  }
  catch {
    throw new ApiError(400, "INVALID_AMOUNT", `${fieldName} must be a positive integer string`);
  }
}

function parseOptionalAmount(value: string | undefined, fieldName: string) {
  if (value === undefined) {
    return undefined;
  }

  try {
    return BigInt(value);
  }
  catch {
    throw new ApiError(400, "INVALID_AMOUNT", `${fieldName} must be an integer string`);
  }
}

function parseOptionalPublicKey(value: string | undefined, fieldName: string) {
  return value ? parsePublicKey(value, fieldName) : undefined;
}

function getAssociatedTokenAddressSync(
  mint: PublicKey,
  owner: PublicKey,
  allowOwnerOffCurve: boolean = false,
  programId: PublicKey = TOKEN_PROGRAM_ID,
  associatedTokenProgramId: PublicKey = ASSOCIATED_TOKEN_PROGRAM_ID,
) {
  if (!allowOwnerOffCurve && !PublicKey.isOnCurve(owner.toBuffer())) {
    throw new ApiError(400, "INVALID_OWNER", "Owner public key is off-curve");
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

function createNoopInstruction() {
  return new TransactionInstruction({
    programId: NOOP_PROGRAM_ID,
    keys: [],
    data: Buffer.from(crypto.getRandomValues(new Uint8Array(5))),
  });
}

function createMemoInstruction(memo: string) {
  return new TransactionInstruction({
    programId: MEMO_PROGRAM_ID,
    keys: [],
    data: Buffer.from(memo, "utf8"),
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
          endpoint,
          status: response.status,
        });
      }

      const payload = await response.json() as RpcIdentityResponse;

      if (payload.error) {
        throw new ApiError(502, "RPC_ERROR", payload.error.message || "Failed to resolve validator identity", {
          endpoint,
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
  }
  catch {
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
      endpoint: config.ephemeralRpcUrl,
    });
  }

  return validator;
}

async function getBlockhash(config: RpcConfig, source: SendTarget, authToken?: string): Promise<BlockhashResult> {
  const connection = source === "base"
    ? getBaseConnection(config)
    : getEphemeralConnection(config, authToken);

  try {
    const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
    return { blockhash, lastValidBlockHeight };
  }
  catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch recent blockhash", {
      source,
      message: error instanceof Error ? error.message : String(error),
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

function serializeTransaction(
  kind: TransactionResponse["kind"],
  sendTo: SendTarget,
  instructions: TransactionInstruction[],
  feePayer: PublicKey,
  blockhash: BlockhashResult,
  validator?: PublicKey,
): TransactionResponse {
  const transaction = new Transaction();
  transaction.feePayer = feePayer;
  transaction.recentBlockhash = blockhash.blockhash;
  transaction.add(...instructions);

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
    recentBlockhash: blockhash.blockhash,
    lastValidBlockHeight: blockhash.lastValidBlockHeight,
    instructionCount: instructions.length,
    requiredSigners: getRequiredSigners(feePayer, instructions),
    validator: validator?.toBase58(),
  };
}

export async function buildDepositTransaction(env: AppEnv, input: DepositInput) {
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
  const blockhash = await getBlockhash(config, "base");

  const instructions = await delegateSpl(owner, mint, amount, {
    payer,
    validator,
    initIfMissing: input.initIfMissing,
    initVaultIfMissing: input.initVaultIfMissing,
    initAtasIfMissing: input.initAtasIfMissing,
    shuttleId: createRandomShuttleId(),
    escrowIndex: 0,
    idempotent: input.idempotent,
  });

  return serializeTransaction(
    "deposit",
    "base",
    instructions,
    feePayer,
    blockhash,
    validator,
  );
}

export async function buildWithdrawTransaction(env: AppEnv, input: WithdrawInput) {
  const config = resolveRpcConfig(env, input.cluster);
  const owner = parsePublicKey(input.owner, "owner");
  const mint = parsePublicKey(input.mint, "mint");
  const amount = parseAmount(input.amount, "amount");
  const payer = owner;
  const feePayer = owner;
  const validator = await resolveValidator(config, input.validator);
  const blockhash = await getBlockhash(config, "base");

  const instructions = await withdrawSpl(owner, mint, amount, {
    payer,
    validator,
    initIfMissing: input.initIfMissing,
    initAtasIfMissing: input.initAtasIfMissing,
    shuttleId: createRandomShuttleId(),
    escrowIndex: input.escrowIndex,
    idempotent: input.idempotent,
  });

  return serializeTransaction(
    "withdraw",
    "base",
    instructions,
    feePayer,
    blockhash,
    validator,
  );
}

export async function buildInitializeMintTransaction(
  env: AppEnv,
  input: InitializeMintTransactionInput,
): Promise<InitializeMintTransactionResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const payer = parsePublicKey(input.payer, "payer");
  const mint = parsePublicKey(input.mint, "mint");
  const validator = await resolveRequiredValidator(config, input.validator);
  const [transferQueue] = deriveTransferQueue(mint, validator);
  const [rentPda] = deriveRentPda();
  const [vault] = deriveVault(mint);
  const [vaultEphemeralAta] = deriveEphemeralAta(vault, mint);
  const vaultAta = deriveVaultAta(mint, vault);
  const blockhash = await getBlockhash(config, "base");

  const instructions = [
    initTransferQueueIx(
      payer,
      transferQueue,
      mint,
      validator,
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
    initVaultIx(vault, mint, payer),
    initVaultAtaIx(payer, vaultAta, vault, mint),
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
}

export async function buildTransferTransaction(env: AppEnv, input: TransferInput, authToken?: string) {
  const config = resolveRpcConfig(env, input.cluster);
  const from = parsePublicKey(input.from, "from");
  const to = parsePublicKey(input.to, "to");
  const mint = parsePublicKey(input.mint, "mint");
  const amount = parseAmount(input.amount, "amount");
  const payer = from;
  const feePayer = from;
  const shuttleId = createRandomShuttleId();

  const minDelayMs = parseOptionalAmount(input.minDelayMs, "minDelayMs");
  const maxDelayMs = parseOptionalAmount(input.maxDelayMs, "maxDelayMs");
  const clientRefId = parseOptionalAmount(input.clientRefId, "clientRefId");
  const split = input.split;

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

  if (split !== undefined && BigInt(split) > amount) {
    throw new ApiError(400, "INVALID_PRIVATE_TRANSFER", "split cannot exceed amount");
  }

  const shouldResolveValidator = input.validator
    || input.visibility === "private"
    || input.fromBalance === "base"
    || input.initVaultIfMissing;

  const validator = shouldResolveValidator
    ? await resolveValidator(config, input.validator)
    : undefined;

  const sendTo: SendTarget = input.fromBalance === "ephemeral" ? "ephemeral" : "base";
  const blockhash = await getBlockhash(config, sendTo, authToken);

  try {
    const instructions = [
      createNoopInstruction(),
      ...(await transferSpl(from, to, mint, amount, {
        visibility: input.visibility,
        fromBalance: input.fromBalance,
        toBalance: input.toBalance,
        payer,
        validator,
        initIfMissing: input.initIfMissing,
        initAtasIfMissing: input.initAtasIfMissing,
        initVaultIfMissing: input.initVaultIfMissing,
        shuttleId,
        privateTransfer: input.minDelayMs !== undefined
          || input.maxDelayMs !== undefined
          || input.clientRefId !== undefined
          || input.split !== undefined
          ? {
            minDelayMs,
            maxDelayMs,
            clientRefId,
            split,
          }
          : undefined,
      })),
      ...(input.memo !== undefined ? [createMemoInstruction(input.memo)] : []),
    ];

    return serializeTransaction(
      "transfer",
      sendTo,
      instructions,
      feePayer,
      blockhash,
      validator,
    );
  }
  catch (error) {
    if (error instanceof Error && error.message.includes("transferSpl route not implemented")) {
      throw new ApiError(400, "UNSUPPORTED_TRANSFER_ROUTE", error.message);
    }

    throw error;
  }
}

async function getBalanceInternal(
  env: AppEnv,
  input: BalanceInput,
  location: SendTarget,
  authToken?: string,
): Promise<BalanceResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const owner = parsePublicKey(input.address, "address");
  const mint = parsePublicKey(input.mint, "mint");
  const ata = getAssociatedTokenAddressSync(mint, owner, false, TOKEN_PROGRAM_ID);
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
  }
  catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch token balance", {
      location,
      message: error instanceof Error ? error.message : String(error),
    });
  }
}

export function getBaseBalance(env: AppEnv, input: BalanceInput) {
  return getBalanceInternal(env, input, "base");
}

export function getPrivateBalance(env: AppEnv, input: BalanceInput, authToken?: string) {
  return getBalanceInternal(env, input, "ephemeral", authToken);
}

function getNewestSignatureTimestampMs(signatures: Array<{ blockTime?: number | null }>) {
  let newestSignatureTimestampMs: number | undefined;

  for (const signature of signatures) {
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

async function ensureTransferQueueCrankRunning(
  config: RpcConfig,
  transferQueue: PublicKey,
  validator: PublicKey,
) {
  const connection = getEphemeralConnection(config);
  const signatures = await connection.getSignaturesForAddress(
    transferQueue,
    { limit: TRANSFER_QUEUE_RECENT_SIGNATURE_LIMIT },
    "confirmed",
  );
  const newestSignatureTimestampMs = getNewestSignatureTimestampMs(signatures);

  if (
    newestSignatureTimestampMs !== undefined
    && Date.now() - newestSignatureTimestampMs < TRANSFER_QUEUE_STALE_MS
  ) {
    return;
  }

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

  const { blockhash, lastValidBlockHeight } = await connection.getLatestBlockhash("confirmed");
  transaction.recentBlockhash = blockhash;
  transaction.lastValidBlockHeight = lastValidBlockHeight;
  transaction.sign(payer);

  const signature = await connection.sendRawTransaction(transaction.serialize(), {
    skipPreflight: true,
    preflightCommitment: "confirmed",
  });
  const confirmation = await connection.confirmTransaction({
    signature,
    blockhash,
    lastValidBlockHeight,
  }, "confirmed");

  if (confirmation.value.err !== null) {
    throw new Error(`Transfer queue crank transaction failed: ${JSON.stringify(confirmation.value.err)}`);
  }
}

function scheduleTransferQueueCrank(
  backgroundScheduler: BackgroundTaskScheduler | undefined,
  config: RpcConfig,
  transferQueue: PublicKey,
  validator: PublicKey,
) {
  if (!backgroundScheduler) {
    return;
  }

  backgroundScheduler.waitUntil(
    ensureTransferQueueCrankRunning(config, transferQueue, validator).catch((error) => {
      console.error("Failed to ensure transfer queue crank", {
        transferQueue: transferQueue.toBase58(),
        validator: validator.toBase58(),
        message: error instanceof Error ? error.message : String(error),
      });
    }),
  );
}

export async function getMintInitializationStatus(
  env: AppEnv,
  input: MintInitializationInput,
  backgroundScheduler?: BackgroundTaskScheduler,
): Promise<MintInitializationResponse> {
  const config = resolveRpcConfig(env, input.cluster);
  const mint = parsePublicKey(input.mint, "mint");
  const validator = await resolveRequiredValidator(config, input.validator);
  const [transferQueue] = deriveTransferQueue(mint, validator);
  const connection = getBaseConnection(config);

  try {
    const accountInfo = await connection.getAccountInfo(transferQueue, "confirmed");
    const initialized = accountInfo !== null;

    if (initialized) {
      scheduleTransferQueueCrank(backgroundScheduler, config, transferQueue, validator);
    }

    return {
      mint: mint.toBase58(),
      validator: validator.toBase58(),
      transferQueue: transferQueue.toBase58(),
      initialized,
    };
  }
  catch (error) {
    throw new ApiError(502, "RPC_ERROR", "Failed to fetch transfer queue account", {
      message: error instanceof Error ? error.message : String(error),
    });
  }
}
