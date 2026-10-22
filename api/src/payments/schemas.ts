import { z } from "@hono/zod-openapi";
import { PublicKey } from "@solana/web3.js";
import { publicKeySchema } from "../schema";

const textSchema = z.string().refine(value => !/[\uD800-\uDFFF]/u.test(value), "Text must contain valid Unicode");
export const walletSchema = publicKeySchema.pipe(z.string().refine(value => PublicKey.isOnCurve(new PublicKey(value).toBytes()), "An on-curve wallet is required"));
export const idSchema = z.string().regex(/^[a-f0-9]{64}$/);
export const clusterSchema = z.enum(["mainnet", "devnet", "mainnet-private", "devnet-private"]);
export const amountSchema = z.string().regex(/^[1-9][0-9]{0,19}$/).pipe(z.string().refine(value => BigInt(value) <= 18_446_744_073_709_551_615n, "Amount exceeds u64"));
export const challengeSchema = z.object({ wallet: walletSchema, purpose: z.enum(["register", "rotate-key"]).default("register") }).strict();
export const registrationSchema = z.object({ wallet: walletSchema, challengeId: z.string().uuid(), signature: z.string().length(88) }).strict();

export const offerSchema = z.object({
  amount: amountSchema,
  cluster: clusterSchema.default("mainnet-private").describe("Defaults to the configured mainnet TEE for public or private accounts. Account permissions determine privacy; public validators do not preserve it."),
  description: textSchema.min(1).max(256).optional(),
  resource: textSchema.url().max(2048).pipe(z.string().refine(value => ["https:", "http:"].includes(new URL(value).protocol))).optional(),
  requestHash: z.string().regex(/^[a-f0-9]{64}$/).optional(),
  expiresInSeconds: z.number().int().min(60).max(86_400).default(3_600),
}).strict();
export const checkoutSchema = offerSchema.extend({
  payer: walletSchema,
  externalReference: textSchema.min(1).max(128),
}).strict();
export const linkSchema = offerSchema.extend({ externalReference: textSchema.min(1).max(128) }).strict();
export const linkCheckoutSchema = z.object({ payer: walletSchema }).strict();
export const settleSchema = z.object({ transactionBase64: z.string().min(1).max(1644) }).strict();
export const facilitatorSchema = z.object({ x402Version: z.literal(2), paymentPayload: z.unknown(), paymentRequirements: z.unknown() }).strict();

export type CheckoutInput = z.infer<typeof checkoutSchema>;
export type LinkInput = z.infer<typeof linkSchema>;
export type PaymentLink = LinkInput & { id: string; merchantId: string; recipient: string; createdAt: string };
