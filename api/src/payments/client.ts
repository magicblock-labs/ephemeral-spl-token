import { Buffer } from "buffer";
import { PublicKey, Transaction } from "@solana/web3.js";
import {
  decodeMppChallenge,
  decodePaymentJson,
  encodeMppCredential,
  encodePaymentJson,
  validateMppCredential,
  validateX402Payload,
} from "./protocols";
import type { PaymentTerms, PreparedPayment } from "./types";

const TOKEN_PROGRAM = new PublicKey("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const ASSOCIATED_TOKEN_PROGRAM = new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");
const MEMO_PROGRAM = new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

function object(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) throw new Error("Invalid payment requirements");
  return value as Record<string, unknown>;
}

function paymentTransaction(terms: PaymentTerms, prepared: PreparedPayment): Transaction {
  const transaction = Transaction.from(Buffer.from(prepared.transactionBase64, "base64"));
  const payer = new PublicKey(terms.payer);
  const mint = new PublicKey(terms.mint);
  const ata = (owner: string) => PublicKey.findProgramAddressSync([
    new PublicKey(owner).toBuffer(), TOKEN_PROGRAM.toBuffer(), mint.toBuffer(),
  ], ASSOCIATED_TOKEN_PROGRAM)[0];
  const [transfer, memo] = transaction.instructions;
  if (
    !transaction.feePayer?.equals(payer)
    || transaction.compileMessage().header.numRequiredSignatures !== 1
    || transaction.instructions.length !== 2
    || !transfer.programId.equals(TOKEN_PROGRAM)
    || transfer.data.length !== 9
    || transfer.data[0] !== 3
    || transfer.data.readBigUInt64LE(1).toString() !== terms.amount
    || transfer.keys.length !== 3
    || !transfer.keys[0].pubkey.equals(ata(terms.payer))
    || !transfer.keys[0].isWritable
    || transfer.keys[0].isSigner
    || !transfer.keys[1].pubkey.equals(ata(terms.recipient))
    || !transfer.keys[1].isWritable
    || transfer.keys[1].isSigner
    || !transfer.keys[2].pubkey.equals(payer)
    || !transfer.keys[2].isSigner
    || !memo.programId.equals(MEMO_PROGRAM)
    || memo.data.toString("utf8") !== `payment:${terms.id}`
    || memo.keys.length !== 0
  ) throw new Error("Transaction does not match the approved payment");
  return transaction;
}

export type PaymentCredentialOptions = {
  protocol: "x402" | "mpp";
  headers: Headers;
  /** Trusted checkout terms: approve these before calling the wallet. */
  payment: PaymentTerms;
  validator: string;
  /** Expected protection space from the trusted Payments API URL. */
  realm: string;
  signTransaction: (transaction: Transaction) => Promise<Transaction>;
};

/** Inspect, sign and encode a credential. No transaction is broadcast by this helper. */
export async function createPaymentCredential(options: PaymentCredentialOptions) {
  const { protocol, headers, payment, validator, signTransaction } = options;
  const challenge = protocol === "mpp"
    ? decodeMppChallenge(headers.get("WWW-Authenticate") ?? "")
    : undefined;
  const required = protocol === "x402"
    ? object(decodePaymentJson(headers.get("PAYMENT-REQUIRED") ?? ""))
    : undefined;
  if (required && (!Array.isArray(required.accepts) || required.accepts.length !== 1)) {
    throw new Error("Expected one MagicBlock payment requirement");
  }
  const accepted = required ? object((required.accepts as unknown[])[0]) : undefined;
  const details = object(challenge
    ? object(decodePaymentJson(challenge.request, true)).methodDetails
    : accepted?.extra);
  if (details.validator !== validator || typeof details.transaction !== "string"
    || typeof details.recentBlockhash !== "string" || !Number.isSafeInteger(details.lastValidBlockHeight)) {
    throw new Error("Invalid payment validator or transaction lifetime");
  }
  const prepared: PreparedPayment = {
    transactionBase64: details.transaction,
    messageBase64: "",
    recentBlockhash: details.recentBlockhash,
    lastValidBlockHeight: details.lastValidBlockHeight as number,
    validator,
    rpcEndpoint: "",
  };
  const encode = (transaction: string): string => challenge
    ? encodeMppCredential(challenge, payment.id, transaction)
    : encodePaymentJson({ x402Version: required?.x402Version, resource: required?.resource, accepted, payload: { paymentId: payment.id, transaction } });
  const unsignedCredential = encode(prepared.transactionBase64);
  if (challenge) {
    validateMppCredential(unsignedCredential, payment, prepared, options.realm);
  } else {
    validateX402Payload(unsignedCredential, payment, prepared);
  }
  const transaction = paymentTransaction(payment, prepared);
  if (transaction.recentBlockhash !== prepared.recentBlockhash) throw new Error("Payment blockhash mismatch");
  const message = transaction.serializeMessage();
  const signed = await signTransaction(transaction);
  if (!signed.serializeMessage().equals(message)) throw new Error("Wallet changed the payment transaction");
  const transactionBase64 = signed.serialize({ requireAllSignatures: true, verifySignatures: true }).toString("base64");
  return {
    paymentId: payment.id,
    headerName: protocol === "x402" ? "PAYMENT-SIGNATURE" : "Payment-Authorization",
    headerValue: encode(transactionBase64),
  };
}
