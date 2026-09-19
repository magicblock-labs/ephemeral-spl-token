import {
  DelegationStatus,
  deriveEphemeralAta,
  getDelegationRecord,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import { Connection, Keypair, PublicKey, Transaction, TransactionInstruction } from "@solana/web3.js";
import bs58 from "bs58";
import nacl from "tweetnacl";

import type { AppEnv } from "../env";
import { ApiError } from "../lib/errors";
import { ASSOCIATED_TOKEN_PROGRAM_ID, resolveRpcConfig, TOKEN_PROGRAM_ID } from "../lib/solana";
import type { PaymentCluster, PaymentTerms, PreparedPayment } from "./types";

const USDC_MINTS = {
  mainnet: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  devnet: "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
};
const MEMO_PROGRAM_ID = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
const RPC_TIMEOUT_MS = 8_000;
const MAX_CACHE_ENTRIES = 8;
const connections = new Map<string, Connection>();
const sessions = new Map<string, { expiresAt: number; request: Promise<string> }>();

function cacheEntry<T>(cache: Map<string, T>, key: string, value: T) {
  if (cache.size >= MAX_CACHE_ENTRIES) cache.delete(cache.keys().next().value!);
  cache.set(key, value);
  return value;
}

function connection(endpoint: string) {
  const existing = connections.get(endpoint);
  if (existing) return existing;
  return cacheEntry(connections, endpoint, new Connection(endpoint, {
    commitment: "confirmed",
    disableRetryOnRateLimit: true,
    fetch: (input, init) => fetch(input, { ...init, signal: AbortSignal.timeout(RPC_TIMEOUT_MS) }),
  }));
}

async function fetchJson(endpoint: string, init?: RequestInit) {
  const response = await fetch(endpoint, { ...init, signal: AbortSignal.timeout(RPC_TIMEOUT_MS) });
  if (!response.ok) throw new ApiError(502, "PAYMENT_RPC_ERROR", "Payment RPC request failed");
  return response.json();
}

function serviceKeypair(env: AppEnv) {
  try {
    const value = JSON.parse(env.PAYMENTS_RPC_AUTH_SECRET_KEY ?? "null");
    if (!Array.isArray(value) || value.length !== 64
      || value.some(byte => !Number.isInteger(byte) || byte < 0 || byte > 255)) {
      throw new Error("Invalid key");
    }
    return Keypair.fromSecretKey(Uint8Array.from(value));
  } catch {
    throw new ApiError(503, "PAYMENT_RPC_AUTH_UNAVAILABLE", "Private payments require a valid PAYMENTS_RPC_AUTH_SECRET_KEY");
  }
}

export function validatePaymentCluster(env: AppEnv, cluster: PaymentCluster): void {
  resolveRpcConfig(env, cluster);
  if (cluster.endsWith("-private")) serviceKeypair(env);
}

async function serviceToken(env: AppEnv, endpoint: string) {
  const signer = serviceKeypair(env);
  const cacheKey = `${endpoint}:${signer.publicKey.toBase58()}`;
  const existing = sessions.get(cacheKey);
  if (existing && existing.expiresAt > Date.now() + 60_000) return existing.request;
  const entry = { expiresAt: Infinity, request: Promise.resolve("") };
  entry.request = (async () => {
    const authEndpoint = new URL(endpoint);
    authEndpoint.pathname = `${authEndpoint.pathname.replace(/\/$/, "")}/auth/challenge`;
    authEndpoint.searchParams.set("pubkey", signer.publicKey.toBase58());
    const challenge = await fetchJson(authEndpoint.toString()) as { challenge?: string };
    if (typeof challenge.challenge !== "string" || !challenge.challenge.length) {
      throw new ApiError(502, "PAYMENT_RPC_AUTH_ERROR", "Payment RPC returned an invalid authentication challenge");
    }
    authEndpoint.pathname = authEndpoint.pathname.replace(/challenge$/, "login");
    authEndpoint.searchParams.delete("pubkey");
    const login = await fetchJson(authEndpoint.toString(), {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        pubkey: signer.publicKey.toBase58(),
        challenge: challenge.challenge,
        signature: bs58.encode(nacl.sign.detached(Buffer.from(challenge.challenge), signer.secretKey)),
      }),
    }) as { token?: string; expiresAt?: number };
    if (typeof login.token !== "string" || !login.token.length
      || (login.expiresAt !== undefined && (!Number.isFinite(login.expiresAt) || login.expiresAt <= Date.now() + 60_000))) {
      throw new ApiError(502, "PAYMENT_RPC_AUTH_ERROR", "Payment RPC returned an invalid authentication token");
    }
    // Refresh periodically even if the gateway issues a long-lived token.
    entry.expiresAt = Math.min(login.expiresAt ?? Infinity, Date.now() + 3_600_000);
    return login.token;
  })().catch((error) => {
    if (sessions.get(cacheKey) === entry) sessions.delete(cacheKey);
    throw error;
  });
  cacheEntry(sessions, cacheKey, entry);
  return entry.request;
}

async function paymentRpc(env: AppEnv, terms: PaymentTerms, prepared?: PreparedPayment) {
  if (!["mainnet", "devnet", "mainnet-private", "devnet-private"].includes(terms.cluster)) {
    throw new ApiError(400, "INVALID_PAYMENT_CLUSTER", "Payments require a configured mainnet or devnet cluster");
  }
  const config = resolveRpcConfig(env, terms.cluster);
  if (prepared && prepared.rpcEndpoint !== config.ephemeralRpcUrl) {
    throw new ApiError(503, "PAYMENT_RPC_CHANGED", "Payment RPC configuration changed; reconcile using the original configured validator");
  }
  const endpoint = new URL(config.ephemeralRpcUrl);
  if (terms.cluster.endsWith("-private")) {
    endpoint.searchParams.set("token", await serviceToken(env, config.ephemeralRpcUrl));
  }
  return { config, endpoint: endpoint.toString(), connection: connection(endpoint.toString()) };
}

async function validatorIdentity(endpoint: string, expected?: string) {
  const response = await fetchJson(endpoint, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "getIdentity", params: [] }),
  }) as { result?: { identity?: string }; error?: unknown };
  let identity: PublicKey;
  try {
    if (response.error || !response.result?.identity) throw new Error("Missing identity");
    identity = new PublicKey(response.result.identity);
  } catch {
    throw new ApiError(502, "PAYMENT_RPC_IDENTITY_ERROR", "Payment RPC did not return a valid validator identity");
  }
  if (expected && identity.toBase58() !== expected) {
    throw new ApiError(503, "PAYMENT_VALIDATOR_CHANGED", "Payment RPC no longer serves the prepared validator");
  }
  return identity;
}

function paymentAccounts(terms: PaymentTerms) {
  const expectedMint = terms.cluster.startsWith("devnet") ? USDC_MINTS.devnet : USDC_MINTS.mainnet;
  if (terms.mint !== expectedMint) {
    throw new ApiError(400, "UNSUPPORTED_PAYMENT_MINT", "Payments support only the cluster's USDC mint (6 decimals)");
  }
  if (!/^[1-9]\d{0,19}$/.test(terms.amount) || BigInt(terms.amount) > 0xffffffffffffffffn) {
    throw new ApiError(400, "INVALID_PAYMENT_AMOUNT", "Payment amount must be a positive u64 integer in USDC base units");
  }
  let payer: PublicKey;
  let recipient: PublicKey;
  try {
    payer = new PublicKey(terms.payer);
    recipient = new PublicKey(terms.recipient);
    if (!PublicKey.isOnCurve(payer.toBytes()) || payer.equals(recipient)) throw new Error("Invalid payer");
  } catch {
    throw new ApiError(400, "INVALID_PAYMENT_ACCOUNTS", "Payment requires a signer wallet and a distinct recipient");
  }
  const mint = new PublicKey(expectedMint);
  const ata = (owner: PublicKey) => PublicKey.findProgramAddressSync(
    [owner.toBuffer(), TOKEN_PROGRAM_ID.toBuffer(), mint.toBuffer()],
    ASSOCIATED_TOKEN_PROGRAM_ID,
  )[0];
  return { payer, recipient, mint, source: ata(payer), destination: ata(recipient) };
}

function paymentTransaction(terms: PaymentTerms, recentBlockhash: string) {
  const { payer, source, destination } = paymentAccounts(terms);
  const data = Buffer.alloc(9);
  data[0] = 3; // SPL Token Transfer; no account creation, relay, or priority fee.
  data.writeBigUInt64LE(BigInt(terms.amount), 1);
  return new Transaction({ feePayer: payer, recentBlockhash }).add(
    new TransactionInstruction({
      programId: TOKEN_PROGRAM_ID,
      keys: [
        { pubkey: source, isSigner: false, isWritable: true },
        { pubkey: destination, isSigner: false, isWritable: true },
        { pubkey: payer, isSigner: true, isWritable: false },
      ],
      data,
    }),
    new TransactionInstruction({
      programId: MEMO_PROGRAM_ID,
      keys: [],
      data: Buffer.from(`payment:${terms.id}`),
    }),
  );
}

async function requireZeroFee(rpc: Connection, transaction: Transaction) {
  const fee = await rpc.getFeeForMessage(transaction.compileMessage(), "confirmed");
  if (fee.value !== 0) {
    throw new ApiError(409, "PAYMENT_ZERO_FEE_REQUIRED", "Payment requires a valid transaction on a zero-fee validator");
  }
}

export async function preparePayment(env: AppEnv, terms: PaymentTerms): Promise<PreparedPayment> {
  const { payer, recipient, mint } = paymentAccounts(terms);
  const rpc = await paymentRpc(env, terms);
  const base = connection(rpc.config.baseRpcUrl);
  // Delegation records are public on the base chain. Private token balances are not read.
  const [validator, blockhash, sender, receiver] = await Promise.all([
    validatorIdentity(rpc.endpoint),
    rpc.connection.getLatestBlockhash("confirmed"),
    getDelegationRecord(base, deriveEphemeralAta(payer, mint)[0]),
    getDelegationRecord(base, deriveEphemeralAta(recipient, mint)[0]),
  ]);
  for (const record of [sender, receiver]) {
    if (record.status !== DelegationStatus.Delegated || !record.validator.equals(validator)) {
      throw new ApiError(409, "PAYMENT_ACCOUNTS_NOT_DELEGATED", "Both USDC accounts must already be delegated to the configured validator");
    }
  }
  const transaction = paymentTransaction(terms, blockhash.blockhash);
  await requireZeroFee(rpc.connection, transaction);
  return {
    transactionBase64: transaction.serialize({ requireAllSignatures: false }).toString("base64"),
    messageBase64: transaction.serializeMessage().toString("base64"),
    recentBlockhash: blockhash.blockhash,
    lastValidBlockHeight: blockhash.lastValidBlockHeight,
    validator: validator.toBase58(),
    rpcEndpoint: rpc.config.ephemeralRpcUrl,
  };
}

export function validateSignedPayment(terms: PaymentTerms, prepared: PreparedPayment, transactionBase64: string) {
  try {
    if (transactionBase64.length > 1644 || !/^[A-Za-z0-9+/]+={0,2}$/.test(transactionBase64)) throw new Error("Invalid encoding");
    const bytes = Buffer.from(transactionBase64, "base64");
    if (bytes.length > 1232 || bytes.toString("base64") !== transactionBase64) throw new Error("Invalid wire transaction");
    const transaction = Transaction.from(bytes);
    const message = transaction.serializeMessage();
    const expectedMessage = paymentTransaction(terms, prepared.recentBlockhash).serializeMessage();
    if (!message.equals(expectedMessage) || message.toString("base64") !== prepared.messageBase64
      || !transaction.verifySignatures() || !transaction.signature) {
      throw new Error("Payment message or signature differs");
    }
    const canonical = transaction.serialize();
    if (!canonical.equals(bytes)) throw new Error("Noncanonical transaction");
    return { signature: bs58.encode(transaction.signature), transactionBase64: canonical.toString("base64") };
  } catch {
    throw new ApiError(400, "INVALID_PAYMENT_TRANSACTION", "Transaction must exactly match the prepared payment and have a valid buyer signature");
  }
}

export async function submitPayment(env: AppEnv, terms: PaymentTerms, prepared: PreparedPayment, signedBase64: string): Promise<void> {
  const signed = validateSignedPayment(terms, prepared, signedBase64);
  const rpc = await paymentRpc(env, terms, prepared);
  const [, , blockHeight] = await Promise.all([
    validatorIdentity(rpc.endpoint, prepared.validator),
    requireZeroFee(rpc.connection, Transaction.from(Buffer.from(signed.transactionBase64, "base64"))),
    rpc.connection.getBlockHeight("confirmed"),
  ]);
  if (blockHeight > prepared.lastValidBlockHeight) {
    throw new ApiError(409, "PAYMENT_BLOCKHASH_EXPIRED", "Prepared payment blockhash expired; reconcile the original signature before creating another payment");
  }
  const signature = await rpc.connection.sendRawTransaction(Buffer.from(signed.transactionBase64, "base64"), {
    skipPreflight: true,
    preflightCommitment: "confirmed",
    maxRetries: 0,
  });
  if (signature !== signed.signature) {
    throw new ApiError(502, "PAYMENT_SIGNATURE_MISMATCH", "Payment RPC returned an unexpected transaction signature");
  }
}

export async function getPaymentStatus(env: AppEnv, terms: PaymentTerms, prepared: PreparedPayment, signature: string): Promise<{ state: "pending" | "paid" | "failed"; slot?: number; failure?: string }> {
  const rpc = await paymentRpc(env, terms, prepared);
  const [, result] = await Promise.all([
    validatorIdentity(rpc.endpoint, prepared.validator),
    rpc.connection.getSignatureStatuses([signature], { searchTransactionHistory: true }),
  ]);
  const status = result.value[0];
  if (!status || !["confirmed", "finalized"].includes(status.confirmationStatus ?? "")) return { state: "pending" };
  if (status.err) return { state: "failed", slot: status.slot, failure: JSON.stringify(status.err) };
  return { state: "paid", slot: status.slot };
}
