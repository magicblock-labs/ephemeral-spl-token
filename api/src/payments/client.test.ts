import { Buffer } from "buffer";
import { Keypair, PublicKey, Transaction, TransactionInstruction } from "@solana/web3.js";
import { describe, expect, it, vi } from "vitest";
import { createPaymentCredential } from "./client";
import {
  createMppChallenge,
  decodeMppCredential,
  decodeX402Payload,
  encodeMppChallenge,
  encodeX402PaymentRequired,
} from "./protocols";
import type { PaymentTerms, PreparedPayment } from "./types";

const tokenProgram = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const associatedTokenProgram = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

function fixture(change?: (transaction: Transaction) => void) {
  const buyer = Keypair.generate();
  const terms: PaymentTerms = {
    id: "pay_test", merchantId: "merchant", payer: buyer.publicKey.toBase58(),
    recipient: Keypair.generate().publicKey.toBase58(), mint: Keypair.generate().publicKey.toBase58(),
    amount: "1000000", cluster: "devnet-private", externalReference: "order", resource: "https://merchant.test/credits",
    expiresAt: new Date(Date.now() + 60_000).toISOString(),
  };
  const ata = (owner: string) => PublicKey.findProgramAddressSync([
    new PublicKey(owner).toBuffer(), tokenProgram.toBuffer(), new PublicKey(terms.mint).toBuffer(),
  ], associatedTokenProgram)[0];
  const data = Buffer.alloc(9);
  data[0] = 3;
  data.writeBigUInt64LE(BigInt(terms.amount), 1);
  const transaction = new Transaction({ feePayer: buyer.publicKey, recentBlockhash: Keypair.generate().publicKey.toBase58() }).add(
    new TransactionInstruction({
      programId: tokenProgram,
      keys: [
        { pubkey: ata(terms.payer), isSigner: false, isWritable: true },
        { pubkey: ata(terms.recipient), isSigner: false, isWritable: true },
        { pubkey: buyer.publicKey, isSigner: true, isWritable: false },
      ],
      data,
    }),
    new TransactionInstruction({ programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"), keys: [], data: Buffer.from(`payment:${terms.id}`) }),
  );
  change?.(transaction);
  const prepared: PreparedPayment = {
    transactionBase64: transaction.serialize({ requireAllSignatures: false, verifySignatures: false }).toString("base64"),
    messageBase64: transaction.serializeMessage().toString("base64"),
    validator: "validator", recentBlockhash: transaction.recentBlockhash!, lastValidBlockHeight: 100, rpcEndpoint: "https://secret.rpc.test",
  };
  const realm = "payments.test";
  const headers = new Headers({
    "PAYMENT-REQUIRED": encodeX402PaymentRequired(terms, prepared),
    "WWW-Authenticate": encodeMppChallenge(createMppChallenge(terms, prepared, realm)),
  });
  const signTransaction = vi.fn(async (unsigned: Transaction) => {
    unsigned.sign(buyer);
    return unsigned;
  });
  return { buyer, payment: terms, validator: prepared.validator, realm, headers, signTransaction };
}

describe("payment wallet helper", () => {
  it.each(["x402", "mpp"] as const)("inspects and signs the prepared %s payment without broadcasting", async (protocol) => {
    const options = fixture();
    const credential = await createPaymentCredential({ ...options, protocol });
    expect(options.signTransaction).toHaveBeenCalledOnce();
    const decoded = protocol === "x402" ? decodeX402Payload(credential.headerValue) : decodeMppCredential(credential.headerValue);
    expect(decoded.paymentId).toBe(options.payment.id);
    expect(Transaction.from(Buffer.from(decoded.transactionBase64, "base64")).verifySignatures()).toBe(true);
    expect(credential.headerName).toBe(protocol === "x402" ? "PAYMENT-SIGNATURE" : "Payment-Authorization");
  });

  it.each(["recipient", "mint", "amount", "resource", "payer"])("rejects unapproved %s before opening the wallet", async (field) => {
    const options = fixture();
    Object.assign(options.payment, { [field]: "different" });
    await expect(createPaymentCredential({ ...options, protocol: "x402" })).rejects.toThrow();
    expect(options.signTransaction).not.toHaveBeenCalled();
  });

  it("rejects wrong validator or MPP realm before opening the wallet", async () => {
    const options = fixture();
    await expect(createPaymentCredential({ ...options, protocol: "x402", validator: "other" })).rejects.toThrow("validator");
    await expect(createPaymentCredential({ ...options, protocol: "mpp", realm: "other.test" })).rejects.toThrow("challenge");
    expect(options.signTransaction).not.toHaveBeenCalled();
  });

  it.each(["x402", "mpp"] as const)("rejects an expired %s checkout before opening the wallet", async (protocol) => {
    const options = fixture();
    const realNow = Date.now();
    const clock = vi.spyOn(Date, "now").mockReturnValue(realNow + 120_000);
    try {
      await expect(createPaymentCredential({ ...options, protocol })).rejects.toThrow("expired");
      expect(options.signTransaction).not.toHaveBeenCalled();
    } finally {
      clock.mockRestore();
    }
  });

  it("inspects transaction contents independently of advertised payment requirements", async () => {
    const options = fixture((transaction) => {
      transaction.instructions[0].data.writeBigUInt64LE(5_000_000n, 1);
    });
    await expect(createPaymentCredential({ ...options, protocol: "x402" })).rejects.toThrow("approved payment");
    expect(options.signTransaction).not.toHaveBeenCalled();
  });

  it("rejects additional instructions and wrong payment memo", async () => {
    for (const options of [
      fixture(transaction => transaction.add(transaction.instructions[0])),
      fixture((transaction) => { transaction.instructions[1].data = Buffer.from("payment:other"); }),
    ]) {
      await expect(createPaymentCredential({ ...options, protocol: "mpp" })).rejects.toThrow("approved payment");
      expect(options.signTransaction).not.toHaveBeenCalled();
    }
  });

  it("rejects wallet message changes and missing signatures", async () => {
    const options = fixture();
    await expect(createPaymentCredential({
      ...options, protocol: "x402", signTransaction: async transaction => transaction,
    })).rejects.toThrow("Signature verification failed");
    await expect(createPaymentCredential({
      ...options,
      protocol: "x402",
      signTransaction: async (transaction) => {
        transaction.recentBlockhash = Keypair.generate().publicKey.toBase58();
        transaction.sign(options.buyer);
        return transaction;
      },
    })).rejects.toThrow("Wallet changed");
  });
});
