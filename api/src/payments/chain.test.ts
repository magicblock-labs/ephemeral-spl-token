import { DELEGATION_PROGRAM_ID, delegationRecordPdaFromDelegatedAccount, deriveEphemeralAta } from "@magicblock-labs/ephemeral-rollups-sdk";
import { ComputeBudgetProgram, Keypair, PublicKey, Transaction } from "@solana/web3.js";
import bs58 from "bs58";
import nacl from "tweetnacl";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { getEnv } from "../env";
import { TOKEN_PROGRAM_ID } from "../lib/solana";
import { getPaymentStatus, preparePayment, submitPayment, validateSignedPayment } from "./chain";
import type { PaymentTerms, PreparedPayment } from "./types";

const payer = Keypair.generate();
const recipient = Keypair.generate();
const validator = Keypair.generate().publicKey;
const otherValidator = Keypair.generate().publicKey;
const blockhash = Keypair.generate().publicKey.toBase58();
const terms: PaymentTerms = {
  id: "pay_test_order",
  merchantId: "merchant_test",
  payer: payer.publicKey.toBase58(),
  recipient: recipient.publicKey.toBase58(),
  amount: "1000000",
  mint: "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU",
  cluster: "devnet",
  externalReference: "order_test",
  resource: "https://merchant.test/credits",
  expiresAt: "2030-01-01T00:00:00.000Z",
};

type RpcCall = { url: URL; method: string; params: any[] };

function setupRpc() {
  const service = Keypair.generate();
  const env = getEnv({
    BASE_RPC_URL: "https://base.mainnet.test",
    EPHEMERAL_RPC_URL: "https://er.mainnet.test",
    BASE_DEVNET_RPC_URL: "https://base.devnet.test",
    EPHEMERAL_DEVNET_RPC_URL: "https://er.devnet.test",
    EPHEMERAL_DEVNET_TEE_RPC_URL: "https://tee.devnet.test",
    PAYMENTS_RPC_AUTH_SECRET_KEY: JSON.stringify([...service.secretKey]),
  });
  const senderRecord = delegationRecordPdaFromDelegatedAccount(deriveEphemeralAta(payer.publicKey, new PublicKey(terms.mint))[0]);
  const recipientRecord = delegationRecordPdaFromDelegatedAccount(deriveEphemeralAta(recipient.publicKey, new PublicKey(terms.mint))[0]);
  const state = {
    fee: 0 as number | null,
    identity: validator,
    recipientValidator: validator,
    senderDelegated: true,
    blockHeight: 100,
    status: { slot: 10, confirmations: 1, err: null, confirmationStatus: "confirmed" } as any,
    rpcFailure: false,
    sendSignature: undefined as string | undefined,
    logins: 0,
    calls: [] as RpcCall[],
  };
  vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
    const url = new URL(String(input));
    expect(init?.signal).toBeDefined();
    if (url.pathname === "/auth/challenge") {
      expect(url.searchParams.get("pubkey")).toBe(service.publicKey.toBase58());
      return Response.json({ challenge: "Authenticate payment service" });
    }
    const body = JSON.parse(String(init?.body));
    if (url.pathname === "/auth/login") {
      state.logins += 1;
      expect(body.pubkey).toBe(service.publicKey.toBase58());
      expect(nacl.sign.detached.verify(Buffer.from(body.challenge), bs58.decode(body.signature), service.publicKey.toBytes())).toBe(true);
      return Response.json({ token: "service-token", expiresAt: Date.now() + 3_600_000 });
    }
    state.calls.push({ url, method: body.method, params: body.params });
    if (state.rpcFailure) return new Response("Unavailable", { status: 503 });
    if (url.hostname === "tee.devnet.test") expect(url.searchParams.get("token")).toBe("service-token");
    let result: unknown;
    switch (body.method) {
      case "getIdentity":
        result = { identity: state.identity.toBase58() };
        break;
      case "getLatestBlockhash":
        result = { context: { slot: 10 }, value: { blockhash, lastValidBlockHeight: 200 } };
        break;
      case "getAccountInfo": {
        expect(url.hostname).toBe("base.devnet.test");
        expect([senderRecord.toBase58(), recipientRecord.toBase58()]).toContain(body.params[0]);
        const isSender = body.params[0] === senderRecord.toBase58();
        const data = Buffer.alloc(40);
        (isSender ? validator : state.recipientValidator).toBuffer().copy(data, 8);
        result = {
          context: { slot: 10 },
          value: isSender && !state.senderDelegated
            ? null
            : {
                data: [data.toString("base64"), "base64"],
                owner: DELEGATION_PROGRAM_ID.toBase58(),
                lamports: 1,
                executable: false,
                rentEpoch: 0,
              },
        };
        break;
      }
      case "getFeeForMessage":
        result = { context: { slot: 10 }, value: state.fee };
        break;
      case "getBlockHeight":
        result = state.blockHeight;
        break;
      case "sendTransaction": {
        const transaction = Transaction.from(Buffer.from(body.params[0], "base64"));
        expect(transaction.verifySignatures()).toBe(true);
        expect(body.params[1].skipPreflight).toBe(true);
        result = state.sendSignature ?? bs58.encode(transaction.signature!);
        break;
      }
      case "getSignatureStatuses":
        expect(body.params[1]).toEqual({ searchTransactionHistory: true });
        result = { context: { slot: 10 }, value: [state.status] };
        break;
      default:
        throw new Error(`Unexpected RPC method: ${body.method}`);
    }
    return Response.json({ jsonrpc: "2.0", id: body.id, result });
  });
  return { env, state };
}

function sign(prepared: PreparedPayment, mutate?: (transaction: Transaction) => void) {
  const transaction = Transaction.from(Buffer.from(prepared.transactionBase64, "base64"));
  mutate?.(transaction);
  transaction.sign(payer);
  return transaction.serialize().toString("base64");
}

describe("payment transaction preparation and verification", () => {
  let rpc: ReturnType<typeof setupRpc>;
  beforeEach(() => {
    rpc = setupRpc();
  });
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("prepares only an exact delegated USDC transfer and payment memo, with the buyer as sole signer", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    const transaction = Transaction.from(Buffer.from(prepared.transactionBase64, "base64"));
    expect(transaction.feePayer?.equals(payer.publicKey)).toBe(true);
    expect(transaction.compileMessage().header.numRequiredSignatures).toBe(1);
    expect(transaction.instructions).toHaveLength(2);
    expect(transaction.instructions[0].programId.equals(TOKEN_PROGRAM_ID)).toBe(true);
    expect(transaction.instructions[0].data[0]).toBe(3);
    expect(transaction.instructions[0].data.readBigUInt64LE(1)).toBe(1_000_000n);
    expect(transaction.instructions[1].data.toString()).toBe(`payment:${terms.id}`);
    expect(transaction.instructions[1].keys).toEqual([]);
    expect(prepared.validator).toBe(validator.toBase58());
    const signed = sign(prepared);
    expect(validateSignedPayment(terms, prepared, signed)).toEqual({
      signature: bs58.encode(Transaction.from(Buffer.from(signed, "base64")).signature!),
      transactionBase64: signed,
    });
  });

  it.each([1, 5000, null])("rejects a nonzero or unavailable network fee (%s)", async (fee) => {
    rpc.state.fee = fee;
    await expect(preparePayment(rpc.env, terms)).rejects.toMatchObject({ code: "PAYMENT_ZERO_FEE_REQUIRED" });
  });

  it("rejects a recipient delegated to another validator", async () => {
    rpc.state.recipientValidator = otherValidator;
    await expect(preparePayment(rpc.env, terms)).rejects.toMatchObject({ code: "PAYMENT_ACCOUNTS_NOT_DELEGATED" });
  });

  it("rejects a sender without an active delegation record", async () => {
    rpc.state.senderDelegated = false;
    await expect(preparePayment(rpc.env, terms)).rejects.toMatchObject({ code: "PAYMENT_ACCOUNTS_NOT_DELEGATED" });
  });

  it("rejects a different mint or a self-payment before RPC calls", async () => {
    await expect(preparePayment(rpc.env, { ...terms, mint: Keypair.generate().publicKey.toBase58() })).rejects.toMatchObject({ code: "UNSUPPORTED_PAYMENT_MINT" });
    await expect(preparePayment(rpc.env, { ...terms, recipient: terms.payer })).rejects.toMatchObject({ code: "INVALID_PAYMENT_ACCOUNTS" });
    expect(rpc.state.calls).toEqual([]);
  });

  it.each([
    ["amount", (tx: Transaction) => { tx.instructions[0].data.writeBigUInt64LE(1n, 1); }],
    ["recipient", (tx: Transaction) => { tx.instructions[0].keys[1].pubkey = otherValidator; }],
    ["blockhash", (tx: Transaction) => { tx.recentBlockhash = otherValidator.toBase58(); }],
    ["memo", (tx: Transaction) => { tx.instructions[1].data = Buffer.from("payment:another_order"); }],
    ["extra fee instruction", (tx: Transaction) => { tx.add(ComputeBudgetProgram.setComputeUnitPrice({ microLamports: 1000 })); }],
  ] as const)("rejects a valid signature over a changed %s", async (_name, mutate) => {
    const prepared = await preparePayment(rpc.env, terms);
    expect(() => validateSignedPayment(terms, prepared, sign(prepared, mutate))).toThrow("exactly match");
  });

  it("rejects missing or forged signatures and trailing wire bytes", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    expect(() => validateSignedPayment(terms, prepared, prepared.transactionBase64)).toThrow("valid buyer signature");
    const signed = Buffer.from(sign(prepared), "base64");
    const forged = Buffer.from(signed);
    forged[1] ^= 1;
    expect(() => validateSignedPayment(terms, prepared, forged.toString("base64"))).toThrow("valid buyer signature");
    expect(() => validateSignedPayment(terms, prepared, Buffer.concat([signed, Buffer.from([0])]).toString("base64"))).toThrow("exactly match");
  });

  it("rejects a different fee payer even when every required signature is valid", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    const transaction = Transaction.from(Buffer.from(prepared.transactionBase64, "base64"));
    transaction.feePayer = recipient.publicKey;
    transaction.sign(recipient, payer);
    expect(transaction.verifySignatures()).toBe(true);
    expect(() => validateSignedPayment(terms, prepared, transaction.serialize().toString("base64"))).toThrow("exactly match");
  });

  it("does not trust a stored message that no longer matches the order terms", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    expect(() => validateSignedPayment({ ...terms, amount: "2000000" }, prepared, sign(prepared))).toThrow("exactly match");
  });

  it("rechecks fees and validator identity before broadcasting the exact signed bytes", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    const signed = sign(prepared);
    rpc.state.fee = 1;
    await expect(submitPayment(rpc.env, terms, prepared, signed)).rejects.toMatchObject({ code: "PAYMENT_ZERO_FEE_REQUIRED" });
    rpc.state.fee = 0;
    rpc.state.identity = otherValidator;
    await expect(submitPayment(rpc.env, terms, prepared, signed)).rejects.toMatchObject({ code: "PAYMENT_VALIDATOR_CHANGED" });
    expect(rpc.state.calls.some(call => call.method === "sendTransaction")).toBe(false);
    rpc.state.identity = validator;
    await submitPayment(rpc.env, terms, prepared, signed);
    expect(rpc.state.calls.find(call => call.method === "sendTransaction")?.params[0]).toBe(signed);
  });

  it("rejects RPC endpoint drift before making a request", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    rpc.state.calls.length = 0;
    await expect(getPaymentStatus(rpc.env, terms, { ...prepared, rpcEndpoint: "https://attacker.test" }, "signature")).rejects.toMatchObject({ code: "PAYMENT_RPC_CHANGED" });
    expect(rpc.state.calls).toEqual([]);
  });

  it("does not trust a successful signature status after the configured validator identity changes", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    const { signature } = validateSignedPayment(terms, prepared, sign(prepared));
    rpc.state.calls.length = 0;
    rpc.state.identity = otherValidator;
    await expect(getPaymentStatus(rpc.env, terms, prepared, signature)).rejects.toMatchObject({ code: "PAYMENT_VALIDATOR_CHANGED" });
    expect(rpc.state.calls.map(call => call.method).sort()).toEqual(["getIdentity", "getSignatureStatuses"].sort());
  });

  it("stops broadcasting after the prepared blockhash expires while still reconciling its original signature", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    const signed = sign(prepared);
    const { signature } = validateSignedPayment(terms, prepared, signed);
    rpc.state.blockHeight = prepared.lastValidBlockHeight + 1;
    await expect(submitPayment(rpc.env, terms, prepared, signed)).rejects.toMatchObject({ code: "PAYMENT_BLOCKHASH_EXPIRED" });
    expect(rpc.state.calls.some(call => call.method === "sendTransaction")).toBe(false);
    expect(await getPaymentStatus(rpc.env, terms, prepared, signature)).toEqual({ state: "paid", slot: 10 });
  });

  it("tracks private confirmation with only cached service authentication and no buyer account access", async () => {
    const privateTerms: PaymentTerms = { ...terms, cluster: "devnet-private" };
    const prepared = await preparePayment(rpc.env, privateTerms);
    const signed = validateSignedPayment(privateTerms, prepared, sign(prepared));
    expect(prepared.rpcEndpoint).toBe(rpc.env.EPHEMERAL_DEVNET_TEE_RPC_URL);
    expect(JSON.stringify(prepared)).not.toContain("service-token");
    rpc.state.calls.length = 0;
    await submitPayment(rpc.env, privateTerms, prepared, signed.transactionBase64);
    expect(await getPaymentStatus(rpc.env, privateTerms, prepared, signed.signature)).toEqual({ state: "paid", slot: 10 });
    expect(await getPaymentStatus(rpc.env, privateTerms, prepared, signed.signature)).toEqual({ state: "paid", slot: 10 });
    expect(rpc.state.logins).toBe(1);
    expect(rpc.state.calls.every(call => call.url.searchParams.get("token") === "service-token")).toBe(true);
    expect(rpc.state.calls.some(call => call.method === "getAccountInfo" || call.method === "getTransaction")).toBe(false);
  });

  it("requires a separate service identity for private payments", async () => {
    await expect(preparePayment({ ...rpc.env, PAYMENTS_RPC_AUTH_SECRET_KEY: undefined }, { ...terms, cluster: "devnet-private" })).rejects.toMatchObject({ code: "PAYMENT_RPC_AUTH_UNAVAILABLE" });
  });

  it("distinguishes confirmation, execution failure, absent status and upstream failure", async () => {
    const prepared = await preparePayment(rpc.env, terms);
    const { signature } = validateSignedPayment(terms, prepared, sign(prepared));
    rpc.state.status.confirmationStatus = "processed";
    expect(await getPaymentStatus(rpc.env, terms, prepared, signature)).toEqual({ state: "pending" });
    rpc.state.status.confirmationStatus = "finalized";
    expect(await getPaymentStatus(rpc.env, terms, prepared, signature)).toEqual({ state: "paid", slot: 10 });
    rpc.state.status.err = { InstructionError: [0, "InsufficientFunds"] };
    expect(await getPaymentStatus(rpc.env, terms, prepared, signature)).toEqual({ state: "failed", slot: 10, failure: JSON.stringify(rpc.state.status.err) });
    rpc.state.status.confirmationStatus = "processed";
    expect(await getPaymentStatus(rpc.env, terms, prepared, signature)).toEqual({ state: "pending" });
    rpc.state.status = null;
    expect(await getPaymentStatus(rpc.env, terms, prepared, signature)).toEqual({ state: "pending" });
    rpc.state.rpcFailure = true;
    await expect(getPaymentStatus(rpc.env, terms, prepared, signature)).rejects.toMatchObject({ code: "PAYMENT_RPC_ERROR" });
  });
});
