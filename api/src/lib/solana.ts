import {
  compileLegacyTransactionToV0,
  DELEGATION_PROGRAM_ID,
  DelegationStatus,
  delegateTransferQueueIx,
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
  transferSpl,
  withdrawSpl, initVaultIx, initVaultAtaIx, delegateEphemeralAtaIx, deriveVault, deriveEphemeralAta, deriveVaultAta,
} from "@magicblock-labs/ephemeral-rollups-sdk";
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
import { BalanceRequest, BalanceResponse, DepositRequest, InitializeMintRequest, InitializeMintResponse, MintInitializationRequest, MintInitializationResponse, TransactionResponse, TransferRequest, WithdrawRequest } from "../routes/spl/spl.schemas";
import { SendTransactionRequest, SendTransactionResponse } from "../routes/transaction.schemas";
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

const MEMO_PROGRAM_ID = new PublicKey(
  "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
);

const DEFAULT_DEPOSIT_MINT = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
const DEFAULT_DEPOSIT_DEVNET_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const MAINNET_USDT_MINT = "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB";
const DEFAULT_FALLBACK_VALIDATOR = new PublicKey(
  "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
);
const TRANSFER_QUEUE_RENT_LAMPORTS = LAMPORTS_PER_SOL / 50;
const PRIVATE_TRANSFER_MAX_DELAY_MS_LIMIT = 10n * 60n * 1000n;
const TRANSFER_QUEUE_RECENT_SIGNATURE_LIMIT = 5;
const TRANSFER_QUEUE_STALE_MS = 60_000;
const TRANSFER_QUEUE_AUTH_ERROR_FORCE_INTERVAL = 100;
const SOLANA_WIRE_TRANSACTION_SIZE_LIMIT = 1232;
// Keep these defaults aligned with scripts/create-private-transfer-lut.js. Updating them requires a redeploy.
const PRIVATE_BASE_TO_BASE_TRANSFER_LOOKUP_TABLES = {
  mainnet: new PublicKey("2J2Pw639kU7U6rj7qUXY5sVXdJqyt4DjEcVxzqmFrFds"),
  devnet: new PublicKey("HFmj4QbofPjhXP2vdnDARDQFw1AucSQTKVAs8df4tkUy"),
} as const;
const PRIVATE_TRANSFER_SETUP_LAMPORTS = 2_039_280n;
const PRIVATE_TRANSFER_FEE_BASIS_POINTS = 10n;
const BASIS_POINTS_FACTOR = 10_000n;
const GASLESS_RELAY_FEE_MICRO_USDC = 200_000n; // 0.2 USDC/USDT
const GASLESS_STABLECOIN_MIN_AMOUNT = BigInt(5 * 1_000_000); // 5 USDC/USDT

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

type TransferFees = NonNullable<TransactionResponse["fees"]>;

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

function createClusterConfigError(missingVars: Array<"BASE_DEVNET_RPC_URL" | "EPHEMERAL_DEVNET_RPC_URL">) {
  return new ApiError(
    500,
    "CONFIG_ERROR",
    "Missing worker environment variables for cluster=devnet",
    {
      issues: missingVars.map(name => ({
        path: [name],
        message: "Required for cluster=devnet",
      })),
      hint: "Set BASE_DEVNET_RPC_URL and EPHEMERAL_DEVNET_RPC_URL before using cluster=devnet.",
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
  if (!cluster) {
    return {
      baseRpcUrl: env.BASE_RPC_URL,
      ephemeralRpcUrl: env.EPHEMERAL_RPC_URL,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_RPC_URL,
      cluster: env.CLUSTER,
    };
  }
  const value = cluster.trim();
  const normalized = value?.toLowerCase();
  if (!value || normalized === "mainnet") {
    return {
      baseRpcUrl: env.BASE_RPC_URL,
      ephemeralRpcUrl: env.EPHEMERAL_RPC_URL,
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_RPC_URL,
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
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL ?? env.EPHEMERAL_DEVNET_RPC_URL!,
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
      transferQueueCrankRpcUrl: env.TRANSFER_QUEUE_CRANK_RPC_URL ?? env.EPHEMERAL_RPC_URL,
      cluster: "custom",
    };
  } catch {
    throw new ApiError(400, "INVALID_CLUSTER", "cluster must be \"mainnet\", \"devnet\", or a valid http(s) URL");
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
  } catch {
    throw new ApiError(400, "INVALID_AMOUNT", `${fieldName} must be a positive integer string`);
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

function createMemoInstruction(memo: string) {
  return new TransactionInstruction({
    programId: MEMO_PROGRAM_ID,
    keys: [],
    data: Buffer.from(memo, "utf8"),
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

async function getBlockhash(config: RpcConfig, source: SendTarget, authToken?: string): Promise<BlockhashResult> {
  const connection = source === "base"
    ? getBaseConnection(config)
    : getEphemeralConnection(config, authToken);

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
  const connection = input.sendTo === "base"
    ? getBaseConnection(config)
    : getEphemeralConnection(config, authToken);
  const confirmationRpcEndpoint = input.sendTo === "base"
    ? config.baseRpcUrl
    : config.ephemeralRpcUrl;
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

    const instructions = await delegateSpl(owner, mint, amount, {
      payer,
      validator,
      tokenProgram,
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

export async function buildTransferTransaction(env: AppEnv, input: TransferRequest, authToken?: string) {
  try {
    const config = resolveRpcConfig(env, input.cluster);
    const from = parsePublicKey(input.from, "from");
    const to = parsePublicKey(input.to, "to");
    const mint = parsePublicKey(input.mint, "mint");
    const amount = parseAmount(input.amount, "amount");
    const shuttleId = createRandomShuttleId();

    const minDelayMs = parseOptionalAmount(input.minDelayMs, "minDelayMs");
    const maxDelayMs = parseOptionalAmount(input.maxDelayMs, "maxDelayMs");
    const clientRefId = parseOptionalAmount(input.clientRefId, "clientRefId");
    const split = input.split;
    const exactOut = input.exactOut;

    console.log("input: ", input);

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
    const privateTransferFee = isPrivateBaseToBaseTransfer(input) ? privateTransferFeeAmount(amount) : 0n;
    const fees = createTransferFees(
      privateTransferSetupLamports(input),
      privateTransferFee + (sponsor ? GASLESS_RELAY_FEE_MICRO_USDC : 0n),
    );

    const shouldResolveValidator = input.validator
      || input.visibility === "private"
      || input.fromBalance === "base"
      || input.initVaultIfMissing;

    const validator = shouldResolveValidator
      ? await resolveValidator(config, input.validator)
      : undefined;

    const tokenProgram = await resolveMintTokenProgram(config, mint);
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

    const transferInstructions = await transferSpl(from, to, mint, amount, {
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
      privateTransfer: input.minDelayMs !== undefined
        || input.maxDelayMs !== undefined
        || input.clientRefId !== undefined
        || input.split !== undefined
        ? {
            minDelayMs,
            maxDelayMs,
            clientRefId,
            split,
            exactOut,
          }
        : undefined,
    });
    // Gasless private base->base already adds a relay-fee token transfer. Dropping
    // the opportunistic queue-refill ix keeps the full transaction under Solana's
    // packet limit while preserving the actual private transfer instruction.
    const effectiveTransferInstructions = sponsor
      && input.visibility === "private"
      && input.fromBalance === "base"
      && input.toBalance === "base"
      && transferInstructions.length > 0
      ? transferInstructions.filter(ix => !isProcessPendingTransferQueueRefillInstruction(ix))
      : transferInstructions;

    const instructions = [
      ...gaslessFeeInstructions,
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
    console.log("instructions: ", instructions);

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
  const ata = getAssociatedTokenAddressSync(mint, owner, true, TOKEN_PROGRAM_ID);
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
  const ata = getAssociatedTokenAddressSync(mint, owner, true, TOKEN_PROGRAM_ID);
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
) {
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

    if (
      newestSignatureTimestampMs !== undefined
      && Date.now() - newestSignatureTimestampMs < TRANSFER_QUEUE_STALE_MS
    ) {
      return;
    }
  } catch (error) {
    if (!isAuthFilteredRpcError(error)) {
      throw error;
    }

    if (!shouldForceTransferQueueCrankAfterAuthError(transferQueue)) {
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
        message: getSanitizedErrorMessage(error),
      });
    }),
  );
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
