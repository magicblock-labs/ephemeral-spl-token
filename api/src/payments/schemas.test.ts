import { Keypair, PublicKey } from "@solana/web3.js";
import { describe, expect, it } from "vitest";
import { amountSchema, checkoutSchema, linkSchema, offerSchema, walletSchema } from "./schemas";

const payer = Keypair.generate().publicKey;

describe("payment input validation", () => {
  it.each(["abc", "", "-1", "0", "01", "1.5", "1e6", "18446744073709551616", "1".repeat(100)])("rejects amount %j without throwing from a refinement", (value) => {
    expect(amountSchema.safeParse(value).success).toBe(false);
  });

  it("accepts positive integer strings through the full u64 range", () => {
    expect(amountSchema.parse("1")).toBe("1");
    expect(amountSchema.parse("18446744073709551615")).toBe("18446744073709551615");
  });

  it.each(["", "bad", "0".repeat(44), "wallet\ud800"])("rejects invalid wallet %j without throwing from a refinement", (value) => {
    expect(walletSchema.safeParse(value).success).toBe(false);
  });

  it("requires an on-curve wallet and accepts a signing key", () => {
    const [pda] = PublicKey.findProgramAddressSync([Buffer.from("test")], payer);
    expect(walletSchema.safeParse(pda.toBase58()).success).toBe(false);
    expect(walletSchema.parse(payer.toBase58())).toBe(payer.toBase58());
  });

  it.each(["not a url", "https://", "ftp://merchant.test/item", "javascript:alert(1)"])("rejects resource %j without throwing from a refinement", (resource) => {
    expect(offerSchema.safeParse({ amount: "1000000", resource }).success).toBe(false);
  });

  it.each(["https://merchant.test/item", "http://localhost:8080/item"])("accepts HTTP resource %j", (resource) => {
    expect(offerSchema.parse({ amount: "1000000", resource }).resource).toBe(resource);
  });

  it.each(["externalReference", "description", "resource"])("rejects unpaired Unicode surrogates in %s before canonical serialization", (field) => {
    const input = {
      payer: payer.toBase58(), amount: "1000000", externalReference: "order_1",
      [field]: field === "resource" ? "https://merchant.test/\ud800" : "invalid\ud800",
    };
    expect(checkoutSchema.safeParse(input).success).toBe(false);
    const linkInput: Record<string, unknown> = { ...input };
    delete linkInput.payer;
    expect(linkSchema.safeParse(linkInput).success).toBe(false);
  });

  it("preserves valid Unicode including emoji in merchant metadata", () => {
    const input = { payer: payer.toBase58(), amount: "1000000", externalReference: "order_🎁", description: "Caffè 🎮", resource: "https://merchant.test/🎁" };
    expect(checkoutSchema.parse(input)).toMatchObject(input);
  });
});
