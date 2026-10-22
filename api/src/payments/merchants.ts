import { PublicKey } from "@solana/web3.js";
import nacl from "tweetnacl";
import type { AppBindings } from "../env";
import { ApiError, errorBody } from "../lib/errors";
import { digest, paymentConfig } from "./config";
import type { PaymentLink } from "./schemas";
import type { MerchantRecord } from "./types";

type Challenge = { id: string; wallet: string; purpose: "register" | "rotate-key"; message: string; expiresAt: number };

/** One registry actor per merchant wallet; payment processing is sharded separately per order. */
export class PaymentMerchants {
  private queue: Promise<unknown> = Promise.resolve();

  constructor(private state: DurableObjectState, private env: AppBindings) {}

  async fetch(request: Request): Promise<Response> {
    const operation = this.queue.then(async () => {
      try {
        const input = await request.json() as Record<string, string> & { link?: PaymentLink };
        return Response.json(await this.handle(new URL(request.url).pathname, input));
      } catch (error) {
        if (error instanceof ApiError) return Response.json(errorBody(error.code, error.message), { status: error.status });
        return Response.json(errorBody("MERCHANT_ERROR", "Merchant operation failed"), { status: 500 });
      }
    });
    this.queue = operation.catch(() => undefined);
    return operation;
  }

  private async handle(path: string, input: Record<string, string> & { link?: PaymentLink }) {
    if (path === "/challenge") {
      const { origin } = paymentConfig(this.env);
      const purpose = input.purpose === "rotate-key" ? "rotate-key" : "register";
      const key = `challenge:${purpose}`;
      const existing = await this.state.storage.get<Challenge>(key);
      if (existing && existing.expiresAt > Date.now()) return existing;
      const id = crypto.randomUUID();
      const expiresAt = Date.now() + 5 * 60_000;
      const challenge: Challenge = {
        id, wallet: input.wallet, purpose, expiresAt,
        message: `${origin} requests merchant ${purpose}\nWallet: ${input.wallet}\nNonce: ${id}\nExpires: ${new Date(expiresAt).toISOString()}`,
      };
      await this.state.storage.put(key, challenge);
      return challenge;
    }
    if (path === "/register" || path === "/rotate-key") {
      const purpose = path === "/register" ? "register" : "rotate-key";
      const key = `challenge:${purpose}`;
      const challenge = await this.state.storage.get<Challenge>(key);
      if (!challenge || challenge.id !== input.challengeId || challenge.wallet !== input.wallet || challenge.expiresAt <= Date.now()) {
        throw new ApiError(401, "INVALID_MERCHANT_CHALLENGE", "Challenge is missing, expired, or already consumed");
      }
      const signature = Buffer.from(input.signature, "base64");
      if (signature.length !== 64 || signature.toString("base64") !== input.signature
        || !nacl.sign.detached.verify(new TextEncoder().encode(challenge.message), signature, new PublicKey(input.wallet).toBytes())) {
        throw new ApiError(401, "INVALID_MERCHANT_SIGNATURE", "Wallet signature is invalid");
      }
      const existing = await this.state.storage.get<MerchantRecord>("merchant");
      if (existing && purpose === "register") throw new ApiError(409, "MERCHANT_EXISTS", "Merchant already exists; use a rotate-key challenge to recover access");
      if (!existing && purpose === "rotate-key") throw new ApiError(404, "MERCHANT_NOT_FOUND", "Merchant is not registered");
      const apiKey = `mb_${Buffer.from(crypto.getRandomValues(new Uint8Array(32))).toString("base64url")}`;
      const merchant: MerchantRecord = { id: input.wallet, wallet: input.wallet, apiKeyHash: digest(apiKey), createdAt: existing?.createdAt ?? new Date().toISOString() };
      await this.state.storage.transaction(async (storage) => {
        await storage.put("merchant", merchant);
        await storage.delete(key);
      });
      return { merchantId: merchant.id, wallet: merchant.wallet, apiKey };
    }
    if (path === "/authenticate") {
      const merchant = await this.state.storage.get<MerchantRecord>("merchant");
      if (!merchant || !input.apiKey || !nacl.verify(Buffer.from(merchant.apiKeyHash, "hex"), Buffer.from(digest(input.apiKey), "hex"))) {
        throw new ApiError(401, "MERCHANT_UNAUTHORIZED", "Valid merchant API credentials required");
      }
      return { merchantId: merchant.id, wallet: merchant.wallet };
    }
    if (path === "/create-link") {
      const link = input.link!;
      const existing = await this.state.storage.get<{ link: PaymentLink; creationHash: string }>(`link:${link.id}`);
      if (existing) {
        if (existing.creationHash !== input.creationHash) throw new ApiError(409, "PAYMENT_LINK_CONFLICT", "Link reference already has different terms");
        return existing.link;
      }
      await this.state.storage.put(`link:${link.id}`, { link, creationHash: input.creationHash });
      return link;
    }
    if (path === "/get-link") {
      const stored = await this.state.storage.get<{ link: PaymentLink }>(`link:${input.id}`);
      if (!stored) throw new ApiError(404, "PAYMENT_LINK_NOT_FOUND", "Payment link not found");
      return stored.link;
    }
    throw new ApiError(404, "NOT_FOUND", "Unknown merchant operation");
  }
}
