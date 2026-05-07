import { afterEach, describe, expect, it, vi } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import {
  DELEGATION_PROGRAM_ID,
  deriveEphemeralAta,
  deriveHydraCrankPda,
  deriveRentPda,
  deriveStashAta,
  deriveStashPda,
  deriveTransferQueue,
  deriveVault,
  deriveVaultAta,
  EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
  HYDRA_PROGRAM_ID,
  magicFeeVaultPdaFromValidator,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import {
  AddressLookupTableAccount,
  AddressLookupTableProgram,
  AccountInfo,
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemInstruction,
  SystemProgram,
  Transaction,
  TransactionInstruction,
  TransactionMessage,
  VersionedTransaction,
} from "@solana/web3.js";

import app from "./app";
import { TOKEN_PROGRAM_ID } from "./lib/solana";
import { MOCK_AUTH_TOKEN } from "./lib/auth";

const env = {
  BASE_RPC_URL: "https://base.rpc.test",
  EPHEMERAL_RPC_URL: "https://ephemeral.rpc.test",
  BASE_DEVNET_RPC_URL: "https://base.devnet.rpc.test",
  EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.devnet.rpc.test",
  CLUSTER: "mainnet" as const,
  CORS_ORIGIN: "*",
};

const DEVNET_USDC_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const owner = Keypair.generate().publicKey.toBase58();
const destination = Keypair.generate().publicKey.toBase58();
const resolvedValidator = Keypair.generate().publicKey.toBase58();

function deriveAssociatedTokenAddress(mint: string, owner: string) {
  const [ata] = PublicKey.findProgramAddressSync(
    [
      new PublicKey(owner).toBuffer(),
      TOKEN_PROGRAM_ID.toBuffer(),
      new PublicKey(mint).toBuffer(),
    ],
    new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
  );

  return ata.toBase58();
}

function createMcpFetch() {
  return (input: RequestInfo | URL, init?: RequestInit) => {
    const request = input instanceof Request
      ? input
      : new Request(input instanceof URL ? input : String(input), init);

    return Promise.resolve(app.fetch(request, env));
  };
}

function createTokenAccountData(amount: bigint) {
  const data = Buffer.alloc(165);
  data.writeBigUInt64LE(amount, 64);
  return data;
}

function createAccountInfo(amount: bigint): AccountInfo<Buffer> {
  return {
    data: createTokenAccountData(amount),
    executable: false,
    lamports: 0,
    owner: TOKEN_PROGRAM_ID,
    rentEpoch: 0,
  };
}

function createQueueAccountInfo(owner: PublicKey): AccountInfo<Buffer> {
  return {
    data: Buffer.alloc(64),
    executable: false,
    lamports: 1,
    owner,
    rentEpoch: 0,
  };
}

function createIdentityResponse(identity: string) {
  return new Response(JSON.stringify({
    result: {
      identity,
    },
  }), {
    status: 200,
    headers: {
      "content-type": "application/json",
    },
  });
}

function createLookupTableResponse(value: AddressLookupTableAccount | null): Awaited<ReturnType<Connection["getAddressLookupTable"]>> {
  return {
    context: {
      slot: 0,
    },
    value,
  };
}

function createLookupTableAccount(addresses: PublicKey[], key = Keypair.generate().publicKey) {
  return new AddressLookupTableAccount({
    key,
    state: {
      deactivationSlot: 18446744073709551615n,
      lastExtendedSlot: 0,
      lastExtendedSlotStartIndex: 0,
      authority: Keypair.generate().publicKey,
      addresses,
    },
  });
}

function createLookupTableAccountInfo(): AccountInfo<Buffer> {
  return {
    data: Buffer.alloc(0),
    executable: false,
    lamports: 0,
    owner: AddressLookupTableProgram.programId,
    rentEpoch: 0,
  };
}

type TestExecutionContext = ExecutionContext & {
  drain: () => Promise<void>;
};

function createExecutionContext(): TestExecutionContext {
  const tasks: Promise<unknown>[] = [];

  return {
    props: undefined,
    waitUntil(promise: Promise<unknown>) {
      tasks.push(promise);
    },
    passThroughOnException() { },
    async drain() {
      await Promise.all(tasks);
    },
  };
}

describe("app", () => {
  afterEach(() => {
    vi.restoreAllMocks();
  });

  it("redirects / to /reference", async () => {
    const response = await app.request("/", {}, env);

    expect(response.status).toBe(302);
    expect(response.headers.get("location")).toBe("/reference");
  });

  it("serves the OpenAPI document", async () => {
    const response = await app.request("/doc", {}, env);

    expect(response.status).toBe(200);

    const json = await response.json() as any;
    expect(json.paths["/v1/spl/deposit"]).toBeDefined();
    expect(json.paths["/mcp"]?.post).toBeDefined();
    expect(json.paths["/mcp"]?.get).toBeUndefined();
    expect(json.paths["/.well-known/mcp.json"]).toBeUndefined();
    expect(json.paths["/mcp"]?.post?.requestBody?.content?.["application/json"]?.schema).toBeDefined();
    expect(json.paths["/v1/spl/private-balance"]).toBeDefined();
    expect(json.paths["/v1/spl/challenge"]).toBeDefined();
    expect(json.paths["/v1/spl/login"]).toBeDefined();
    expect(json.paths["/v1/spl/is-mint-initialized"]).toBeDefined();
    expect(json.paths["/v1/spl/initialize-mint"]).toBeDefined();
    expect(json.paths["/v1/swap/quote"]).toBeDefined();
    expect(json.paths["/v1/swap/swap"]).toBeDefined();
    expect(json.paths["/v1/swap/swap-instructions"]).toBeUndefined();
    expect(json.paths["/v1/swap/program-id-to-label"]).toBeUndefined();
    expect(json.paths["/v1/swap/quote"]?.get?.parameters).toEqual(expect.arrayContaining([
      expect.objectContaining({ name: "inputMint", required: true }),
      expect.objectContaining({ name: "outputMint", required: true }),
      expect.objectContaining({ name: "amount", required: true }),
      expect.objectContaining({ name: "slippageBps" }),
      expect.objectContaining({ name: "swapMode" }),
    ]));
    expect(json.paths["/v1/swap/swap"]?.post?.requestBody?.content?.["application/json"]?.schema).toBeDefined();
    expect(json.paths["/v1/swap/quote"]?.get?.responses?.["200"]?.content?.["application/json"]?.example).toMatchObject({
      inputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      outputMint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
      inAmount: "1000000",
      outAmount: "999519",
    });
    expect(json.paths["/v1/swap/swap"]?.post?.requestBody?.content?.["application/json"]?.examples?.public?.value).toMatchObject({
      userPublicKey: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
      quoteResponse: {
        inputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        outputMint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
        inAmount: "1000000",
        outAmount: "999519",
      },
    });

    // Private-swap visibility + required params are documented on the named
    // SwapRequest component (the request body itself uses a $ref).
    const swapRequestSchema = (json.components?.schemas as Record<string, any>)?.SwapRequest;
    expect(swapRequestSchema?.properties?.visibility).toBeDefined();
    expect(swapRequestSchema?.properties?.destination).toBeDefined();
    expect(swapRequestSchema?.properties?.minDelayMs).toBeDefined();
    expect(swapRequestSchema?.properties?.maxDelayMs).toBeDefined();
    expect(swapRequestSchema?.properties?.split).toBeDefined();
    expect(swapRequestSchema?.properties?.clientRefId).toBeDefined();
    expect(swapRequestSchema?.properties?.validator).toBeDefined();

    // Both request examples (public + private) are surfaced.
    const swapRequestExamples = json.paths["/v1/swap/swap"]?.post?.requestBody?.content?.["application/json"]?.examples;
    expect(swapRequestExamples?.public?.value).toMatchObject({
      userPublicKey: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
    });
    expect(swapRequestExamples?.private?.value).toMatchObject({
      visibility: "private",
      destination: expect.any(String),
      minDelayMs: "0",
      maxDelayMs: "0",
      split: 1,
    });

    // The `privateTransfer` diagnostic is present on the response schema +
    // visible in the private example.
    const swapResponseSchema = (json.components?.schemas as Record<string, any>)?.SwapResponse;
    expect(swapResponseSchema?.properties?.privateTransfer).toBeDefined();
    const swapResponseExamples = json.paths["/v1/swap/swap"]?.post?.responses?.["200"]?.content?.["application/json"]?.examples;
    expect(swapResponseExamples?.private?.value?.privateTransfer).toMatchObject({
      stashAta: expect.any(String),
      hydraCrankPda: expect.any(String),
      shuttleId: expect.any(Number),
    });
    expect(json.paths["/v1/spl/deposit"]?.post?.responses?.["200"]?.content?.["application/json"]?.example).toMatchObject({
      kind: "deposit",
      instructionCount: 3,
    });
    const transferRequestSchema = (json.components?.schemas as Record<string, any>)?.TransferRequest;
    expect(transferRequestSchema?.properties?.gasless).toMatchObject({
      type: "boolean",
      example: true,
    });
    expect(transferRequestSchema?.example).toMatchObject({
      amount: 5000000,
      gasless: true,
    });
    expect(transferRequestSchema?.example).not.toHaveProperty("initIfMissing");
    expect(transferRequestSchema?.example).not.toHaveProperty("initAtasIfMissing");
    expect(transferRequestSchema?.example).not.toHaveProperty("initVaultIfMissing");
    expect(transferRequestSchema?.example).not.toHaveProperty("split");
    expect(json.paths["/v1/spl/withdraw"]?.post?.responses?.["200"]?.content?.["application/json"]?.example).toMatchObject({
      kind: "withdraw",
      instructionCount: 2,
    });
  });

  it("proxies Metis quote requests to the configured upstream endpoint", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      expect(String(input)).toBe(
        `${metisEnv.METIS_SWAP_API_URL}/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&amount=1000000&slippageBps=50`,
      );
      expect(init?.method).toBe("GET");
      return new Response(JSON.stringify({
        inputMint: "So11111111111111111111111111111111111111112",
        outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
        outAmount: "999000",
      }), {
        status: 200,
        headers: {
          "content-type": "application/json",
        },
      });
    });

    const response = await app.request(
      "/v1/swap/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&amount=1000000&slippageBps=50",
      {},
      metisEnv,
    );

    expect(response.status).toBe(200);

    const json = await response.json() as {
      outAmount: string;
    };

    expect(json.outAmount).toBe("999000");
  });

  it("proxies Metis swap requests to the configured upstream endpoint", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis/",
    };
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [{
        swapInfo: {
          ammKey: "AMM111111111111111111111111111111111111111",
          inputMint: "So11111111111111111111111111111111111111112",
          outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
          inAmount: "1000000",
          outAmount: "999000",
          label: "Raydium",
        },
        percent: 100,
        bps: 10_000,
      }],
    };

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      expect(String(input)).toBe("https://triton.rpc.test/private-token/metis/swap");
      expect(init?.method).toBe("POST");
      expect(init?.headers).toBeInstanceOf(Headers);
      expect((init?.headers as Headers).get("content-type")).toBe("application/json");
      const rawBody = init?.body;
      const decodedBody = typeof rawBody === "string"
        ? rawBody
        : new TextDecoder().decode(rawBody as ArrayBuffer);
      expect(JSON.parse(decodedBody)).toMatchObject({
        userPublicKey: owner,
        quoteResponse,
      });
      return new Response(JSON.stringify({
        swapTransaction: "base64-tx",
      }), {
        status: 200,
        headers: {
          "content-type": "application/json",
        },
      });
    });

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
      }),
    }, metisEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      swapTransaction: string;
    };

    expect(json.swapTransaction).toBe("base64-tx");
  });

  it("returns a config error when the Metis endpoint is missing", async () => {
    const response = await app.request(
      "/v1/swap/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&amount=1000000",
      {},
      env,
    );

    expect(response.status).toBe(500);

    const json = await response.json() as {
      error: {
        code: string;
        message: string;
      };
    };

    expect(json.error.code).toBe("CONFIG_ERROR");
    expect(json.error.message).toBe("Missing worker environment variable `METIS_SWAP_API_URL`");
  });

  it("visibility=private forces the stash ATA and appends schedule_private_transfer", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const recipient = Keypair.generate().publicKey.toBase58();
    const validator = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [{
        swapInfo: {
          ammKey: "AMM111111111111111111111111111111111111111",
          inputMint: "So11111111111111111111111111111111111111112",
          outputMint,
          inAmount: "1000000",
          outAmount: "999000",
          label: "Raydium",
        },
        percent: 100,
        bps: 10_000,
      }],
    };

    const ownerPk = new PublicKey(owner);
    const stashPda = deriveStashPda(ownerPk, new PublicKey(outputMint))[0];
    const [stashAtaExpected] = deriveStashAta(ownerPk, new PublicKey(outputMint));

    // Jupiter-like mock: a v0 tx with a single memo ix and no ALTs.
    const memoIx = new TransactionInstruction({
      programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
      keys: [],
      data: Buffer.from("jupiter-mock"),
    });
    const jupiterV0 = new VersionedTransaction(
      new TransactionMessage({
        payerKey: ownerPk,
        recentBlockhash: "11111111111111111111111111111111",
        instructions: [memoIx],
      }).compileToV0Message(),
    );
    const jupiterBase64 = Buffer.from(jupiterV0.serialize()).toString("base64");

    let metisRequestBody: Record<string, unknown> | undefined;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);
      if (url.startsWith("https://triton.rpc.test") && url.endsWith("/swap")) {
        const rawBody = init?.body;
        metisRequestBody = JSON.parse(
          typeof rawBody === "string"
            ? rawBody
            : new TextDecoder().decode(rawBody as ArrayBuffer),
        );
        return new Response(
          JSON.stringify({ swapTransaction: jupiterBase64 }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }
      // BASE_RPC_URL (no ALTs in our fake tx, so nothing interesting to mock).
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    });

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "100",
        maxDelayMs: "300",
        split: 1,
        validator,
      }),
    }, metisEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      swapTransaction: string;
      privateTransfer: { stashAta: string; hydraCrankPda: string; shuttleId: number };
    };

    // Metis received the forced destinationTokenAccount + forced v0.
    expect(metisRequestBody?.destinationTokenAccount).toBe(stashAtaExpected.toBase58());
    expect(metisRequestBody?.asLegacyTransaction).toBe(false);
    // Private-only fields were stripped before proxying.
    expect(metisRequestBody?.visibility).toBeUndefined();
    expect(metisRequestBody?.destination).toBeUndefined();
    expect(metisRequestBody?.minDelayMs).toBeUndefined();
    expect(metisRequestBody?.split).toBeUndefined();

    // Diagnostic block is correct.
    expect(json.privateTransfer.stashAta).toBe(stashAtaExpected.toBase58());
    expect(typeof json.privateTransfer.shuttleId).toBe("number");
    // Crank PDA depends on the server-generated shuttleId; rederive to match.
    const [hydraCrankExpected] = deriveHydraCrankPda(stashPda, json.privateTransfer.shuttleId);
    expect(json.privateTransfer.hydraCrankPda).toBe(hydraCrankExpected.toBase58());

    // Returned tx: [idempotent-ATA-create, memo, schedule_private_transfer].
    // The Jupiter mock has no SetComputeUnitLimit, so we deliberately leave
    // the tx alone — Solana's default (200k × ix_count) covers us.
    const returned = VersionedTransaction.deserialize(
      Buffer.from(json.swapTransaction, "base64"),
    );
    const decompiled = TransactionMessage.decompile(returned.message, {
      addressLookupTableAccounts: [],
    });

    expect(decompiled.instructions).toHaveLength(3);

    const computeBudgetProgram = new PublicKey(
      "ComputeBudget111111111111111111111111111111",
    );
    // No SetComputeUnitLimit was prepended.
    expect(
      decompiled.instructions.some((ix) => ix.programId.equals(computeBudgetProgram)),
    ).toBe(false);

    const [createIx, memoRebuilt, scheduleIx] = decompiled.instructions;
    const ataProgram = new PublicKey(
      "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
    );
    expect(createIx.programId.toBase58()).toBe(ataProgram.toBase58());
    expect(createIx.data[0]).toBe(1); // CreateIdempotent

    expect(memoRebuilt.programId.toBase58()).toBe(memoIx.programId.toBase58());

    expect(scheduleIx.programId.toBase58()).toBe(
      EPHEMERAL_SPL_TOKEN_PROGRAM_ID.toBase58(),
    );
    expect(scheduleIx.data[0]).toBe(29);
    expect(scheduleIx.keys).toHaveLength(7);
    expect(scheduleIx.keys[1].pubkey.toBase58()).toBe(stashPda.toBase58());
    expect(scheduleIx.keys[4].pubkey.toBase58()).toBe(HYDRA_PROGRAM_ID.toBase58());
  });

  it("visibility=private honors a custom payer for appended instructions", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const sponsor = Keypair.generate().publicKey;
    const recipient = Keypair.generate().publicKey.toBase58();
    const validator = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const memoIx = new TransactionInstruction({
      programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
      keys: [],
      data: Buffer.from("jupiter-mock"),
    });
    const jupiterV0 = new VersionedTransaction(
      new TransactionMessage({
        payerKey: sponsor,
        recentBlockhash: "11111111111111111111111111111111",
        instructions: [memoIx],
      }).compileToV0Message(),
    );
    const jupiterBase64 = Buffer.from(jupiterV0.serialize()).toString("base64");

    let metisRequestBody: Record<string, unknown> | undefined;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);
      if (url.startsWith("https://triton.rpc.test") && url.endsWith("/swap")) {
        const rawBody = init?.body;
        metisRequestBody = JSON.parse(
          typeof rawBody === "string"
            ? rawBody
            : new TextDecoder().decode(rawBody as ArrayBuffer),
        );
        return new Response(
          JSON.stringify({ swapTransaction: jupiterBase64 }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    });

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        payer: sponsor.toBase58(),
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        validator,
      }),
    }, metisEnv);

    expect(response.status).toBe(200);
    expect(metisRequestBody?.payer).toBe(sponsor.toBase58());

    const json = await response.json() as { swapTransaction: string };
    const returned = VersionedTransaction.deserialize(
      Buffer.from(json.swapTransaction, "base64"),
    );
    const decompiled = TransactionMessage.decompile(returned.message, {
      addressLookupTableAccounts: [],
    });

    const [createIx, , scheduleIx] = decompiled.instructions;
    expect(returned.message.staticAccountKeys[0]?.toBase58()).toBe(sponsor.toBase58());
    expect(createIx?.keys[0]?.pubkey.toBase58()).toBe(sponsor.toBase58());
    expect(scheduleIx?.keys[0]?.pubkey.toBase58()).toBe(sponsor.toBase58());
  });

  it("visibility=private caches upstream swap lookup tables across rebuilds", async () => {
    const metisEnv = {
      ...env,
      BASE_RPC_URL: "https://base.swap.cached-lut.rpc.test",
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const recipient = Keypair.generate().publicKey.toBase58();
    const validator = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const ownerPk = new PublicKey(owner);
    const lookupKey = Keypair.generate().publicKey;
    const lookedUpAccount = Keypair.generate().publicKey;
    const lookupTable = createLookupTableAccount([lookedUpAccount], lookupKey);
    const memoIx = new TransactionInstruction({
      programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
      keys: [{ pubkey: lookedUpAccount, isSigner: false, isWritable: false }],
      data: Buffer.from("jupiter-mock"),
    });
    const jupiterV0 = new VersionedTransaction(
      new TransactionMessage({
        payerKey: ownerPk,
        recentBlockhash: "11111111111111111111111111111111",
        instructions: [memoIx],
      }).compileToV0Message([lookupTable]),
    );
    expect(jupiterV0.message.addressTableLookups).toHaveLength(1);
    const jupiterBase64 = Buffer.from(jupiterV0.serialize()).toString("base64");

    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(createLookupTableResponse(lookupTable));
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockResolvedValue(createLookupTableAccountInfo());

    let swapCalls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = String(input);
      if (url.startsWith("https://triton.rpc.test") && url.endsWith("/swap")) {
        swapCalls += 1;
        return new Response(
          JSON.stringify({ swapTransaction: jupiterBase64 }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    });

    const body = {
      userPublicKey: owner,
      quoteResponse,
      visibility: "private",
      destination: recipient,
      minDelayMs: "0",
      maxDelayMs: "0",
      split: 1,
      validator,
    };

    const firstResponse = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }, metisEnv);
    const secondResponse = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify(body),
    }, metisEnv);

    expect(firstResponse.status).toBe(200);
    expect(secondResponse.status).toBe(200);
    expect(swapCalls).toBe(2);
    expect(getAddressLookupTableSpy).toHaveBeenCalledOnce();
    expect(getAccountInfoSpy).not.toHaveBeenCalled();
  });

  it("visibility=private bumps an existing SetComputeUnitLimit in place rather than prepending", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const recipient = Keypair.generate().publicKey.toBase58();
    const validator = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const ownerPk = new PublicKey(owner);

    // Jupiter-like mock: a v0 tx that already carries a SetComputeUnitLimit
    // at 100k units (typical Jupiter number), followed by a memo.
    const existingLimit = 100_000;
    const setLimitData = Buffer.alloc(5);
    setLimitData[0] = 0x02;
    setLimitData.writeUInt32LE(existingLimit, 1);
    const computeBudgetProgram = new PublicKey(
      "ComputeBudget111111111111111111111111111111",
    );
    const existingLimitIx = new TransactionInstruction({
      programId: computeBudgetProgram,
      keys: [],
      data: setLimitData,
    });
    const memoIx = new TransactionInstruction({
      programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
      keys: [],
      data: Buffer.from("jupiter-mock"),
    });
    const jupiterV0 = new VersionedTransaction(
      new TransactionMessage({
        payerKey: ownerPk,
        recentBlockhash: "11111111111111111111111111111111",
        instructions: [existingLimitIx, memoIx],
      }).compileToV0Message(),
    );
    const jupiterBase64 = Buffer.from(jupiterV0.serialize()).toString("base64");

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = String(input);
      if (url.startsWith("https://triton.rpc.test") && url.endsWith("/swap")) {
        return new Response(
          JSON.stringify({ swapTransaction: jupiterBase64 }),
          { status: 200, headers: { "content-type": "application/json" } },
        );
      }
      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    });

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        validator,
      }),
    }, metisEnv);

    expect(response.status).toBe(200);
    const json = await response.json() as { swapTransaction: string };

    const returned = VersionedTransaction.deserialize(
      Buffer.from(json.swapTransaction, "base64"),
    );
    const decompiled = TransactionMessage.decompile(returned.message, {
      addressLookupTableAccounts: [],
    });

    // Same 4 ixs (no new SetComputeUnitLimit prepended), but the existing
    // one has been rewritten with a bumped value.
    expect(decompiled.instructions).toHaveLength(4);
    const cbCount = decompiled.instructions.filter((ix) =>
      ix.programId.equals(computeBudgetProgram),
    ).length;
    expect(cbCount).toBe(1);

    const cbIx = decompiled.instructions.find((ix) =>
      ix.programId.equals(computeBudgetProgram),
    )!;
    const bumped = Buffer.from(cbIx.data).readUInt32LE(1);
    expect(bumped).toBe(existingLimit + 40_000);
  });

  it("visibility=private returns 502 when the upstream swap response is missing swapTransaction", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };

    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(
      JSON.stringify({ lastValidBlockHeight: 123 }),
      { status: 200, headers: { "content-type": "application/json" } },
    ));

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse: {
          inputMint: "So11111111111111111111111111111111111111112",
          inAmount: "1000000",
          outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
          outAmount: "999000",
          otherAmountThreshold: "998000",
          swapMode: "ExactIn",
          slippageBps: 50,
          priceImpactPct: "0.01",
          routePlan: [],
        },
        visibility: "private",
        destination,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
      }),
    }, metisEnv);

    expect(response.status).toBe(502);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("SWAP_UPSTREAM_ERROR");
    expect(json.error.message).toBe("Upstream swap response missing swapTransaction");
  });

  it("visibility=private returns 502 when the upstream swap transaction is invalid", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };

    vi.spyOn(globalThis, "fetch").mockResolvedValue(new Response(
      JSON.stringify({ swapTransaction: "%%%%" }),
      { status: 200, headers: { "content-type": "application/json" } },
    ));

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse: {
          inputMint: "So11111111111111111111111111111111111111112",
          inAmount: "1000000",
          outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
          outAmount: "999000",
          otherAmountThreshold: "998000",
          swapMode: "ExactIn",
          slippageBps: 50,
          priceImpactPct: "0.01",
          routePlan: [],
        },
        visibility: "private",
        destination,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
      }),
    }, metisEnv);

    expect(response.status).toBe(502);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("SWAP_UPSTREAM_ERROR");
    expect(json.error.message).toBe("Invalid upstream swap transaction encoding");
  });

  it("visibility=private requotes downward from maxAccounts=39 until the rebuilt transaction fits", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const recipient = Keypair.generate().publicKey.toBase58();
    const validator = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const ownerPk = new PublicKey(owner);
    const memoIx = new TransactionInstruction({
      programId: new PublicKey("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr"),
      keys: [],
      data: Buffer.from("jupiter-mock"),
    });
    const jupiterV0 = new VersionedTransaction(
      new TransactionMessage({
        payerKey: ownerPk,
        recentBlockhash: "11111111111111111111111111111111",
        instructions: [memoIx],
      }).compileToV0Message(),
    );
    const jupiterBase64 = Buffer.from(jupiterV0.serialize()).toString("base64");

    const originalSerialize = VersionedTransaction.prototype.serialize;
    let serializeCalls = 0;
    vi.spyOn(VersionedTransaction.prototype, "serialize").mockImplementation(function(
      this: VersionedTransaction,
    ) {
      serializeCalls += 1;
      if (serializeCalls <= 2) {
        return new Uint8Array(1233);
      }
      return originalSerialize.call(this);
    });

    const quoteMaxAccounts: string[] = [];
    let swapCalls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = new URL(String(input));

      if (url.origin === "https://triton.rpc.test" && url.pathname.endsWith("/quote")) {
        quoteMaxAccounts.push(url.searchParams.get("maxAccounts") ?? "");
        return new Response(JSON.stringify(quoteResponse), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }

      if (url.origin === "https://triton.rpc.test" && url.pathname.endsWith("/swap")) {
        swapCalls += 1;
        return new Response(JSON.stringify({ swapTransaction: jupiterBase64 }), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }

      return new Response("{}", { status: 200, headers: { "content-type": "application/json" } });
    });

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        validator,
      }),
    }, metisEnv);

    expect(response.status).toBe(200);
    expect(swapCalls).toBe(3);
    expect(quoteMaxAccounts).toEqual(["39", "38"]);

    const json = await response.json() as { swapTransaction: string };
    expect(typeof json.swapTransaction).toBe("string");
    expect(json.swapTransaction.length).toBeGreaterThan(0);
  });

  it("visibility=private rejects missing required fields", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        // destination + delays + split intentionally missing
      }),
    }, metisEnv);

    expect(response.status).toBe(400);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toMatch(/destination, minDelayMs/);
  });

  it("visibility=private rejects maxDelayMs above 10 minutes", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination,
        minDelayMs: "0",
        maxDelayMs: "600001",
        split: 1,
      }),
    }, metisEnv);

    expect(response.status).toBe(400);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toBe("maxDelayMs must be less than or equal to 600000");
  });

  it("visibility=private rejects split values above 14", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 15,
      }),
    }, metisEnv);

    expect(response.status).toBe(400);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toBe("split must be an integer between 1 and 14 when visibility=private");
  });

  it("visibility=private rejects destinationTokenAccount", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const recipient = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        destinationTokenAccount: Keypair.generate().publicKey.toBase58(),
      }),
    }, metisEnv);

    expect(response.status).toBe(400);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toMatch(/destinationTokenAccount is not supported/);
  });

  it("visibility=private rejects asLegacyTransaction=true", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v";
    const recipient = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "So11111111111111111111111111111111111111112",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        asLegacyTransaction: true,
      }),
    }, metisEnv);

    expect(response.status).toBe(400);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toMatch(/asLegacyTransaction is not supported/);
  });

  it("visibility=private rejects nativeDestinationAccount", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };
    const outputMint = "So11111111111111111111111111111111111111112";
    const recipient = Keypair.generate().publicKey.toBase58();
    const quoteResponse = {
      inputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      inAmount: "1000000",
      outputMint,
      outAmount: "999000",
      otherAmountThreshold: "998000",
      swapMode: "ExactIn",
      slippageBps: 50,
      priceImpactPct: "0.01",
      routePlan: [],
    };

    const response = await app.request("/v1/swap/swap", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({
        userPublicKey: owner,
        quoteResponse,
        visibility: "private",
        destination: recipient,
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        nativeDestinationAccount: Keypair.generate().publicKey.toBase58(),
      }),
    }, metisEnv);

    expect(response.status).toBe(400);
    const json = await response.json() as { error: { code: string; message: string } };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toBe("nativeDestinationAccount is not supported when visibility=private");
  });

  it("serves MCP info and discovery documents", async () => {
    const mcpResponse = await app.request("/mcp", {}, env);
    const discoveryResponse = await app.request("/.well-known/mcp.json", {}, env);

    expect(mcpResponse.status).toBe(200);
    expect(discoveryResponse.status).toBe(200);

    const mcpJson = await mcpResponse.json() as {
      endpoint: string;
      discovery: string;
      tools: Array<{ name: string }>;
    };
    const discoveryJson = await discoveryResponse.json() as {
      transport: { endpoint: string; type: string };
      tools: Array<{ name: string }>;
    };

    expect(mcpJson.endpoint).toBe("http://localhost/mcp");
    expect(mcpJson.discovery).toBe("http://localhost/.well-known/mcp.json");
    expect(mcpJson.tools.some((tool) => tool.name === "spl.transfer")).toBe(true);

    expect(discoveryJson.transport.type).toBe("streamable-http");
    expect(discoveryJson.transport.endpoint).toBe("http://localhost/mcp");
    expect(discoveryJson.tools.some((tool) => tool.name === "spl.getPrivateBalance")).toBe(true);
  });

  it("accepts MCP initialize requests from doc clients that do not send a JSON content type", async () => {
    const response = await app.request("/mcp", {
      method: "POST",
      headers: {
        accept: "application/json",
      },
      body: JSON.stringify({
        jsonrpc: "2.0",
        id: 1,
        method: "initialize",
        params: {
          protocolVersion: "2025-11-25",
          capabilities: {},
          clientInfo: {
            name: "doc-example",
            version: "1.0.0",
          },
        },
      }),
    }, env);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      result?: {
        protocolVersion?: string;
      };
    };

    expect(json.result?.protocolVersion).toBe("2025-11-25");
  });

  it("builds an unsigned deposit transaction", async () => {
    const depositEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.deposit.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(depositEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/deposit", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        amount: 1,
        idempotent: true,
        initIfMissing: true,
        initAtasIfMissing: true,
        initVaultIfMissing: false,
      }),
    }, depositEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      sendTo: string;
      transactionBase64: string;
      recentBlockhash: string;
      validator: string;
    };

    expect(json.sendTo).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    expect(transaction.instructions.length).toBeGreaterThan(0);
    const depositIx = transaction.instructions[transaction.instructions.length - 1]!;
    expect(depositIx.data[0]).toBe(24);
    expect(depositIx.data.length).toBe(45);
  });

  it("falls back to the default validator after a transient RPC failure and retries later", async () => {
    const retryEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.retry.rpc.test",
    };
    const fallbackValidator = "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57";
    let fetchCalls = 0;

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(retryEnv.EPHEMERAL_RPC_URL);
      fetchCalls += 1;

      return fetchCalls === 1
        ? new Response("upstream error", { status: 502 })
        : createIdentityResponse(resolvedValidator);
    });

    const firstResponse = await app.request("/v1/spl/deposit", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        amount: 1,
      }),
    }, retryEnv);

    expect(firstResponse.status).toBe(200);

    const firstJson = await firstResponse.json() as {
      validator: string;
    };

    expect(firstJson.validator).toBe(fallbackValidator);

    const secondResponse = await app.request("/v1/spl/deposit", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        amount: 1,
      }),
    }, retryEnv);

    expect(secondResponse.status).toBe(200);
    expect(fetchCalls).toBe(2);

    const json = await secondResponse.json() as {
      validator: string;
    };

    expect(json.validator).toBe(resolvedValidator);
  });

  it("uses the devnet RPC endpoints when cluster=devnet", async () => {
    const devnetEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.deposit.devnet.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(async function getLatestBlockhash(this: Connection & { _rpcEndpoint: string }) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      return endpoint.includes("base.devnet.rpc.test")
        ? {
          blockhash: "So11111111111111111111111111111111111111112",
          lastValidBlockHeight: 321,
        }
        : {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        };
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(devnetEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/deposit", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        amount: 1,
        cluster: "devnet",
      }),
    }, devnetEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      recentBlockhash: string;
      validator: string;
      transactionBase64: string;
    };

    expect(json.recentBlockhash).toBe("So11111111111111111111111111111111111111112");
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    expect(transaction.instructions.some((instruction) =>
      instruction.keys.some((key) => key.pubkey.toBase58() === DEVNET_USDC_MINT)
    )).toBe(true);
  });

  it("defaults the worker cluster binding to mainnet when it is omitted", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createAccountInfo(3n));

    const response = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      {
        ...env,
        CLUSTER: undefined,
      },
    );

    expect(response.status).toBe(200);

    const json = await response.json() as {
      location: string;
      balance: string;
    };
    expect(json.location).toBe("base");
    expect(json.balance).toBe("3");
  });

  it("builds an unsigned withdraw transaction with integer amount", async () => {
    const withdrawEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.withdraw.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(withdrawEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/withdraw", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        mint: "So11111111111111111111111111111111111111112",
        amount: 1,
        idempotent: true,
      }),
    }, withdrawEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      sendTo: string;
      transactionBase64: string;
      recentBlockhash: string;
      validator: string;
    };

    expect(json.sendTo).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    expect(transaction.instructions.length).toBeGreaterThan(0);
    const withdrawIx = transaction.instructions[transaction.instructions.length - 1]!;
    expect(withdrawIx.data[0]).toBe(26);
    expect(withdrawIx.data.length).toBe(45);
  });

  it("builds an initialize mint transaction with the expected queue setup instructions", async () => {
    const validatorPublicKey = Keypair.generate().publicKey;
    const validator = validatorPublicKey.toBase58();
    const mint = "So11111111111111111111111111111111111111112";
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(new PublicKey(mint));
    const [vaultEphemeralAta] = deriveEphemeralAta(vault, new PublicKey(mint));
    const vaultAta = deriveVaultAta(new PublicKey(mint), vault);
    const fetchSpy = vi.spyOn(globalThis, "fetch");

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(async function getLatestBlockhash(this: Connection & { _rpcEndpoint: string }) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      return endpoint.includes("base")
        ? {
          blockhash: "So11111111111111111111111111111111111111112",
          lastValidBlockHeight: 321,
        }
        : {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        };
    });

    const response = await app.request("/v1/spl/initialize-mint", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        payer: owner,
        mint,
        validator,
      }),
    }, env);

    expect(response.status).toBe(200);
    expect(fetchSpy).not.toHaveBeenCalled();

    const json = await response.json() as {
      kind: string;
      sendTo: string;
      validator: string;
      transferQueue: string;
      rentPda: string;
      recentBlockhash: string;
      instructionCount: number;
      transactionBase64: string;
    };

    expect(json.kind).toBe("initializeMint");
    expect(json.sendTo).toBe("base");
    expect(json.validator).toBe(validator);
    expect(json.transferQueue).toBe(transferQueue.toBase58());
    expect(json.rentPda).toBe(rentPda.toBase58());
    expect(json.recentBlockhash).toBe("So11111111111111111111111111111111111111112");
    expect(json.instructionCount).toBe(7);

    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    expect(transaction.instructions).toHaveLength(7);
    expect(transaction.instructions[2]?.programId.toBase58()).toBe(SystemProgram.programId.toBase58());

    const decodedTransfer = SystemInstruction.decodeTransfer(transaction.instructions[2]!);
    expect(decodedTransfer.fromPubkey.toBase58()).toBe(owner);
    expect(decodedTransfer.toPubkey.toBase58()).toBe(rentPda.toBase58());
    expect(decodedTransfer.lamports).toBe(BigInt(LAMPORTS_PER_SOL / 50));

    expect(transaction.instructions[0]?.keys.some((key) => key.pubkey.toBase58() === transferQueue.toBase58())).toBe(true);
    expect(transaction.instructions[1]?.keys.some((key) => key.pubkey.toBase58() === rentPda.toBase58())).toBe(true);
    expect(transaction.instructions[3]?.keys.some((key) => key.pubkey.toBase58() === transferQueue.toBase58())).toBe(true);
    expect(transaction.instructions[4]?.keys.some((key) => key.pubkey.toBase58() === vault.toBase58())).toBe(true);
    expect(transaction.instructions[5]?.keys.some((key) => key.pubkey.toBase58() === vaultAta.toBase58())).toBe(true);
    expect(transaction.instructions[6]?.keys.some((key) => key.pubkey.toBase58() === vaultEphemeralAta.toBase58())).toBe(true);
    expect(Array.from(transaction.instructions[0]!.data)).toEqual([12]);
    expect(Array.from(transaction.instructions[1]!.data)).toEqual([23]);
    expect(Array.from(transaction.instructions[3]!.data)).toEqual([19]);
    expect(Array.from(transaction.instructions[4]!.data)).toEqual([1]);
    expect(Array.from(transaction.instructions[6]!.data)).toEqual([4, ...validatorPublicKey.toBytes()]);
  });

  it("defaults the validator when building an initialize mint transaction", async () => {
    const initializeMintEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.initialize-mint.rpc.test",
    };
    const mint = "So11111111111111111111111111111111111111112";

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "So11111111111111111111111111111111111111112",
      lastValidBlockHeight: 321,
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(initializeMintEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/initialize-mint", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        payer: owner,
        mint,
      }),
    }, initializeMintEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      validator: string;
      instructionCount: number;
    };

    expect(json.validator).toBe(resolvedValidator);
    expect(json.instructionCount).toBe(7);
  });

  it("returns a config error when RPC env vars are missing", async () => {
    const response = await app.request("/v1/spl/deposit", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        amount: 1,
      }),
    }, {
      CORS_ORIGIN: "*",
    });

    expect(response.status).toBe(500);

    const json = await response.json() as {
      error: {
        code: string;
        details?: {
          hint?: string;
        };
      };
    };

    expect(json.error.code).toBe("CONFIG_ERROR");
    expect(json.error.details?.hint).toContain(".dev.vars");
  });

  it("returns a config error when devnet RPC env vars are invalid URLs", async () => {
    const response = await app.request("/v1/spl/deposit", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        owner,
        amount: 1,
      }),
    }, {
      ...env,
      BASE_DEVNET_RPC_URL: "not-a-url",
      EPHEMERAL_DEVNET_RPC_URL: "still-not-a-url",
    });

    expect(response.status).toBe(500);

    const json = await response.json() as {
      error: {
        code: string;
        details?: {
          issues?: Array<{
            path?: string[];
          }>;
        };
      };
    };

    expect(json.error.code).toBe("CONFIG_ERROR");
    expect(json.error.details?.issues).toEqual(expect.arrayContaining([
      expect.objectContaining({ path: ["BASE_DEVNET_RPC_URL"] }),
      expect.objectContaining({ path: ["EPHEMERAL_DEVNET_RPC_URL"] }),
    ]));
  });

  it("exposes the SPL tools over MCP", async () => {
    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });

    const client = new Client({
      name: "vitest-client",
      version: "1.0.0",
    });
    const transport = new StreamableHTTPClientTransport(new URL("http://localhost/mcp"), {
      fetch: createMcpFetch(),
    });

    await client.connect(transport);

    const tools = await client.listTools();
    expect(tools.tools.some((tool) => tool.name === "spl.deposit")).toBe(true);
    expect(tools.tools.some((tool) => tool.name === "spl.getPrivateBalance")).toBe(true);

    const result = await client.callTool({
      name: "spl.deposit",
      arguments: {
        owner,
        amount: 1,
        validator: "11111111111111111111111111111111",
      },
    });

    expect(result.isError).toBeUndefined();
    expect(result.structuredContent).toMatchObject({
      kind: "deposit",
      sendTo: "base",
    });

    await client.close();
    await transport.close();
  });

  it("uses the ephemeral blockhash for ephemeral transfers", async () => {
    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(async function getLatestBlockhash(this: Connection & { _rpcEndpoint: string }) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      return endpoint.includes("ephemeral")
        ? {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 456,
        }
        : {
          blockhash: "So11111111111111111111111111111111111111112",
          lastValidBlockHeight: 123,
        };
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 1,
        visibility: "public",
        fromBalance: "ephemeral",
        toBalance: "ephemeral",
      }),
    }, env);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      sendTo: string;
      recentBlockhash: string;
      instructionCount: number;
    };

    expect(json.sendTo).toBe("ephemeral");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.instructionCount).toBe(1);
  });

  it("builds a private transfer with top-level split and delay options", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(createLookupTableResponse(null));
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      sendTo: string;
      recentBlockhash: string;
      validator: string;
      version: string;
    };

    expect(json.sendTo).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);
    expect(json.version).toBe("legacy");
  });

  it("builds a v0 private base transfer when the LUT is useful", async () => {
    const transferEnv = {
      ...env,
      BASE_RPC_URL: "https://base.transfer.v0.rpc.test",
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.v0.rpc.test",
    };
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const validator = new PublicKey(resolvedValidator);
    const [transferQueue] = deriveTransferQueue(mint, validator);
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault);

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(createLookupTableResponse(
      createLookupTableAccount([
        mint,
        transferQueue,
        rentPda,
        vault,
        vaultAta,
        TOKEN_PROGRAM_ID,
        SystemProgram.programId,
      ]),
    ));
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createLookupTableAccountInfo());
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: mint.toBase58(),
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      version: string;
      transactionBase64: string;
    };

    expect(json.version).toBe("v0");
    expect(() => VersionedTransaction.deserialize(Buffer.from(json.transactionBase64, "base64"))).not.toThrow();
  });

  it("caches the private transfer lookup table across transfer builds", async () => {
    const transferEnv = {
      ...env,
      BASE_RPC_URL: "https://base.transfer.cached-lut.rpc.test",
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.cached-lut.rpc.test",
    };
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const validator = new PublicKey(resolvedValidator);
    const [transferQueue] = deriveTransferQueue(mint, validator);
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault);

    const getLatestBlockhashSpy = vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(createLookupTableResponse(
        createLookupTableAccount([
          mint,
          transferQueue,
          rentPda,
          vault,
          vaultAta,
          TOKEN_PROGRAM_ID,
          SystemProgram.programId,
        ]),
      ));
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockResolvedValue(createLookupTableAccountInfo());

    const body = {
      from: owner,
      to: destination,
      mint: mint.toBase58(),
      amount: 2,
      visibility: "private",
      fromBalance: "base",
      toBalance: "base",
      validator: resolvedValidator,
      minDelayMs: "0",
      maxDelayMs: "0",
      split: 1,
    };

    const firstResponse = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify(body),
    }, transferEnv);
    const secondResponse = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify(body),
    }, transferEnv);

    expect(firstResponse.status).toBe(200);
    expect(secondResponse.status).toBe(200);
    expect(getLatestBlockhashSpy).toHaveBeenCalledTimes(2);
    expect(getAddressLookupTableSpy).toHaveBeenCalledOnce();
    expect(getAccountInfoSpy).toHaveBeenCalledOnce();
  });

  it("returns a legacy private base transfer when legacy=true even if the LUT is useful", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.legacy.rpc.test",
    };
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const validator = new PublicKey(resolvedValidator);
    const [transferQueue] = deriveTransferQueue(mint, validator);
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault);

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(createLookupTableResponse(
        createLookupTableAccount([
          mint,
          transferQueue,
          rentPda,
          vault,
          vaultAta,
          TOKEN_PROGRAM_ID,
          SystemProgram.programId,
        ]),
      ));
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createLookupTableAccountInfo());
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: mint.toBase58(),
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        legacy: true,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      version: string;
      transactionBase64: string;
    };

    expect(json.version).toBe("legacy");
    expect(() => Transaction.from(Buffer.from(json.transactionBase64, "base64"))).not.toThrow();
    expect(getAddressLookupTableSpy).not.toHaveBeenCalled();
  });

  it("falls back to a legacy private base transfer when the LUT has no matching addresses", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.no-match.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(createLookupTableResponse(
        createLookupTableAccount([
          Keypair.generate().publicKey,
          Keypair.generate().publicKey,
          Keypair.generate().publicKey,
          Keypair.generate().publicKey,
          Keypair.generate().publicKey,
          Keypair.generate().publicKey,
          Keypair.generate().publicKey,
        ]),
      ));
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createLookupTableAccountInfo());
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      version: string;
      transactionBase64: string;
    };

    expect(json.version).toBe("legacy");
    expect(() => Transaction.from(Buffer.from(json.transactionBase64, "base64"))).not.toThrow();
    expect(getAddressLookupTableSpy).toHaveBeenCalledOnce();
  });

  it("builds a gasless private transfer with the sponsor as fee payer", async () => {
    const sponsor = Keypair.generate();
    const mint = DEVNET_USDC_MINT;
    const amount = 5_000_000;
    const ownerAta = deriveAssociatedTokenAddress(mint, owner);
    const sponsorAta = deriveAssociatedTokenAddress(mint, sponsor.publicKey.toBase58());
    const transferEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.gasless-transfer.rpc.test",
      GASLESS_SPONSOR_SECRET_KEY: JSON.stringify(Array.from(sponsor.secretKey)),
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(null);
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation((array) => {
      if (array instanceof Uint32Array) {
        array.fill(7);
        return array;
      }

      (array as Uint8Array).fill(1);
      return array;
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint,
        amount,
        cluster: "devnet",
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        minDelayMs: "0",
        maxDelayMs: "0",
        split: 1,
        gasless: true,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      requiredSigners: string[];
      transactionBase64: string;
    };
    expect(json.requiredSigners).toEqual(expect.arrayContaining([
      owner,
      sponsor.publicKey.toBase58(),
    ]));

    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    expect(transaction.feePayer?.toBase58()).toBe(sponsor.publicKey.toBase58());
    expect(transaction.instructions).toHaveLength(2);

    const sponsorSignature = transaction.signatures.find((signature) =>
      signature.publicKey.toBase58() === sponsor.publicKey.toBase58(),
    );
    expect(sponsorSignature?.signature).not.toBeNull();

    const relayFeeIx = transaction.instructions[0]!;
    expect(relayFeeIx.programId.toBase58()).toBe(TOKEN_PROGRAM_ID.toBase58());
    expect(relayFeeIx.keys.map((key) => key.pubkey.toBase58())).toEqual([
      ownerAta,
      sponsorAta,
      owner,
    ]);
    expect(relayFeeIx.data[0]).toBe(3);
    expect(relayFeeIx.data.readBigUInt64LE(1)).toBe(200_000n);

    const privateTransferIx = transaction.instructions[1]!;
    expect(privateTransferIx.programId.toBase58()).toBe(EPHEMERAL_SPL_TOKEN_PROGRAM_ID.toBase58());
    expect(privateTransferIx.data[0]).toBe(25);
    expect(privateTransferIx.data.readUInt32LE(1)).toBe(7);
    expect(privateTransferIx.data.readBigUInt64LE(5)).toBe(BigInt(amount));
    expect(privateTransferIx.data[13]).toBe(1);
    expect(privateTransferIx.data[94]).toBe(1);
    expect(privateTransferIx.data.subarray(95, 127)).toEqual(new PublicKey(resolvedValidator).toBuffer());
    expect(privateTransferIx.data[127]).toBe(privateTransferIx.data.length - 128);
  });

  it("builds a gasless public transfer with the sponsor as fee payer", async () => {
    const sponsor = Keypair.generate();
    const mint = DEVNET_USDC_MINT;
    const amount = 5_000_000;
    const ownerAta = deriveAssociatedTokenAddress(mint, owner);
    const sponsorAta = deriveAssociatedTokenAddress(mint, sponsor.publicKey.toBase58());
    const destinationAta = deriveAssociatedTokenAddress(mint, destination);
    const transferEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.gasless-public-transfer.rpc.test",
      GASLESS_SPONSOR_SECRET_KEY: JSON.stringify(Array.from(sponsor.secretKey)),
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(null);
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint,
        amount,
        cluster: "devnet",
        visibility: "public",
        fromBalance: "base",
        toBalance: "base",
        gasless: true,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      requiredSigners: string[];
      transactionBase64: string;
    };
    expect(json.requiredSigners).toEqual(expect.arrayContaining([
      owner,
      sponsor.publicKey.toBase58(),
    ]));

    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    expect(transaction.feePayer?.toBase58()).toBe(sponsor.publicKey.toBase58());
    expect(transaction.instructions).toHaveLength(2);

    const relayFeeIx = transaction.instructions[0]!;
    expect(relayFeeIx.programId.toBase58()).toBe(TOKEN_PROGRAM_ID.toBase58());
    expect(relayFeeIx.keys.map((key) => key.pubkey.toBase58())).toEqual([
      ownerAta,
      sponsorAta,
      owner,
    ]);
    expect(relayFeeIx.data[0]).toBe(3);
    expect(relayFeeIx.data.readBigUInt64LE(1)).toBe(200_000n);

    const publicTransferIx = transaction.instructions[1]!;
    expect(publicTransferIx.programId.toBase58()).toBe(TOKEN_PROGRAM_ID.toBase58());
    expect(publicTransferIx.keys.map((key) => key.pubkey.toBase58())).toEqual([
      ownerAta,
      destinationAta,
      owner,
    ]);
    expect(publicTransferIx.data[0]).toBe(3);
    expect(publicTransferIx.data.readBigUInt64LE(1)).toBe(BigInt(amount));
  });

  it("rejects gasless transfers when the sponsor key is not configured", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.gasless-missing.rpc.test",
    };

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: DEVNET_USDC_MINT,
        amount: 5_000_000,
        cluster: "devnet",
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        gasless: true,
      }),
    }, transferEnv);

    expect(response.status).toBe(503);

    const json = await response.json() as {
      error: {
        code: string;
        message: string;
      };
    };
    expect(json.error.code).toBe("SPONSOR_UNAVAILABLE");
    expect(json.error.message).toBe("Gasless transfers are not configured");
  });

  it("includes clientRefId in private transfer payloads when provided", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.client-ref.rpc.test",
    };
    const baseBody = {
      from: owner,
      to: destination,
      mint: "So11111111111111111111111111111111111111112",
      amount: 2,
      visibility: "private" as const,
      fromBalance: "base" as const,
      toBalance: "base" as const,
      minDelayMs: "0",
      maxDelayMs: "0",
      split: 1,
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(createLookupTableResponse(null));
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation((array) => {
      if (array instanceof Uint32Array) {
        array.fill(7);
        return array;
      }

      (array as Uint8Array).fill(1);
      return array;
    });

    const withoutClientRefResponse = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify(baseBody),
    }, transferEnv);

    const withClientRefResponse = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        ...baseBody,
        clientRefId: "42",
      }),
    }, transferEnv);

    expect(withoutClientRefResponse.status).toBe(200);
    expect(withClientRefResponse.status).toBe(200);

    const withoutClientRefJson = await withoutClientRefResponse.json() as {
      instructionCount: number;
      transactionBase64: string;
    };
    const withClientRefJson = await withClientRefResponse.json() as {
      instructionCount: number;
      transactionBase64: string;
    };

    expect(withClientRefJson.instructionCount).toBe(withoutClientRefJson.instructionCount);
    expect(withClientRefJson.transactionBase64).not.toBe(withoutClientRefJson.transactionBase64);

    const withoutClientRefTx = Transaction.from(Buffer.from(withoutClientRefJson.transactionBase64, "base64"));
    const withClientRefTx = Transaction.from(Buffer.from(withClientRefJson.transactionBase64, "base64"));

    expect(withClientRefTx.instructions).toHaveLength(withoutClientRefTx.instructions.length);
    expect(withClientRefTx.instructions.map((instruction) => Buffer.from(instruction.data).toString("base64"))).not.toEqual(
      withoutClientRefTx.instructions.map((instruction) => Buffer.from(instruction.data).toString("base64")),
    );
  });

  it("rejects split values above 15", async () => {
    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        split: 16,
      }),
    }, env);

    expect(response.status).toBe(422);
  });

  it("validates clientRefId as a bigint string at the API layer", async () => {
    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        clientRefId: "1.5",
      }),
    }, env);

    expect(response.status).toBe(422);

    const json = await response.json() as {
      error: {
        issues: Array<{
          path: string[];
          message: string;
        }>;
      };
    };

    expect(json.error.issues).toEqual(expect.arrayContaining([
      expect.objectContaining({
        path: ["clientRefId"],
        message: "Must be a non-negative bigint string",
      }),
    ]));
  });

  it("rejects private transfers with maxDelayMs above 10 minutes", async () => {
    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 2,
        visibility: "private",
        fromBalance: "base",
        toBalance: "base",
        maxDelayMs: "600001",
      }),
    }, env);

    expect(response.status).toBe(400);

    const json = await response.json() as {
      error: {
        code: string;
        message: string;
      };
    };

    expect(json.error.code).toBe("INVALID_PRIVATE_TRANSFER");
    expect(json.error.message).toBe("maxDelayMs must be less than or equal to 600000");
  });

  it("appends a memo instruction to transfers when memo is provided", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.memo.rpc.test",
    };
    const memo = "hello from memo";

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 1,
        visibility: "public",
        fromBalance: "base",
        toBalance: "base",
        memo,
      }),
    }, transferEnv);

    expect(response.status).toBe(200);

    const json = await response.json() as {
      instructionCount: number;
      transactionBase64: string;
    };
    const transaction = Transaction.from(Buffer.from(json.transactionBase64, "base64"));
    const memoInstruction = transaction.instructions.at(-1);

    expect(json.instructionCount).toBe(2);
    expect(memoInstruction?.programId.toBase58()).toBe("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");
    expect(memoInstruction?.data.toString("utf8")).toBe(memo);
  });

  it("returns a 400 for unsupported transfer combinations", async () => {
    const unsupportedTransferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.unsupported-transfer.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(unsupportedTransferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request("/v1/spl/transfer", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        from: owner,
        to: destination,
        mint: "So11111111111111111111111111111111111111112",
        amount: 1,
        visibility: "public",
        fromBalance: "base",
        toBalance: "ephemeral",
      }),
    }, unsupportedTransferEnv);

    expect(response.status).toBe(400);

    const json = await response.json() as { error: { code: string } };
    expect(json.error.code).toBe("UNSUPPORTED_TRANSFER_ROUTE");
  });

  it("returns base and private balances from different RPCs", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      return endpoint.includes("ephemeral")
        ? createAccountInfo(9n)
        : createAccountInfo(3n);
    });

    const baseResponse = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      env,
    );
    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      { headers: { authorization: "Bearer 1234567890" } },
      env,
    );

    expect(baseResponse.status).toBe(200);
    expect(privateResponse.status).toBe(200);

    const baseJson = await baseResponse.json() as { location: string; balance: string };
    const privateJson = await privateResponse.json() as { location: string; balance: string };

    expect(baseJson.location).toBe("base");
    expect(baseJson.balance).toBe("3");
    expect(privateJson.location).toBe("ephemeral");
    expect(privateJson.balance).toBe("9");
  });

  it("returns base and mock private balances from different RPCs", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      return endpoint.includes("ephemeral")
        ? createAccountInfo(9n)
        : createAccountInfo(3n);
    });

    const baseResponse = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      env,
    );
    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      { headers: { authorization: "Bearer mock-auth-token" } },
      env,
    );

    expect(baseResponse.status).toBe(200);
    expect(privateResponse.status).toBe(200);

    const baseJson = await baseResponse.json() as { location: string; balance: string };
    const privateJson = await privateResponse.json() as { location: string; balance: string };

    expect(baseJson.location).toBe("base");
    expect(baseJson.balance).toBe("3");
    expect(privateJson.location).toBe("base");
    expect(privateJson.balance).toBe("3");
  });

  it("returns 422 when private balance is requested without authToken", async () => {
    const response = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      env,
    );
    expect(response.status).toBe(422);
  });

  it("returns a challenge from the ephemeral rollup auth endpoint", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(`${env.EPHEMERAL_RPC_URL}/auth/challenge?pubkey=${owner}`);
      return new Response(JSON.stringify({ challenge: "challenge-token-abc" }), {
        status: 200,
        headers: {
          "content-type": "application/json",
        },
      });
    });

    const response = await app.request(
      `/v1/spl/challenge?pubkey=${owner}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = await response.json() as { challenge: string };
    expect(json.challenge).toBe("challenge-token-abc");
  });

  it("returns the mock challenge", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(`${env.EPHEMERAL_RPC_URL}/auth/challenge?pubkey=${owner}`);
      return new Response(JSON.stringify({
        "jsonrpc": "2.0",
        "error": {
          "code": -32600,
          "message": "invalid request: missing request body"
        }
      }), {
        status: 200,
        headers: {
          "content-type": "application/json",
        },
      });
    });

    const response = await app.request(
      `/v1/spl/challenge?pubkey=${owner}`,
      {},
      env,
    );

    console.log(response);
    expect(response.status).toBe(200);

    const json = await response.json() as { challenge: string };
    expect(json.challenge).toMatch(/Login to Query Filtering Service/);
    expect(json.challenge).toContain(owner);
  });

  it("returns a token from the ephemeral rollup login endpoint", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      expect(String(input)).toBe(`${env.EPHEMERAL_RPC_URL}/auth/login`);
      expect(init?.method).toBe("POST");
      const body = JSON.parse(init?.body as string) as {
        pubkey: string;
        challenge: string;
        signature: string;
      };
      expect(body.pubkey).toBe(owner);
      expect(body.challenge).toBe("c1");
      expect(body.signature).toBe("s1");
      return new Response(JSON.stringify({ token: "token-xyz" }), {
        status: 200,
        headers: {
          "content-type": "application/json",
        },
      });
    });

    const response = await app.request("/v1/spl/login", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        pubkey: owner,
        challenge: "c1",
        signature: "s1",
      }),
    }, env);

    expect(response.status).toBe(200);

    const json = await response.json() as { token: string };
    expect(json.token).toBe("token-xyz");
  });

  it("returns the mock token", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(`${env.EPHEMERAL_RPC_URL}/auth/login`);
      return new Response(JSON.stringify({
        "jsonrpc": "2.0",
        "error": {
          "code": -32700,
          "message": "error parsing request body: missing field `id` at line 1 column 91\n\n\tre\":\"s1\"}\n\t........^\n"
        }
      }), {
        status: 200,
        headers: {
          "content-type": "application/json",
        },
      });
    });

    const response = await app.request("/v1/spl/login", {
      method: "POST",
      headers: {
        "content-type": "application/json",
      },
      body: JSON.stringify({
        pubkey: owner,
        challenge: "c1",
        signature: "s1",
      }),
    }, env);

    expect(response.status).toBe(200);

    const json = await response.json() as { token: string };
    expect(json.token).toBe(MOCK_AUTH_TOKEN);
  });

  it("redacts RPC URLs from balance error details", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockRejectedValue(
      new Error("HTTP status server error (503 Service Unavailable) for url (https://devnet.helius-rpc.com/?api-key=secret-value)"),
    );

    const response = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      env,
    );

    expect(response.status).toBe(502);

    const json = await response.json() as {
      error: {
        details?: {
          message?: string;
        };
      };
    };

    expect(json.error.details?.message).toContain("[redacted-url]");
    expect(json.error.details?.message).not.toContain("https://devnet.helius-rpc.com/");
    expect(json.error.details?.message).not.toContain("api-key=secret-value");
  });

  it("returns a clearer validation error when required balance query params are missing", async () => {
    const response = await app.request("/v1/spl/balance", {}, env);

    expect(response.status).toBe(422);

    const json = await response.json() as {
      error: {
        code: string;
        message: string;
        issues: Array<{
          path: string[];
          message: string;
        }>;
      };
    };

    expect(json.error.code).toBe("VALIDATION_ERROR");
    expect(json.error.message).toBe("Missing required fields: address, mint");
    expect(json.error.issues).toEqual(expect.arrayContaining([
      expect.objectContaining({
        path: ["address"],
        message: "address is required",
      }),
      expect.objectContaining({
        path: ["mint"],
        message: "mint is required",
      }),
    ]));
  });

  it("returns initialized=true when the mint transfer queue exists", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));
    const fetchSpy = vi.spyOn(globalThis, "fetch");

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }, address) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      expect(endpoint).toBe(env.BASE_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return {
        data: Buffer.alloc(64),
        executable: false,
        lamports: 1,
        owner: DELEGATION_PROGRAM_ID,
        rentEpoch: 0,
      };
    });

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      {},
      env,
    );

    expect(response.status).toBe(200);
    expect(fetchSpy).not.toHaveBeenCalled();

    const json = await response.json() as {
      mint: string;
      validator: string;
      transferQueue: string;
      initialized: boolean;
    };

    expect(json).toEqual({
      mint,
      validator,
      transferQueue: transferQueue.toBase58(),
      initialized: true,
    });
  });

  it("returns initialized=false when the mint transfer queue is missing", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }, address) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      expect(endpoint).toBe(env.BASE_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return null;
    });

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = await response.json() as {
      initialized: boolean;
    };

    expect(json.initialized).toBe(false);
  });

  it("defaults the validator when checking mint initialization", async () => {
    const mintInitializationEnv = {
      ...env,
      BASE_RPC_URL: "https://base.mint-init.rpc.test",
    };
    const mint = "So11111111111111111111111111111111111111112";
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(resolvedValidator));

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(mintInitializationEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }, address) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      expect(endpoint).toBe(mintInitializationEnv.BASE_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return {
        data: Buffer.alloc(64),
        executable: false,
        lamports: 1,
        owner: DELEGATION_PROGRAM_ID,
        rentEpoch: 0,
      };
    });

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}`,
      {},
      mintInitializationEnv,
    );

    expect(response.status).toBe(200);

    const json = await response.json() as {
      validator: string;
      transferQueue: string;
      initialized: boolean;
    };

    expect(json.validator).toBe(resolvedValidator);
    expect(json.transferQueue).toBe(transferQueue.toBase58());
    expect(json.initialized).toBe(true);
  });

  it("returns initialized=false when the mint transfer queue exists but is not delegated yet", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }, address) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      expect(endpoint).toBe(env.BASE_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return {
        data: Buffer.alloc(64),
        executable: false,
        lamports: 1,
        owner: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
        rentEpoch: 0,
      };
    });

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = await response.json() as {
      initialized: boolean;
    };

    expect(json.initialized).toBe(false);
  });

  it("skips the background crank when recent transfer queue activity exists", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    const getLatestBlockhashSpy = vi.spyOn(Connection.prototype, "getLatestBlockhash");
    const sendRawTransactionSpy = vi.spyOn(Connection.prototype, "sendRawTransaction");

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createQueueAccountInfo(DELEGATION_PROGRAM_ID));
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockImplementation(async function getSignaturesForAddress(this: Connection & { _rpcEndpoint: string }, address) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(env.EPHEMERAL_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return [{
        blockTime: Math.floor((nowMs - 30_000) / 1000),
        confirmationStatus: "confirmed",
        err: null,
        memo: null,
        signature: "recent-signature",
        slot: 1,
      }];
    });

    const response = await app.fetch(
      new Request(`http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(getLatestBlockhashSpy).not.toHaveBeenCalled();
    expect(sendRawTransactionSpy).not.toHaveBeenCalled();
  });

  it("starts the background crank when the transfer queue has no recent transactions", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const validatorPublicKey = new PublicKey(validator);
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), validatorPublicKey);
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    let rawTransaction: Buffer | undefined;

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createQueueAccountInfo(DELEGATION_PROGRAM_ID));
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockResolvedValue([]);
    vi.spyOn(Connection.prototype, "getLatestBlockhashAndContext").mockResolvedValue({
      context: { slot: 1 },
      value: {
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123,
      },
    });
    vi.spyOn(Connection.prototype, "getEpochInfo").mockResolvedValue({
      absoluteSlot: 1,
      blockHeight: 1,
      epoch: 1,
      slotIndex: 1,
      slotsInEpoch: 1,
      transactionCount: 1,
    });
    vi.spyOn(Connection.prototype, "sendRawTransaction").mockImplementation(async function sendRawTransaction(this: Connection & { _rpcEndpoint: string }, raw) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(env.EPHEMERAL_RPC_URL);
      rawTransaction = Buffer.from(raw);
      return "background-crank-signature";
    });
    vi.spyOn(Connection.prototype, "confirmTransaction").mockResolvedValue({
      context: { slot: 1 },
      value: { err: null },
    } as never);

    const response = await app.fetch(
      new Request(`http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(rawTransaction).toBeDefined();

    const transaction = Transaction.from(rawTransaction!);
    expect(transaction.instructions).toHaveLength(1);
    expect(transaction.instructions[0]?.keys[1]?.pubkey.toBase58()).toBe(transferQueue.toBase58());
    expect(transaction.instructions[0]?.keys[2]?.pubkey.toBase58()).toBe(
      magicFeeVaultPdaFromValidator(validatorPublicKey).toBase58(),
    );
  });

  it("starts the background crank when the latest transfer queue transaction is older than a minute", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    const sendRawTransactionSpy = vi.spyOn(Connection.prototype, "sendRawTransaction").mockResolvedValue("background-crank-signature");

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createQueueAccountInfo(DELEGATION_PROGRAM_ID));
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockImplementation(async function getSignaturesForAddress(this: Connection & { _rpcEndpoint: string }, address) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(env.EPHEMERAL_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return [{
        blockTime: Math.floor((nowMs - 61_000) / 1000),
        confirmationStatus: "confirmed",
        err: null,
        memo: null,
        signature: "stale-signature",
        slot: 1,
      }];
    });
    vi.spyOn(Connection.prototype, "getLatestBlockhashAndContext").mockResolvedValue({
      context: { slot: 1 },
      value: {
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123,
      },
    });
    vi.spyOn(Connection.prototype, "getEpochInfo").mockResolvedValue({
      absoluteSlot: 1,
      blockHeight: 1,
      epoch: 1,
      slotIndex: 1,
      slotsInEpoch: 1,
      transactionCount: 1,
    });
    vi.spyOn(Connection.prototype, "confirmTransaction").mockResolvedValue({
      context: { slot: 1 },
      value: { err: null },
    } as never);

    const response = await app.fetch(
      new Request(`http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(sendRawTransactionSpy).toHaveBeenCalledOnce();
  });

  it("uses a custom RPC URL only for base RPC calls when cluster is a URL", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      return endpoint.includes("custom.rpc.test")
        ? createAccountInfo(7n)
        : endpoint.includes("ephemeral")
          ? createAccountInfo(9n)
          : createAccountInfo(0n);
    });

    const baseResponse = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112&cluster=${encodeURIComponent("https://custom.rpc.test")}`,
      {},
      env,
    );
    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=So11111111111111111111111111111111111111112&cluster=${encodeURIComponent("https://custom.rpc.test")}`,
      { headers: { authorization: "Bearer 1234567890" } },
      env,
    );

    expect(baseResponse.status).toBe(200);
    expect(privateResponse.status).toBe(200);

    const baseJson = await baseResponse.json() as { location: string; balance: string };
    const privateJson = await privateResponse.json() as { location: string; balance: string };

    expect(baseJson.location).toBe("base");
    expect(baseJson.balance).toBe("7");
    expect(privateJson.location).toBe("ephemeral");
    expect(privateJson.balance).toBe("9");
  });
});
