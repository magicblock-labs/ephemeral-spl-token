import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import {
  DELEGATION_PROGRAM_ID,
  delegateBufferPdaFromDelegatedAccountAndOwnerProgram,
  delegationMetadataPdaFromDelegatedAccount,
  delegationRecordPdaFromDelegatedAccount,
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
  PERMISSION_PROGRAM_ID,
  permissionPdaFromAccount,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import { sha256 } from "@noble/hashes/sha256";
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
import {
  ASSOCIATED_TOKEN_PROGRAM_ID,
  deriveStealthPoolFromHandle,
  TOKEN_2022_PROGRAM_ID,
  TOKEN_PROGRAM_ID,
} from "./lib/solana";
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
const stealthHandle = "john.doe@magicblock.id";
const stealthPool = deriveStealthPoolFromHandle(stealthHandle)[0].toBase58();

function createStealthHandleStorage(handle: string) {
  const handleBytes = Buffer.from(handle, "utf8");
  const storage = Buffer.alloc(256);
  storage[0] = handleBytes.length;
  storage.set(handleBytes, 1);
  return storage;
}

function deriveAssociatedTokenAddress(
  mint: string,
  owner: string,
  tokenProgram = TOKEN_PROGRAM_ID,
) {
  const [ata] = PublicKey.findProgramAddressSync(
    [
      new PublicKey(owner).toBuffer(),
      tokenProgram.toBuffer(),
      new PublicKey(mint).toBuffer(),
    ],
    new PublicKey("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL"),
  );

  return ata.toBase58();
}

function createMcpFetch() {
  return (input: RequestInfo | URL, init?: RequestInit) => {
    const request
      = input instanceof Request
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

function createAccountInfo(
  amount: bigint,
  tokenProgram = TOKEN_PROGRAM_ID,
): AccountInfo<Buffer> {
  return {
    data: createTokenAccountData(amount),
    executable: false,
    lamports: 0,
    owner: tokenProgram,
    rentEpoch: 0,
  };
}

function createMintAccountInfo(tokenProgram: PublicKey): AccountInfo<Buffer> {
  return {
    data: Buffer.alloc(82),
    executable: false,
    lamports: 1,
    owner: tokenProgram,
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

function createDelegationAccountInfo(validator: PublicKey): AccountInfo<Buffer> {
  const data = Buffer.alloc(40);
  validator.toBuffer().copy(data, 8);
  return {
    data,
    executable: false,
    lamports: 1,
    owner: DELEGATION_PROGRAM_ID,
    rentEpoch: 0,
  };
}

function createStealthPoolAccountInfo(): AccountInfo<Buffer> {
  const data = Buffer.alloc(428);
  data.set(Buffer.from("stpool@1"), 0);
  return {
    data,
    executable: false,
    lamports: 1,
    owner: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    rentEpoch: 0,
  };
}

function deriveEataDelegationRecord(owner: string, mint: string) {
  const [eata] = deriveEphemeralAta(new PublicKey(owner), new PublicKey(mint));
  return delegationRecordPdaFromDelegatedAccount(eata);
}
function createIdentityResponse(identity: string) {
  return new Response(
    JSON.stringify({
      result: {
        identity,
      },
    }),
    {
      status: 200,
      headers: {
        "content-type": "application/json",
      },
    },
  );
}

function createLookupTableResponse(
  value: AddressLookupTableAccount | null,
): Awaited<ReturnType<Connection["getAddressLookupTable"]>> {
  return {
    context: {
      slot: 0,
    },
    value,
  };
}

function createLookupTableAccount(
  addresses: PublicKey[],
  key = Keypair.generate().publicKey,
) {
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
  beforeEach(() => {
    vi.spyOn(Connection.prototype, "getMultipleAccountsInfo").mockImplementation(
      async addresses => addresses.map(() => null),
    );
  });

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

    const json = (await response.json()) as any;
    expect(json.paths["/v1/spl/deposit"]).toBeDefined();
    expect(json.paths["/mcp"]?.post).toBeDefined();
    expect(json.paths["/mcp"]?.get).toBeUndefined();
    expect(json.paths["/.well-known/mcp.json"]).toBeUndefined();
    expect(
      json.paths["/mcp"]?.post?.requestBody?.content?.["application/json"]
        ?.schema,
    ).toBeDefined();
    expect(json.paths["/v1/spl/private-balance"]).toBeDefined();
    expect(json.paths["/v1/spl/challenge"]).toBeDefined();
    expect(json.paths["/v1/spl/login"]).toBeDefined();
    expect(json.paths["/v1/spl/is-mint-initialized"]).toBeDefined();
    expect(json.paths["/v1/spl/initialize-mint"]).toBeDefined();
    expect(json.paths["/v1/spl/transfer-queue/ensure-crank"]).toBeDefined();
    expect(json.paths["/v1/spl/undelegate-ephemeral-ata"]).toBeDefined();
    expect(json.paths["/v1/spl/stealth-pool"]).toBeDefined();
    expect(json.paths["/v1/spl/transfer-stealth"]).toBeUndefined();
    expect(json.paths["/v1/swap/quote"]).toBeDefined();
    expect(json.paths["/v1/swap/swap"]).toBeDefined();
    expect(json.tags).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          name: "Swap",
          description: "Provide quoting and execution for public and private swaps.",
        }),
      ]),
    );
    expect(json.paths["/v1/transaction/send"]).toBeDefined();
    expect(json.paths["/v1/swap/swap-instructions"]).toBeUndefined();
    expect(json.paths["/v1/swap/program-id-to-label"]).toBeUndefined();
    expect(json.paths["/v1/swap/quote"]?.get?.parameters).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ name: "inputMint", required: true }),
        expect.objectContaining({ name: "outputMint", required: true }),
        expect.objectContaining({ name: "amount", required: true }),
        expect.objectContaining({ name: "slippageBps" }),
        expect.objectContaining({ name: "swapMode" }),
      ]),
    );
    expect(
      json.paths["/v1/swap/swap"]?.post?.requestBody?.content?.[
        "application/json"
      ]?.schema,
    ).toBeDefined();
    expect(
      json.paths["/v1/swap/quote"]?.get?.responses?.["200"]?.content?.[
        "application/json"
      ]?.example,
    ).toMatchObject({
      inputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
      outputMint: "Es9vMFrzaCERmJfrF4H2FYD4KCoNkY11McCe8BenwNYB",
      inAmount: "1000000",
      outAmount: "999519",
    });
    expect(
      json.paths["/v1/swap/swap"]?.post?.requestBody?.content?.[
        "application/json"
      ]?.examples?.public?.value,
    ).toMatchObject({
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
    const swapRequestSchema = (json.components?.schemas as Record<string, any>)
      ?.SwapRequest;
    expect(swapRequestSchema?.properties?.visibility).toBeDefined();
    expect(swapRequestSchema?.properties?.destination).toBeDefined();
    expect(swapRequestSchema?.properties?.minDelayMs).toBeDefined();
    expect(swapRequestSchema?.properties?.maxDelayMs).toBeDefined();
    expect(swapRequestSchema?.properties?.split).toBeDefined();
    expect(swapRequestSchema?.properties?.clientRefId).toBeDefined();
    expect(swapRequestSchema?.properties?.validator).toBeDefined();

    // Both request examples (public + private) are surfaced.
    const swapRequestExamples
      = json.paths["/v1/swap/swap"]?.post?.requestBody?.content?.[
        "application/json"
      ]?.examples;
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
    const swapResponseSchema = (json.components?.schemas as Record<string, any>)
      ?.SwapResponse;
    expect(swapResponseSchema?.properties?.privateTransfer).toBeDefined();
    const swapResponseExamples
      = json.paths["/v1/swap/swap"]?.post?.responses?.["200"]?.content?.[
        "application/json"
      ]?.examples;
    expect(swapResponseExamples?.private?.value?.privateTransfer).toMatchObject(
      {
        stashAta: expect.any(String),
        hydraCrankPda: expect.any(String),
        shuttleId: expect.any(Number),
      },
    );
    expect(
      json.paths["/v1/spl/deposit"]?.post?.responses?.["200"]?.content?.[
        "application/json"
      ]?.example,
    ).toMatchObject({
      kind: "deposit",
      instructionCount: 3,
    });
    const depositRequestSchema = (
      json.components?.schemas as Record<string, any>
    )?.DepositRequest;
    expect(depositRequestSchema?.properties?.private).toMatchObject({
      type: "boolean",
      example: true,
    });
    const transferRequestSchema = (
      json.components?.schemas as Record<string, any>
    )?.TransferRequest;
    expect(transferRequestSchema?.properties?.gasless).toMatchObject({
      type: "boolean",
      example: true,
    });
    expect(transferRequestSchema?.properties?.to?.description).toContain(
      "stealth handle",
    );
    expect(transferRequestSchema?.example).toMatchObject({
      amount: 5000000,
      gasless: true,
    });
    expect(transferRequestSchema?.example).not.toHaveProperty("initIfMissing");
    expect(transferRequestSchema?.example).not.toHaveProperty(
      "initAtasIfMissing",
    );
    expect(transferRequestSchema?.example).not.toHaveProperty(
      "initVaultIfMissing",
    );
    expect(transferRequestSchema?.example).not.toHaveProperty("split");
    const transactionResponseSchema = (
      json.components?.schemas as Record<string, any>
    )?.UnsignedTransactionResponse;
    expect(transactionResponseSchema?.properties?.fees).toBeDefined();
    expect(transactionResponseSchema?.properties?.sendRpcEndpoint).toBeDefined();
    const sendTransactionRequestSchema = (
      json.components?.schemas as Record<string, any>
    )?.SendTransactionRequest;
    expect(sendTransactionRequestSchema?.properties?.sendRpcEndpoint).toBeDefined();
    expect(
      (json.components?.schemas as Record<string, any>)?.StealthTransferRequest,
    ).toBeUndefined();
    expect(
      json.paths["/v1/spl/withdraw"]?.post?.responses?.["200"]?.content?.[
        "application/json"
      ]?.example,
    ).toMatchObject({
      kind: "withdraw",
      instructionCount: 2,
    });
  });

  it("sends a signed transaction to the selected base RPC", async () => {
    const transaction = Buffer.from([1, 2, 3, 4]);
    const sendRawTransactionSpy = vi
      .spyOn(Connection.prototype, "sendRawTransaction")
      .mockImplementation(async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
        raw,
        options,
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          env.BASE_DEVNET_RPC_URL,
        );
        expect(Buffer.from(raw as Uint8Array)).toEqual(transaction);
        expect(options?.preflightCommitment).toBe("confirmed");
        expect(options?.skipPreflight).toBe(true);
        expect(options?.maxRetries).toBe(2);
        return "base-signature";
      });

    const response = await app.request(
      "/v1/transaction/send",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          transactionBase64: transaction.toString("base64"),
          sendTo: "base",
          cluster: "devnet",
          skipPreflight: true,
          maxRetries: 2,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);
    expect(sendRawTransactionSpy).toHaveBeenCalledOnce();

    const json = (await response.json()) as {
      signature: string;
      sendTo: string;
      confirmed: boolean;
      confirmationRpcEndpoint: string;
      confirmationRequiresAuthToken: boolean;
    };

    expect(json).toEqual({
      signature: "base-signature",
      sendTo: "base",
      confirmed: false,
      confirmationRpcEndpoint: env.BASE_DEVNET_RPC_URL,
      confirmationRequiresAuthToken: false,
    });
  });

  it("sends a signed transaction to the ephemeral RPC with the auth token", async () => {
    const transaction = Buffer.from([5, 6, 7, 8]);

    vi.spyOn(Connection.prototype, "sendRawTransaction").mockImplementation(
      async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
        raw,
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          `${env.EPHEMERAL_DEVNET_RPC_URL}/?token=private-token`,
        );
        expect(Buffer.from(raw as Uint8Array)).toEqual(transaction);
        return "ephemeral-signature";
      },
    );

    const response = await app.request(
      "/v1/transaction/send",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "Authorization": "Bearer private-token",
        },
        body: JSON.stringify({
          transactionBase64: transaction.toString("base64"),
          sendTo: "ephemeral",
          cluster: "devnet",
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      signature: string;
      sendTo: string;
      confirmed: boolean;
      confirmationRpcEndpoint: string;
      confirmationRequiresAuthToken: boolean;
    };

    expect(json).toEqual({
      signature: "ephemeral-signature",
      sendTo: "ephemeral",
      confirmed: false,
      confirmationRpcEndpoint: env.EPHEMERAL_DEVNET_RPC_URL,
      confirmationRequiresAuthToken: true,
    });
  });

  it("sends and confirms a signed ephemeral transaction to the provided RPC endpoint", async () => {
    const transaction = Buffer.from([5, 6, 7, 8]);
    const sendRpcEndpoint = "https://devnet-tee.magicblock.app";
    const blockhash = "11111111111111111111111111111111";

    vi.spyOn(Connection.prototype, "sendRawTransaction").mockImplementation(
      async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
        raw,
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          `${sendRpcEndpoint}/?token=private-token`,
        );
        expect(Buffer.from(raw as Uint8Array)).toEqual(transaction);
        return "endpoint-signature";
      },
    );
    vi.spyOn(Connection.prototype, "confirmTransaction").mockImplementation(
      async function confirmTransaction(this: Connection & { _rpcEndpoint: string }) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          `${sendRpcEndpoint}/?token=private-token`,
        );
        return {
          context: { slot: 1 },
          value: { err: null },
        };
      },
    );

    const response = await app.request(
      "/v1/transaction/send",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "Authorization": "Bearer private-token",
        },
        body: JSON.stringify({
          transactionBase64: transaction.toString("base64"),
          sendTo: "ephemeral",
          sendRpcEndpoint,
          cluster: "devnet",
          confirm: true,
          recentBlockhash: blockhash,
          lastValidBlockHeight: 123,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      signature: string;
      sendTo: string;
      confirmed: boolean;
      confirmationRpcEndpoint: string;
      confirmationRequiresAuthToken: boolean;
    };

    expect(json).toEqual({
      signature: "endpoint-signature",
      sendTo: "ephemeral",
      confirmed: true,
      confirmationRpcEndpoint: sendRpcEndpoint,
      confirmationRequiresAuthToken: true,
    });
  });

  it("rejects a send RPC endpoint override for base transactions", async () => {
    const sendRawTransactionSpy = vi.spyOn(
      Connection.prototype,
      "sendRawTransaction",
    );

    const response = await app.request(
      "/v1/transaction/send",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          transactionBase64: Buffer.from([1, 2, 3]).toString("base64"),
          sendTo: "base",
          sendRpcEndpoint: "https://devnet-tee.magicblock.app",
        }),
      },
      env,
    );

    expect(response.status).toBe(400);
    expect(sendRawTransactionSpy).not.toHaveBeenCalled();

    const json = (await response.json()) as {
      error: {
        code: string;
      };
    };
    expect(json.error.code).toBe("INVALID_SEND_RPC_ENDPOINT");
  });

  it("confirms a sent transaction when requested", async () => {
    const transaction = Buffer.from([9, 10, 11, 12]);
    const blockhash = "11111111111111111111111111111111";
    const confirmTransactionSpy = vi
      .spyOn(Connection.prototype, "confirmTransaction")
      .mockResolvedValue({
        context: { slot: 1 },
        value: { err: null },
      } as never);

    vi.spyOn(Connection.prototype, "sendRawTransaction").mockResolvedValue(
      "confirmed-signature",
    );

    const response = await app.request(
      "/v1/transaction/send",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          transactionBase64: transaction.toString("base64"),
          sendTo: "base",
          confirm: true,
          recentBlockhash: blockhash,
          lastValidBlockHeight: 123,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);
    expect(confirmTransactionSpy).toHaveBeenCalledWith({
      signature: "confirmed-signature",
      blockhash,
      lastValidBlockHeight: 123,
    }, "confirmed");

    const json = (await response.json()) as {
      confirmed: boolean;
    };

    expect(json.confirmed).toBe(true);
  });

  it("requires confirmation fields only when confirmation is requested", async () => {
    const sendRawTransactionSpy = vi.spyOn(
      Connection.prototype,
      "sendRawTransaction",
    );

    const response = await app.request(
      "/v1/transaction/send",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          transactionBase64: Buffer.from([1, 2, 3]).toString("base64"),
          sendTo: "base",
          confirm: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(400);
    expect(sendRawTransactionSpy).not.toHaveBeenCalled();

    const json = (await response.json()) as {
      error: {
        code: string;
      };
    };

    expect(json.error.code).toBe("MISSING_CONFIRMATION_FIELDS");
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
      return new Response(
        JSON.stringify({
          inputMint: "So11111111111111111111111111111111111111112",
          outputMint: "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
          outAmount: "999000",
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    });

    const response = await app.request(
      "/v1/swap/quote?inputMint=So11111111111111111111111111111111111111112&outputMint=EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v&amount=1000000&slippageBps=50",
      {},
      metisEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
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
      routePlan: [
        {
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
        },
      ],
    };

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      expect(String(input)).toBe(
        "https://triton.rpc.test/private-token/metis/swap",
      );
      expect(init?.method).toBe("POST");
      expect(init?.headers).toBeInstanceOf(Headers);
      expect((init?.headers as Headers).get("content-type")).toBe(
        "application/json",
      );
      const rawBody = init?.body;
      const decodedBody
        = typeof rawBody === "string"
          ? rawBody
          : new TextDecoder().decode(rawBody as ArrayBuffer);
      expect(JSON.parse(decodedBody)).toMatchObject({
        userPublicKey: owner,
        quoteResponse,
      });
      return new Response(
        JSON.stringify({
          swapTransaction: "base64-tx",
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    });

    const response = await app.request(
      "/v1/swap/swap",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          userPublicKey: owner,
          quoteResponse,
        }),
      },
      metisEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
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

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
      };
    };

    expect(json.error.code).toBe("CONFIG_ERROR");
    expect(json.error.message).toBe(
      "Missing worker environment variable `METIS_SWAP_API_URL`",
    );
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
      routePlan: [
        {
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
        },
      ],
    };

    const ownerPk = new PublicKey(owner);
    const stashPda = deriveStashPda(ownerPk, new PublicKey(outputMint))[0];
    const [stashAtaExpected] = deriveStashAta(
      ownerPk,
      new PublicKey(outputMint),
    );

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
      return new Response("{}", {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      swapTransaction: string;
      privateTransfer: {
        stashAta: string;
        hydraCrankPda: string;
        shuttleId: number;
      };
    };

    // Metis received the forced destinationTokenAccount + forced v0.
    expect(metisRequestBody?.destinationTokenAccount).toBe(
      stashAtaExpected.toBase58(),
    );
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
    const [hydraCrankExpected] = deriveHydraCrankPda(
      stashPda,
      json.privateTransfer.shuttleId,
    );
    expect(json.privateTransfer.hydraCrankPda).toBe(
      hydraCrankExpected.toBase58(),
    );

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
      decompiled.instructions.some(ix =>
        ix.programId.equals(computeBudgetProgram),
      ),
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
    expect(scheduleIx.keys[4].pubkey.toBase58()).toBe(
      HYDRA_PROGRAM_ID.toBase58(),
    );
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
      return new Response("{}", {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(200);
    expect(metisRequestBody?.payer).toBe(sponsor.toBase58());

    const json = (await response.json()) as { swapTransaction: string };
    const returned = VersionedTransaction.deserialize(
      Buffer.from(json.swapTransaction, "base64"),
    );
    const decompiled = TransactionMessage.decompile(returned.message, {
      addressLookupTableAccounts: [],
    });

    const [createIx, , scheduleIx] = decompiled.instructions;
    expect(returned.message.staticAccountKeys[0]?.toBase58()).toBe(
      sponsor.toBase58(),
    );
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
      return new Response("{}", {
        status: 200,
        headers: { "content-type": "application/json" },
      });
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

    const firstResponse = await app.request(
      "/v1/swap/swap",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      },
      metisEnv,
    );
    const secondResponse = await app.request(
      "/v1/swap/swap",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify(body),
      },
      metisEnv,
    );

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
      return new Response("{}", {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(200);
    const json = (await response.json()) as { swapTransaction: string };

    const returned = VersionedTransaction.deserialize(
      Buffer.from(json.swapTransaction, "base64"),
    );
    const decompiled = TransactionMessage.decompile(returned.message, {
      addressLookupTableAccounts: [],
    });

    // Same 4 ixs (no new SetComputeUnitLimit prepended), but the existing
    // one has been rewritten with a bumped value.
    expect(decompiled.instructions).toHaveLength(4);
    const cbCount = decompiled.instructions.filter(ix =>
      ix.programId.equals(computeBudgetProgram),
    ).length;
    expect(cbCount).toBe(1);

    const cbIx = decompiled.instructions.find(ix =>
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

    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ lastValidBlockHeight: 123 }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(502);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("SWAP_UPSTREAM_ERROR");
    expect(json.error.message).toBe(
      "Upstream swap response missing swapTransaction",
    );
  });

  it("visibility=private returns 502 when the upstream swap transaction is invalid", async () => {
    const metisEnv = {
      ...env,
      METIS_SWAP_API_URL: "https://triton.rpc.test/private-token/metis",
    };

    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(JSON.stringify({ swapTransaction: "%%%%" }), {
        status: 200,
        headers: { "content-type": "application/json" },
      }),
    );

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(502);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("SWAP_UPSTREAM_ERROR");
    expect(json.error.message).toBe(
      "Invalid upstream swap transaction encoding",
    );
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
    vi.spyOn(VersionedTransaction.prototype, "serialize").mockImplementation(
      function (this: VersionedTransaction) {
        serializeCalls += 1;
        if (serializeCalls <= 2) {
          return new Uint8Array(1233);
        }
        return originalSerialize.call(this);
      },
    );

    const quoteMaxAccounts: string[] = [];
    let swapCalls = 0;
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      const url = new URL(String(input));

      if (
        url.origin === "https://triton.rpc.test"
        && url.pathname.endsWith("/quote")
      ) {
        quoteMaxAccounts.push(url.searchParams.get("maxAccounts") ?? "");
        return new Response(JSON.stringify(quoteResponse), {
          status: 200,
          headers: { "content-type": "application/json" },
        });
      }

      if (
        url.origin === "https://triton.rpc.test"
        && url.pathname.endsWith("/swap")
      ) {
        swapCalls += 1;
        return new Response(
          JSON.stringify({ swapTransaction: jupiterBase64 }),
          {
            status: 200,
            headers: { "content-type": "application/json" },
          },
        );
      }

      return new Response("{}", {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(200);
    expect(swapCalls).toBe(3);
    expect(quoteMaxAccounts).toEqual(["39", "38"]);

    const json = (await response.json()) as { swapTransaction: string };
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

    const response = await app.request(
      "/v1/swap/swap",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({
          userPublicKey: owner,
          quoteResponse,
          visibility: "private",
          // destination + delays + split intentionally missing
        }),
      },
      metisEnv,
    );

    expect(response.status).toBe(400);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
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

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(400);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toBe(
      "maxDelayMs must be less than or equal to 600000",
    );
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

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(400);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toBe(
      "split must be an integer between 1 and 14 when visibility=private",
    );
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

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(400);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toMatch(
      /destinationTokenAccount is not supported/,
    );
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

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(400);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
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

    const response = await app.request(
      "/v1/swap/swap",
      {
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
      },
      metisEnv,
    );

    expect(response.status).toBe(400);
    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("INVALID_REQUEST");
    expect(json.error.message).toBe(
      "nativeDestinationAccount is not supported when visibility=private",
    );
  });

  it("serves MCP info and discovery documents", async () => {
    const mcpResponse = await app.request("/mcp", {}, env);
    const discoveryResponse = await app.request(
      "/.well-known/mcp.json",
      {},
      env,
    );

    expect(mcpResponse.status).toBe(200);
    expect(discoveryResponse.status).toBe(200);

    const mcpJson = (await mcpResponse.json()) as {
      endpoint: string;
      discovery: string;
      tools: Array<{ name: string }>;
    };
    const discoveryJson = (await discoveryResponse.json()) as {
      transport: { endpoint: string; type: string };
      tools: Array<{ name: string }>;
    };

    expect(mcpJson.endpoint).toBe("http://localhost/mcp");
    expect(mcpJson.discovery).toBe("http://localhost/.well-known/mcp.json");
    expect(mcpJson.tools.some(tool => tool.name === "spl.transfer")).toBe(
      true,
    );

    expect(discoveryJson.transport.type).toBe("streamable-http");
    expect(discoveryJson.transport.endpoint).toBe("http://localhost/mcp");
    expect(
      discoveryJson.tools.some(tool => tool.name === "spl.getPrivateBalance"),
    ).toBe(true);
  });

  it("accepts MCP initialize requests from doc clients that do not send a JSON content type", async () => {
    const response = await app.request(
      "/mcp",
      {
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
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
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

    const response = await app.request(
      "/v1/spl/deposit",
      {
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
      },
      depositEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      sendTo: string;
      transactionBase64: string;
      recentBlockhash: string;
      validator: string;
    };

    expect(json.sendTo).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(transaction.instructions.length).toBeGreaterThan(0);
    const privatePermissionIx = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 6,
    );
    const depositIx
      = transaction.instructions[transaction.instructions.length - 1]!;
    expect(privatePermissionIx).toBeDefined();
    expect(depositIx.data[0]).toBe(24);
    expect(depositIx.data.length).toBe(45);
  });

  it("wraps native SOL before a SOL deposit when the WSOL balance is short", async () => {
    const depositEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.sol-deposit.rpc.test",
    };
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const sourceAta = new PublicKey(deriveAssociatedTokenAddress(mint.toBase58(), owner));
    const amount = 100_000_000;
    const wrappedBalance = 25_000_000n;

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getBalance").mockResolvedValue(20_000_000);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async (address) => {
        if (address.equals(mint)) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }

        if (address.equals(sourceAta)) {
          return createAccountInfo(wrappedBalance);
        }

        return null;
      },
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(depositEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount,
          idempotent: true,
          initIfMissing: true,
          initAtasIfMissing: true,
          initVaultIfMissing: true,
        }),
      },
      depositEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const createAtaIx = transaction.instructions[0]!;
    const transferIx = transaction.instructions[1]!;
    const syncNativeIx = transaction.instructions[2]!;
    const decodedTransfer = SystemInstruction.decodeTransfer(transferIx);

    expect(createAtaIx.programId.equals(ASSOCIATED_TOKEN_PROGRAM_ID)).toBe(true);
    expect(createAtaIx.keys[1]?.pubkey.toBase58()).toBe(sourceAta.toBase58());
    expect(decodedTransfer.fromPubkey.toBase58()).toBe(owner);
    expect(decodedTransfer.toPubkey.toBase58()).toBe(sourceAta.toBase58());
    expect(BigInt(decodedTransfer.lamports)).toBe(BigInt(amount) - wrappedBalance);
    expect(syncNativeIx.programId.equals(TOKEN_PROGRAM_ID)).toBe(true);
    expect(syncNativeIx.keys[0]?.pubkey.toBase58()).toBe(sourceAta.toBase58());
    expect(syncNativeIx.data[0]).toBe(17);
  });

  it("does not wrap native SOL before a SOL deposit when WSOL is sufficient", async () => {
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const sourceAta = new PublicKey(deriveAssociatedTokenAddress(mint.toBase58(), owner));
    const amount = 100_000_000;

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getBalance").mockResolvedValue(20_000_000);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async (address) => {
        if (address.equals(mint)) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }

        if (address.equals(sourceAta)) {
          return createAccountInfo(BigInt(amount));
        }

        return null;
      },
    );

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount,
          validator: resolvedValidator,
          idempotent: true,
          initIfMissing: true,
          initAtasIfMissing: true,
          initVaultIfMissing: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const syncNativeIx = transaction.instructions.find(
      ix => ix.programId.equals(TOKEN_PROGRAM_ID)
        && ix.keys[0]?.pubkey.equals(sourceAta)
        && ix.data[0] === 17,
    );
    const systemTransferIx = transaction.instructions.find(
      ix => ix.programId.equals(SystemProgram.programId)
        && ix.keys[1]?.pubkey.equals(sourceAta),
    );

    expect(syncNativeIx).toBeUndefined();
    expect(systemTransferIx).toBeUndefined();
  });

  it("tops up the rent PDA before a SOL deposit when the rent PDA is short", async () => {
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const sourceAta = new PublicKey(deriveAssociatedTokenAddress(mint.toBase58(), owner));
    const [rentPda] = deriveRentPda();
    const amount = 100_000_000;

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getBalance").mockResolvedValue(500_000);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async (address) => {
        if (address.equals(mint)) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }

        if (address.equals(sourceAta)) {
          return createAccountInfo(BigInt(amount));
        }

        return null;
      },
    );

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount,
          validator: resolvedValidator,
          idempotent: true,
          initIfMissing: true,
          initAtasIfMissing: true,
          initVaultIfMissing: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const rentTopUpIx = transaction.instructions[0]!;
    const decodedTransfer = SystemInstruction.decodeTransfer(rentTopUpIx);

    expect(decodedTransfer.fromPubkey.toBase58()).toBe(owner);
    expect(decodedTransfer.toPubkey.toBase58()).toBe(rentPda.toBase58());
    expect(BigInt(decodedTransfer.lamports)).toBe(19_500_000n);
  });

  it("builds a public deposit transaction when private is false", async () => {
    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
          validator: resolvedValidator,
          idempotent: true,
          private: false,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const privatePermissionIx = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 6,
    );

    expect(privatePermissionIx).toBeUndefined();
  });

  it("uses the mint token program when building a deposit", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");
    const validator = new PublicKey(resolvedValidator);
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault, TOKEN_2022_PROGRAM_ID);

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_2022_PROGRAM_ID),
    );

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount: 1,
          validator: validator.toBase58(),
          idempotent: true,
          initIfMissing: true,
          initAtasIfMissing: true,
          initVaultIfMissing: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const initVaultInstruction = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 1,
    );
    const depositInstruction = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 24,
    );

    expect(initVaultInstruction?.keys[4].pubkey.toBase58()).toBe(
      vaultAta.toBase58(),
    );
    expect(initVaultInstruction?.keys[5].pubkey.toBase58()).toBe(
      TOKEN_2022_PROGRAM_ID.toBase58(),
    );
    expect(depositInstruction?.keys[15].pubkey.toBase58()).toBe(
      TOKEN_2022_PROGRAM_ID.toBase58(),
    );
    expect(depositInstruction?.keys[18].pubkey.toBase58()).toBe(
      vaultAta.toBase58(),
    );
  });

  it("builds an unsigned eATA undelegation transaction", async () => {
    const mint = new PublicKey(DEVNET_USDC_MINT);
    const payer = new PublicKey(owner);
    const [ephemeralAta] = deriveEphemeralAta(payer, mint);
    const ata = deriveAssociatedTokenAddress(mint.toBase58(), owner);
    const sendRpcEndpoint = "https://devnet-tee.magicblock.app";

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(this: Connection & { _rpcEndpoint: string }) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          `${sendRpcEndpoint}/?token=${MOCK_AUTH_TOKEN}`,
        );
        return {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        };
      },
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      expect(String(input)).toBe("https://devnet-router.magicblock.app/");
      const body = JSON.parse(String(init?.body)) as {
        method?: string;
        params?: string[];
      };
      expect(body.method).toBe("getDelegationStatus");
      expect(body.params).toEqual([ephemeralAta.toBase58()]);

      return new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          result: {
            isDelegated: true,
            fqdn: `${sendRpcEndpoint}/`,
          },
        }),
        {
          status: 200,
          headers: { "content-type": "application/json" },
        },
      );
    });

    const response = await app.request(
      "/v1/spl/undelegate-ephemeral-ata",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "authorization": `Bearer ${MOCK_AUTH_TOKEN}`,
        },
        body: JSON.stringify({
          payer: owner,
          mint: mint.toBase58(),
          cluster: "devnet",
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      kind: string;
      sendTo: string;
      sendRpcEndpoint: string;
      transactionBase64: string;
      recentBlockhash: string;
      requiredSigners: string[];
    };

    expect(json.kind).toBe("undelegateEphemeralAta");
    expect(json.sendTo).toBe("ephemeral");
    expect(json.sendRpcEndpoint).toBe(sendRpcEndpoint);
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.requiredSigners).toContain(owner);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(transaction.instructions).toHaveLength(1);

    const instruction = transaction.instructions[0]!;
    expect(instruction.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)).toBe(true);
    expect([...instruction.data]).toEqual([5]);
    expect(instruction.keys[0]).toEqual(expect.objectContaining({
      pubkey: payer,
      isSigner: true,
      isWritable: true,
    }));
    expect(instruction.keys[1]).toEqual(expect.objectContaining({
      pubkey: new PublicKey(ata),
      isSigner: false,
      isWritable: true,
    }));
    expect(instruction.keys[2]).toEqual(expect.objectContaining({
      pubkey: ephemeralAta,
      isSigner: false,
      isWritable: false,
    }));
  });

  it("falls back to the hardcoded TEE endpoint when router delegation status has no fqdn", async () => {
    const mint = new PublicKey(DEVNET_USDC_MINT);
    const payer = new PublicKey(owner);
    const [ephemeralAta] = deriveEphemeralAta(payer, mint);
    const delegationRecord = delegationRecordPdaFromDelegatedAccount(ephemeralAta);
    const teeValidator = new PublicKey("MTEWGuqxUpYZGFJQcp8tLN7x5v9BSeoFHYWQQ3n3xzo");
    const sendRpcEndpoint = "https://mainnet-tee.magicblock.app";
    const calls: string[] = [];

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(this: Connection & { _rpcEndpoint: string }) {
        calls.push("blockhash");
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          `${sendRpcEndpoint}/?token=${MOCK_AUTH_TOKEN}`,
        );
        return {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        };
      },
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async (address) => {
        if (address.equals(mint)) {
          calls.push("mint");
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }

        if (address.equals(delegationRecord)) {
          calls.push("delegationRecord");
          return createDelegationAccountInfo(teeValidator);
        }

        return null;
      },
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async () => {
      calls.push("router");
      return new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          result: {
            isDelegated: true,
          },
        }),
        {
          status: 200,
          headers: { "content-type": "application/json" },
        },
      );
    });

    const response = await app.request(
      "/v1/spl/undelegate-ephemeral-ata",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "authorization": `Bearer ${MOCK_AUTH_TOKEN}`,
        },
        body: JSON.stringify({
          payer: owner,
          mint: mint.toBase58(),
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      sendRpcEndpoint: string;
    };

    expect(json.sendRpcEndpoint).toBe(sendRpcEndpoint);
    expect(calls).toEqual(["mint", "router", "delegationRecord", "blockhash"]);
  });

  it("returns an error when the undelegation endpoint cannot be resolved", async () => {
    const mint = new PublicKey(DEVNET_USDC_MINT);
    const payer = new PublicKey(owner);
    const [ephemeralAta] = deriveEphemeralAta(payer, mint);
    const delegationRecord = delegationRecordPdaFromDelegatedAccount(ephemeralAta);
    const getLatestBlockhashSpy = vi.spyOn(Connection.prototype, "getLatestBlockhash");

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async (address) => {
        if (address.equals(mint)) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }

        if (address.equals(delegationRecord)) {
          return null;
        }

        return null;
      },
    );
    vi.spyOn(globalThis, "fetch").mockResolvedValue(
      new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          id: 1,
          result: {
            isDelegated: true,
          },
        }),
        {
          status: 200,
          headers: { "content-type": "application/json" },
        },
      ),
    );

    const response = await app.request(
      "/v1/spl/undelegate-ephemeral-ata",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "authorization": `Bearer ${MOCK_AUTH_TOKEN}`,
        },
        body: JSON.stringify({
          payer: owner,
          mint: mint.toBase58(),
          cluster: "devnet",
        }),
      },
      env,
    );

    expect(response.status).toBe(400);
    expect(getLatestBlockhashSpy).not.toHaveBeenCalled();

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
      };
    };
    expect(json.error.code).toBe("EPHEMERAL_ENDPOINT_UNRESOLVED");
    expect(json.error.message).toBe("Ephemeral RPC endpoint cannot be retrieved");
  });

  it("returns MINT_NOT_FOUND when a deposit mint account is missing", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockResolvedValue(null);

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount: 1,
          validator: resolvedValidator,
        }),
      },
      env,
    );

    expect(response.status).toBe(400);
    expect(getAccountInfoSpy).toHaveBeenCalledOnce();

    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("MINT_NOT_FOUND");
    expect(json.error.message).toBe("Mint account not found");
  });

  it("returns UNSUPPORTED_TOKEN_PROGRAM when a deposit mint owner is unsupported", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockResolvedValue(createMintAccountInfo(SystemProgram.programId));

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount: 1,
          validator: resolvedValidator,
        }),
      },
      env,
    );

    expect(response.status).toBe(400);
    expect(getAccountInfoSpy).toHaveBeenCalledOnce();

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
        details?: {
          mint?: string;
          owner?: string;
        };
      };
    };
    expect(json.error.code).toBe("UNSUPPORTED_TOKEN_PROGRAM");
    expect(json.error.message).toBe(
      "Mint owner is not a supported token program",
    );
    expect(json.error.details).toEqual({
      mint: mint.toBase58(),
      owner: SystemProgram.programId.toBase58(),
    });
  });

  it("returns RPC_ERROR when resolving a deposit mint token program fails", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockRejectedValue(new Error("mint lookup failed"));

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount: 1,
          validator: resolvedValidator,
        }),
      },
      env,
    );

    expect(response.status).toBe(502);
    expect(getAccountInfoSpy).toHaveBeenCalledOnce();

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
        details?: {
          mint?: string;
          message?: string;
        };
      };
    };
    expect(json.error.code).toBe("RPC_ERROR");
    expect(json.error.message).toBe("Failed to resolve mint token program");
    expect(json.error.details).toEqual({
      mint: mint.toBase58(),
      message: "mint lookup failed",
    });
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

    const firstResponse = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
        }),
      },
      retryEnv,
    );

    expect(firstResponse.status).toBe(200);

    const firstJson = (await firstResponse.json()) as {
      validator: string;
    };

    expect(firstJson.validator).toBe(fallbackValidator);

    const secondResponse = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
        }),
      },
      retryEnv,
    );

    expect(secondResponse.status).toBe(200);
    expect(fetchCalls).toBe(2);

    const json = (await secondResponse.json()) as {
      validator: string;
    };

    expect(json.validator).toBe(resolvedValidator);
  });

  it("uses the devnet RPC endpoints when cluster=devnet", async () => {
    const devnetEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.deposit.devnet.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(
        this: Connection & { _rpcEndpoint: string },
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        return endpoint.includes("base.devnet.rpc.test")
          ? {
              blockhash: "So11111111111111111111111111111111111111112",
              lastValidBlockHeight: 321,
            }
          : {
              blockhash: "11111111111111111111111111111111",
              lastValidBlockHeight: 123,
            };
      },
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(devnetEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
          cluster: "devnet",
        }),
      },
      devnetEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      recentBlockhash: string;
      validator: string;
      transactionBase64: string;
    };

    expect(json.recentBlockhash).toBe(
      "So11111111111111111111111111111111111111112",
    );
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(
      transaction.instructions.some(instruction =>
        instruction.keys.some(
          key => key.pubkey.toBase58() === DEVNET_USDC_MINT,
        ),
      ),
    ).toBe(true);
  });

  it("uses the devnet TEE RPC endpoint when cluster=devnet-private", async () => {
    const devnetPrivateEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.deposit.devnet.rpc.test",
      EPHEMERAL_DEVNET_TEE_RPC_URL: "https://ephemeral-tee.deposit.devnet.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(
        this: Connection & { _rpcEndpoint: string },
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        return endpoint.includes("base.devnet.rpc.test")
          ? {
              blockhash: "So11111111111111111111111111111111111111112",
              lastValidBlockHeight: 321,
            }
          : {
              blockhash: "11111111111111111111111111111111",
              lastValidBlockHeight: 123,
            };
      },
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(devnetPrivateEnv.EPHEMERAL_DEVNET_TEE_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
          cluster: "devnet-private",
        }),
      },
      devnetPrivateEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      recentBlockhash: string;
      validator: string;
      transactionBase64: string;
    };

    expect(json.recentBlockhash).toBe(
      "So11111111111111111111111111111111111111112",
    );
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(
      transaction.instructions.some(instruction =>
        instruction.keys.some(
          key => key.pubkey.toBase58() === DEVNET_USDC_MINT,
        ),
      ),
    ).toBe(true);
  });

  it("uses the mainnet TEE RPC endpoint when cluster=mainnet-private", async () => {
    const mainnetPrivateEnv = {
      ...env,
      EPHEMERAL_TEE_RPC_URL: "https://ephemeral-tee.challenge.rpc.test",
    };

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(
        `${mainnetPrivateEnv.EPHEMERAL_TEE_RPC_URL}/auth/challenge?pubkey=${owner}`,
      );
      return new Response(
        JSON.stringify({ challenge: "mainnet-private-challenge" }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    });

    const response = await app.request(
      `/v1/spl/challenge?pubkey=${owner}&cluster=mainnet-private`,
      {},
      mainnetPrivateEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as { challenge: string };
    expect(json.challenge).toBe("mainnet-private-challenge");
  });

  it("defaults the worker cluster binding to mainnet when it is omitted", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createAccountInfo(3n),
    );

    const response = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      {
        ...env,
        CLUSTER: undefined,
      },
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
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
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(withdrawEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/withdraw",
      {
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
      },
      withdrawEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      sendTo: string;
      transactionBase64: string;
      recentBlockhash: string;
      validator: string;
    };

    expect(json.sendTo).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(transaction.instructions.length).toBeGreaterThan(0);
    const withdrawIx
      = transaction.instructions[transaction.instructions.length - 1]!;
    expect(withdrawIx.data[0]).toBe(26);
    expect(withdrawIx.data.length).toBe(45);
  });

  it("uses the mint token program when building a withdraw", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_2022_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(env.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/withdraw",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          mint: mint.toBase58(),
          amount: 1,
          idempotent: true,
          initAtasIfMissing: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const ataInstruction = transaction.instructions.find(
      ix => ix.keys[5]?.pubkey.equals(TOKEN_2022_PROGRAM_ID),
    );
    const withdrawInstruction = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 26,
    );

    expect(ataInstruction).toBeDefined();
    expect(withdrawInstruction?.keys[15].pubkey.toBase58()).toBe(
      TOKEN_2022_PROGRAM_ID.toBase58(),
    );
  });

  it("builds an initialize mint transaction with the expected queue setup instructions", async () => {
    const validatorPublicKey = Keypair.generate().publicKey;
    const validator = validatorPublicKey.toBase58();
    const mint = "So11111111111111111111111111111111111111112";
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(validator),
    );
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(new PublicKey(mint));
    const [vaultEphemeralAta] = deriveEphemeralAta(vault, new PublicKey(mint));
    const vaultAta = deriveVaultAta(new PublicKey(mint), vault);
    const fetchSpy = vi.spyOn(globalThis, "fetch");

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(
        this: Connection & { _rpcEndpoint: string },
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        return endpoint.includes("base")
          ? {
              blockhash: "So11111111111111111111111111111111111111112",
              lastValidBlockHeight: 321,
            }
          : {
              blockhash: "11111111111111111111111111111111",
              lastValidBlockHeight: 123,
            };
      },
    );
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockResolvedValue(createMintAccountInfo(TOKEN_PROGRAM_ID));

    const response = await app.request(
      "/v1/spl/initialize-mint",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          payer: owner,
          mint,
          validator,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);
    expect(fetchSpy).not.toHaveBeenCalled();
    expect(getAccountInfoSpy).toHaveBeenCalledOnce();

    const json = (await response.json()) as {
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
    expect(json.recentBlockhash).toBe(
      "So11111111111111111111111111111111111111112",
    );
    expect(json.instructionCount).toBe(7);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(transaction.instructions).toHaveLength(7);
    expect(transaction.instructions[2]?.programId.toBase58()).toBe(
      SystemProgram.programId.toBase58(),
    );

    const decodedTransfer = SystemInstruction.decodeTransfer(
      transaction.instructions[2]!,
    );
    expect(decodedTransfer.fromPubkey.toBase58()).toBe(owner);
    expect(decodedTransfer.toPubkey.toBase58()).toBe(rentPda.toBase58());
    expect(decodedTransfer.lamports).toBe(BigInt(LAMPORTS_PER_SOL / 50));

    expect(
      transaction.instructions[0]?.keys.some(
        key => key.pubkey.toBase58() === transferQueue.toBase58(),
      ),
    ).toBe(true);
    expect(
      transaction.instructions[1]?.keys.some(
        key => key.pubkey.toBase58() === rentPda.toBase58(),
      ),
    ).toBe(true);
    expect(
      transaction.instructions[3]?.keys.some(
        key => key.pubkey.toBase58() === transferQueue.toBase58(),
      ),
    ).toBe(true);
    expect(
      transaction.instructions[4]?.keys.some(
        key => key.pubkey.toBase58() === vault.toBase58(),
      ),
    ).toBe(true);
    expect(
      transaction.instructions[5]?.keys.some(
        key => key.pubkey.toBase58() === vaultAta.toBase58(),
      ),
    ).toBe(true);
    expect(
      transaction.instructions[6]?.keys.some(
        key => key.pubkey.toBase58() === vaultEphemeralAta.toBase58(),
      ),
    ).toBe(true);
    expect(Array.from(transaction.instructions[0]!.data)).toEqual([12]);
    expect(Array.from(transaction.instructions[1]!.data)).toEqual([23]);
    expect(Array.from(transaction.instructions[3]!.data)).toEqual([19]);
    expect(Array.from(transaction.instructions[4]!.data)).toEqual([1]);
    expect(Array.from(transaction.instructions[6]!.data)).toEqual([
      4,
      ...validatorPublicKey.toBytes(),
    ]);
  });

  it("builds an initialize mint transaction for Token-2022 mints", async () => {
    const validatorPublicKey = Keypair.generate().publicKey;
    const validator = validatorPublicKey.toBase58();
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault, TOKEN_2022_PROGRAM_ID);

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "So11111111111111111111111111111111111111112",
      lastValidBlockHeight: 321,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_2022_PROGRAM_ID),
    );

    const response = await app.request(
      "/v1/spl/initialize-mint",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          payer: owner,
          mint: mint.toBase58(),
          validator,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const initVaultInstruction = transaction.instructions[4]!;
    const initVaultAtaInstruction = transaction.instructions[5]!;

    expect(initVaultInstruction.keys[4].pubkey.toBase58()).toBe(
      vaultAta.toBase58(),
    );
    expect(initVaultInstruction.keys[5].pubkey.toBase58()).toBe(
      TOKEN_2022_PROGRAM_ID.toBase58(),
    );
    expect(initVaultAtaInstruction.keys[1].pubkey.toBase58()).toBe(
      vaultAta.toBase58(),
    );
    expect(initVaultAtaInstruction.keys[5].pubkey.toBase58()).toBe(
      TOKEN_2022_PROGRAM_ID.toBase58(),
    );
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
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(initializeMintEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/initialize-mint",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          payer: owner,
          mint,
        }),
      },
      initializeMintEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      validator: string;
      instructionCount: number;
    };

    expect(json.validator).toBe(resolvedValidator);
    expect(json.instructionCount).toBe(7);
  });

  it("returns a config error when RPC env vars are missing", async () => {
    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
        }),
      },
      {
        CORS_ORIGIN: "*",
      },
    );

    expect(response.status).toBe(500);

    const json = (await response.json()) as {
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
    const response = await app.request(
      "/v1/spl/deposit",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          owner,
          amount: 1,
        }),
      },
      {
        ...env,
        BASE_DEVNET_RPC_URL: "not-a-url",
        EPHEMERAL_DEVNET_RPC_URL: "still-not-a-url",
      },
    );

    expect(response.status).toBe(500);

    const json = (await response.json()) as {
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
    expect(json.error.details?.issues).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ path: ["BASE_DEVNET_RPC_URL"] }),
        expect.objectContaining({ path: ["EPHEMERAL_DEVNET_RPC_URL"] }),
      ]),
    );
  });

  it("returns a config error when devnet-private TEE RPC env vars are missing", async () => {
    const response = await app.request(
      `/v1/spl/challenge?pubkey=${owner}&cluster=devnet-private`,
      {},
      env,
    );

    expect(response.status).toBe(500);

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
        details?: {
          issues?: Array<{
            path?: string[];
          }>;
        };
      };
    };

    expect(json.error.code).toBe("CONFIG_ERROR");
    expect(json.error.message).toBe(
      "Missing worker environment variables for cluster=devnet-private",
    );
    expect(json.error.details?.issues).toEqual(
      expect.arrayContaining([
        expect.objectContaining({ path: ["EPHEMERAL_DEVNET_TEE_RPC_URL"] }),
      ]),
    );
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
    const transport = new StreamableHTTPClientTransport(
      new URL("http://localhost/mcp"),
      {
        fetch: createMcpFetch(),
      },
    );

    await client.connect(transport);

    const tools = await client.listTools();
    expect(tools.tools.some(tool => tool.name === "spl.deposit")).toBe(true);
    expect(
      tools.tools.some(tool => tool.name === "spl.getPrivateBalance"),
    ).toBe(true);

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
    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(
        this: Connection & { _rpcEndpoint: string },
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        return endpoint.includes("ephemeral")
          ? {
              blockhash: "11111111111111111111111111111111",
              lastValidBlockHeight: 456,
            }
          : {
              blockhash: "So11111111111111111111111111111111111111112",
              lastValidBlockHeight: 123,
            };
      },
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      sendTo: string;
      sendRpcEndpoint: string;
      from: string;
      recentBlockhash: string;
      instructionCount: number;
      fees: {
        lamports: string;
        tokens: string;
      };
    };

    expect(json.sendTo).toBe("ephemeral");
    expect(json.sendRpcEndpoint).toBe(env.EPHEMERAL_RPC_URL);
    expect(json.from).toBe("ephemeral");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.instructionCount).toBe(1);
    expect(json.fees).toEqual({
      lamports: "0",
      tokens: "0",
    });
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
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(
      createLookupTableResponse(null),
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: destination,
          mint: "So11111111111111111111111111111111111111112",
          amount: 5_000_000,
          visibility: "private",
          fromBalance: "base",
          toBalance: "base",
          minDelayMs: "0",
          maxDelayMs: "0",
          split: 1,
        }),
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      sendTo: string;
      from: string;
      recentBlockhash: string;
      validator: string;
      version: string;
      fees: {
        lamports: string;
        tokens: string;
      };
    };

    expect(json.sendTo).toBe("base");
    expect(json.from).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);
    expect(json.version).toBe("legacy");
  });

  it("rejects private base transfers when the source eATA is delegated to another validator", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.mismatch.rpc.test",
    };
    const mint = DEVNET_USDC_MINT;
    const currentValidator = Keypair.generate().publicKey;
    const selectedValidator = new PublicKey(resolvedValidator);
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const [sourceEata] = deriveEphemeralAta(new PublicKey(owner), new PublicKey(mint));
    const sourceAta = deriveAssociatedTokenAddress(mint, owner);

    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    const getMultipleAccountsInfoSpy = vi
      .spyOn(Connection.prototype, "getMultipleAccountsInfo")
      .mockImplementation(async function getMultipleAccountsInfo(
        this: Connection & { _rpcEndpoint: string },
        addresses,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(transferEnv.BASE_RPC_URL);
        expect(addresses.map(address => address.toBase58())).toEqual([
          delegationRecord.toBase58(),
        ]);
        return [createDelegationAccountInfo(currentValidator)];
      });
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(selectedValidator.toBase58());
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: destination,
          mint,
          amount: 5_000_000,
          visibility: "private",
          fromBalance: "base",
          toBalance: "base",
          minDelayMs: "0",
          maxDelayMs: "0",
          split: 1,
        }),
      },
      transferEnv,
    );

    expect(response.status).toBe(400);
    expect(getMultipleAccountsInfoSpy).toHaveBeenCalledOnce();

    const json = (await response.json()) as {
      error: {
        code: string;
        details: {
          accounts: Array<{
            role: string;
            ata: string;
            eata: string;
            currentValidator: string;
            selectedValidator: string;
          }>;
        };
      };
    };

    expect(json.error.code).toBe("EATA_VALIDATOR_MISMATCH");
    expect(json.error.details.accounts).toEqual([
      {
        role: "source",
        owner,
        mint,
        ata: sourceAta,
        eata: sourceEata.toBase58(),
        delegationRecord: delegationRecord.toBase58(),
        currentValidator: currentValidator.toBase58(),
        selectedValidator: selectedValidator.toBase58(),
      },
    ]);
  });

  it("uses the mint token program when initializing a transfer vault", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault, TOKEN_2022_PROGRAM_ID);

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_2022_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(env.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: destination,
          mint: mint.toBase58(),
          amount: 2,
          visibility: "public",
          fromBalance: "base",
          toBalance: "base",
          initVaultIfMissing: true,
          legacy: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const initVaultInstruction = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 1,
    );
    const initVaultAtaInstruction = transaction.instructions.find(
      ix => ix.keys[1]?.pubkey.equals(vaultAta)
        && ix.keys[5]?.pubkey.equals(TOKEN_2022_PROGRAM_ID),
    );
    const transferInstruction = transaction.instructions.find(
      ix => ix.programId.equals(TOKEN_2022_PROGRAM_ID) && ix.data[0] === 3,
    );

    expect(initVaultInstruction?.keys[4].pubkey.toBase58()).toBe(
      vaultAta.toBase58(),
    );
    expect(initVaultInstruction?.keys[5].pubkey.toBase58()).toBe(
      TOKEN_2022_PROGRAM_ID.toBase58(),
    );
    expect(initVaultAtaInstruction).toBeDefined();
    expect(transferInstruction).toBeDefined();
  });

  it("uses the mint token program when building a transfer without init flags", async () => {
    const mint = new PublicKey("2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo");

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_2022_PROGRAM_ID),
    );

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: destination,
          mint: mint.toBase58(),
          amount: 2,
          visibility: "public",
          fromBalance: "base",
          toBalance: "base",
          validator: resolvedValidator,
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const transferInstruction = transaction.instructions.find(
      ix => ix.programId.equals(TOKEN_2022_PROGRAM_ID) && ix.data[0] === 3,
    );

    expect(transferInstruction).toBeDefined();
  });

  it("builds an initialize-or-update stealth pool transaction", async () => {
    const payer = Keypair.generate().publicKey.toBase58();
    const authority = Keypair.generate().publicKey.toBase58();

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(
        this: Connection & { _rpcEndpoint: string },
      ) {
        return {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        };
      },
    );

    const response = await app.request(
      "/v1/spl/stealth-pool",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "authorization": `Bearer ${MOCK_AUTH_TOKEN}`,
        },
        body: JSON.stringify({
          payer,
          authority,
          handle: stealthHandle,
          destinations: [destination],
          splitAcrossKeys: true,
        }),
      },
      env,
    );

    expect(response.status, await response.clone().text()).toBe(200);

    const json = (await response.json()) as {
      kind: string;
      sendTo: string;
      stealthPool: string;
      transactionBase64: string;
      requiredSigners: string[];
      setupTransaction: {
        sendTo: string;
        transactionBase64: string;
        requiredSigners: string[];
        validator: string;
      };
    };
    expect(json.kind).toBe("stealthPool");
    expect(json.sendTo).toBe("ephemeral");
    expect(json.stealthPool).toBe(stealthPool);
    expect(json).not.toHaveProperty("handleHash");
    expect(json.requiredSigners.sort()).toEqual([authority, payer].sort());
    expect(json.setupTransaction.sendTo).toBe("base");
    expect(json.setupTransaction.requiredSigners.sort()).toEqual([authority, payer].sort());

    const setupTransaction = Transaction.from(
      Buffer.from(json.setupTransaction.transactionBase64, "base64"),
    );
    const setupInstruction = setupTransaction.instructions[0];
    const stealthPoolPubkey = new PublicKey(stealthPool);
    expect(setupInstruction.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)).toBe(
      true,
    );
    expect(setupInstruction.keys.map(key => key.pubkey.toBase58())).toEqual([
      payer,
      stealthPool,
      permissionPdaFromAccount(stealthPoolPubkey).toBase58(),
      EPHEMERAL_SPL_TOKEN_PROGRAM_ID.toBase58(),
      delegateBufferPdaFromDelegatedAccountAndOwnerProgram(
        stealthPoolPubkey,
        EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
      ).toBase58(),
      delegationRecordPdaFromDelegatedAccount(stealthPoolPubkey).toBase58(),
      delegationMetadataPdaFromDelegatedAccount(stealthPoolPubkey).toBase58(),
      DELEGATION_PROGRAM_ID.toBase58(),
      SystemProgram.programId.toBase58(),
      PERMISSION_PROGRAM_ID.toBase58(),
      authority,
    ]);
    const handleStorage = createStealthHandleStorage(stealthHandle);
    expect([...setupInstruction.data]).toEqual([
      22,
      ...handleStorage,
      ...new PublicKey(json.setupTransaction.validator).toBuffer(),
    ]);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const instruction = transaction.instructions[0];
    expect(instruction.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)).toBe(
      true,
    );
    expect(instruction.keys.map(key => key.pubkey.toBase58())).toEqual([
      payer,
      stealthPool,
      authority,
      SystemProgram.programId.toBase58(),
    ]);
    expect([...instruction.data.subarray(0, 259)]).toEqual([
      21,
      ...handleStorage,
      1,
      1,
    ]);
    const destinationsOffset = 259;
    expect(instruction.data.subarray(destinationsOffset).toString("hex")).toBe(
      new PublicKey(destination).toBuffer().toString("hex"),
    );
  });

  it("reports base stealth pool status without exposing destinations", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address: PublicKey,
      ) {
        expect(this._rpcEndpoint).toBe(env.BASE_RPC_URL);
        expect(address.toBase58()).toBe(stealthPool);
        return createStealthPoolAccountInfo();
      },
    );

    const response = await app.request(
      `/v1/spl/stealth-pool?handle=${encodeURIComponent(stealthHandle)}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      stealthPool: string;
      exists: boolean;
      handleHash?: unknown;
      destinations?: unknown;
    };
    expect(json).toEqual({
      stealthPool,
      exists: true,
    });
    expect(json.destinations).toBeUndefined();
  });

  it("derives stealth pool PDAs with chunked handle seeds after 32 bytes", () => {
    const longHandle = "john.doe-long-handle-more-than-32.block";
    const handleBytes = Buffer.from(longHandle, "utf8");
    expect(handleBytes.length).toBeGreaterThan(32);

    const [actual] = deriveStealthPoolFromHandle(longHandle);
    const [expected] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("stealth_pool"),
        handleBytes.subarray(0, 32),
        handleBytes.subarray(32),
      ],
      EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    );

    expect(actual.toBase58()).toBe(expected.toBase58());
  });

  it("derives stealth pool PDAs for 255-byte handles", () => {
    const longHandle = "a".repeat(255);
    const handleBytes = Buffer.from(longHandle, "utf8");
    const handleHashSeed = Buffer.from(
      sha256(Buffer.concat([Buffer.from("stealth_pool_handle"), handleBytes])),
    );

    const [actual] = deriveStealthPoolFromHandle(longHandle);
    const [expected] = PublicKey.findProgramAddressSync(
      [
        Buffer.from("stealth_pool"),
        handleBytes.subarray(0, 32),
        handleHashSeed,
      ],
      EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    );

    expect(actual.toBase58()).toBe(expected.toBase58());
  });

  it("rejects stealth transfers from ephemeral balance", async () => {
    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
          "authorization": "Bearer abc",
        },
        body: JSON.stringify({
          from: owner,
          to: stealthHandle,
          mint: "So11111111111111111111111111111111111111112",
          amount: 2,
          visibility: "private",
          fromBalance: "ephemeral",
          toBalance: "base",
          minDelayMs: "0",
          maxDelayMs: "0",
          split: 1,
        }),
      },
      env,
    );

    expect(response.status).toBe(400);

    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("INVALID_STEALTH_TRANSFER");
    expect(json.error.message).toBe(
      "Stealth handle transfers require visibility=private, fromBalance=base, and toBalance=base",
    );
  });

  it("rejects stealth transfers when the derived pool PDA is missing", async () => {
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const [stealthPoolPubkey] = deriveStealthPoolFromHandle(stealthHandle);
    expect(stealthPoolPubkey.toBase58()).toBe(stealthPool);
    const getAccountInfo = vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address: PublicKey,
      ) {
        expect(this._rpcEndpoint).toBe(env.BASE_RPC_URL);
        expect(address.toBase58()).toBe(stealthPoolPubkey.toBase58());
        return null;
      },
    );

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: stealthHandle,
          mint: mint.toBase58(),
          amount: 2,
          minDelayMs: "0",
          maxDelayMs: "0",
          split: 1,
          legacy: true,
        }),
      },
      env,
    );

    expect(response.status, await response.clone().text()).toBe(400);
    expect(getAccountInfo).toHaveBeenCalledTimes(1);

    const json = (await response.json()) as {
      error: { code: string; message: string };
    };
    expect(json.error.code).toBe("STEALTH_POOL_NOT_FOUND");
    expect(json.error.message).toBe("Stealth handle is not initialized");
  });

  it.skip("builds a base stealth transfer after verifying the derived pool PDA exists", async () => {
    const mint = new PublicKey("So11111111111111111111111111111111111111112");
    const [stealthPoolPubkey] = deriveStealthPoolFromHandle(stealthHandle);
    expect(stealthPoolPubkey.toBase58()).toBe(stealthPool);

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockImplementation(
      async function getLatestBlockhash(
        this: Connection & { _rpcEndpoint: string },
      ) {
        expect(this._rpcEndpoint).toBe(env.BASE_RPC_URL);
        return {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        };
      },
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async (address) => {
        if (address.equals(stealthPoolPubkey)) {
          return createStealthPoolAccountInfo();
        }

        if (address.equals(mint)) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }

        throw new Error(`Unexpected stealth transfer account lookup: ${address.toBase58()}`);
      },
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(env.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: stealthHandle,
          mint: mint.toBase58(),
          amount: 2,
          minDelayMs: "0",
          maxDelayMs: "0",
          split: 1,
          legacy: true,
        }),
      },
      env,
    );

    expect(response.status, await response.clone().text()).toBe(200);

    const json = (await response.json()) as {
      sendTo: string;
      transactionBase64: string;
      validator: string;
    };
    expect(json.sendTo).toBe("base");
    expect(json.validator).toBe(resolvedValidator);

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const instruction = transaction.instructions.at(-1)!;
    expect(instruction.data[0]).toBe(25);
    expect(instruction.keys).toHaveLength(19);
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
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(
      createLookupTableResponse(
        createLookupTableAccount([
          mint,
          transferQueue,
          rentPda,
          vault,
          vaultAta,
          TOKEN_PROGRAM_ID,
          SystemProgram.programId,
        ]),
      ),
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async address =>
        address.equals(mint)
          ? createMintAccountInfo(TOKEN_PROGRAM_ID)
          : createLookupTableAccountInfo(),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      version: string;
      from: string;
      transactionBase64: string;
    };

    expect(json.version).toBe("v0");
    expect(json.from).toBe("base");
    expect(() =>
      VersionedTransaction.deserialize(
        Buffer.from(json.transactionBase64, "base64"),
      ),
    ).not.toThrow();
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

    const getLatestBlockhashSpy = vi
      .spyOn(Connection.prototype, "getLatestBlockhash")
      .mockResolvedValue({
        blockhash: "11111111111111111111111111111111",
        lastValidBlockHeight: 123,
      });
    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(
        createLookupTableResponse(
          createLookupTableAccount([
            mint,
            transferQueue,
            rentPda,
            vault,
            vaultAta,
            TOKEN_PROGRAM_ID,
            SystemProgram.programId,
          ]),
        ),
      );
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockImplementation(async address =>
        address.equals(mint)
          ? createMintAccountInfo(TOKEN_PROGRAM_ID)
          : createLookupTableAccountInfo(),
      );

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

    const firstResponse = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify(body),
      },
      transferEnv,
    );
    const secondResponse = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify(body),
      },
      transferEnv,
    );

    expect(firstResponse.status).toBe(200);
    expect(secondResponse.status).toBe(200);
    expect(getLatestBlockhashSpy).toHaveBeenCalledTimes(2);
    expect(getAddressLookupTableSpy).toHaveBeenCalledOnce();
    expect(getAccountInfoSpy).toHaveBeenCalledTimes(5);
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
      .mockResolvedValue(
        createLookupTableResponse(
          createLookupTableAccount([
            mint,
            transferQueue,
            rentPda,
            vault,
            vaultAta,
            TOKEN_PROGRAM_ID,
            SystemProgram.programId,
          ]),
        ),
      );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      version: string;
      transactionBase64: string;
    };

    expect(json.version).toBe("legacy");
    expect(() =>
      Transaction.from(Buffer.from(json.transactionBase64, "base64")),
    ).not.toThrow();
    expect(getAddressLookupTableSpy).not.toHaveBeenCalled();
  });

  it("falls back to a legacy private base transfer when the LUT has no matching addresses", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.transfer.no-match.rpc.test",
    };
    const mint = new PublicKey("So11111111111111111111111111111111111111112");

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    const getAddressLookupTableSpy = vi
      .spyOn(Connection.prototype, "getAddressLookupTable")
      .mockResolvedValue(
        createLookupTableResponse(
          createLookupTableAccount([
            Keypair.generate().publicKey,
            Keypair.generate().publicKey,
            Keypair.generate().publicKey,
            Keypair.generate().publicKey,
            Keypair.generate().publicKey,
            Keypair.generate().publicKey,
            Keypair.generate().publicKey,
          ]),
        ),
      );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async address =>
        address.equals(mint)
          ? createMintAccountInfo(TOKEN_PROGRAM_ID)
          : createLookupTableAccountInfo(),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      version: string;
      transactionBase64: string;
    };

    expect(json.version).toBe("legacy");
    expect(() =>
      Transaction.from(Buffer.from(json.transactionBase64, "base64")),
    ).not.toThrow();
    expect(getAddressLookupTableSpy).toHaveBeenCalledOnce();
  });

  it("builds a gasless private transfer with the sponsor as fee payer", async () => {
    const sponsor = Keypair.generate();
    const mint = DEVNET_USDC_MINT;
    const amount = 5_000_000;
    const ownerAta = deriveAssociatedTokenAddress(mint, owner);
    const sponsorAta = deriveAssociatedTokenAddress(
      mint,
      sponsor.publicKey.toBase58(),
    );
    const transferEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.gasless-transfer.rpc.test",
      GASLESS_SPONSOR_SECRET_KEY: JSON.stringify(Array.from(sponsor.secretKey)),
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation(
      (array) => {
        if (array instanceof Uint32Array) {
          array.fill(7);
          return array;
        }

        (array as Uint8Array).fill(1);
        return array;
      },
    );

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      requiredSigners: string[];
      transactionBase64: string;
      fees: {
        lamports: string;
        tokens: string;
      };
    };
    expect(json.fees).toEqual({
      lamports: "2039280",
      tokens: "205000",
    });
    expect(json.requiredSigners).toEqual(
      expect.arrayContaining([owner, sponsor.publicKey.toBase58()]),
    );

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(transaction.feePayer?.toBase58()).toBe(sponsor.publicKey.toBase58());
    expect(transaction.instructions).toHaveLength(4);
    expect(
      transaction.instructions.some(ix =>
        ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID)
        && ix.data.length === 1
        && ix.data[0] === 28,
      ),
    ).toBe(false);

    const sponsorSignature = transaction.signatures.find(
      signature =>
        signature.publicKey.toBase58() === sponsor.publicKey.toBase58(),
    );
    expect(sponsorSignature?.signature).not.toBeNull();

    const relayFeeIx = transaction.instructions[0]!;
    expect(relayFeeIx.programId.toBase58()).toBe(TOKEN_PROGRAM_ID.toBase58());
    expect(relayFeeIx.keys.map(key => key.pubkey.toBase58())).toEqual([
      ownerAta,
      sponsorAta,
      owner,
    ]);
    expect(relayFeeIx.data[0]).toBe(3);
    expect(relayFeeIx.data.readBigUInt64LE(1)).toBe(200_000n);

    const privateTransferIx = transaction.instructions.at(-1)!;
    expect(privateTransferIx.programId.toBase58()).toBe(
      EPHEMERAL_SPL_TOKEN_PROGRAM_ID.toBase58(),
    );
    expect(privateTransferIx.data[0]).toBe(25);
    expect(privateTransferIx.data.readUInt32LE(1)).toBe(7);
    expect(privateTransferIx.data.readBigUInt64LE(5)).toBe(BigInt(amount));
    expect(privateTransferIx.data[13]).toBe(1);
    expect(privateTransferIx.data[94]).toBe(1);
    expect(privateTransferIx.data.subarray(95, 127)).toEqual(
      new PublicKey(resolvedValidator).toBuffer(),
    );
    expect(privateTransferIx.data[127]).toBe(
      privateTransferIx.data.length - 128,
    );
  });

  it("builds a gasless v0 private transfer with the sponsor signature", async () => {
    const sponsor = Keypair.generate();
    const mint = new PublicKey(DEVNET_USDC_MINT);
    const validator = new PublicKey(resolvedValidator);
    const [transferQueue] = deriveTransferQueue(mint, validator);
    const [rentPda] = deriveRentPda();
    const [vault] = deriveVault(mint);
    const vaultAta = deriveVaultAta(mint, vault);
    const transferEnv = {
      ...env,
      BASE_DEVNET_RPC_URL: "https://base.devnet.gasless-transfer.v0.rpc.test",
      EPHEMERAL_DEVNET_RPC_URL:
        "https://ephemeral.devnet.gasless-transfer.v0.rpc.test",
      GASLESS_SPONSOR_SECRET_KEY: JSON.stringify(Array.from(sponsor.secretKey)),
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(
      createLookupTableResponse(
        createLookupTableAccount(
          [
            mint,
            transferQueue,
            rentPda,
            vault,
            vaultAta,
            TOKEN_PROGRAM_ID,
            EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
            SystemProgram.programId,
          ],
          new PublicKey("E26JGdRsdKkGe6oRU4Un24agZjBF2Bg9z1ctfZByETRo"),
        ),
      ),
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async address =>
        address.equals(mint)
          ? createMintAccountInfo(TOKEN_PROGRAM_ID)
          : createLookupTableAccountInfo(),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: destination,
          mint: mint.toBase58(),
          amount: 5_000_000,
          cluster: "devnet",
          visibility: "private",
          fromBalance: "base",
          toBalance: "base",
          minDelayMs: "0",
          maxDelayMs: "0",
          split: 1,
          gasless: true,
        }),
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      requiredSigners: string[];
      transactionBase64: string;
      version: string;
      fees: {
        lamports: string;
        tokens: string;
      };
    };
    expect(json.version).toBe("v0");
    expect(json.fees).toEqual({
      lamports: "2039280",
      tokens: "205000",
    });
    expect(json.requiredSigners).toEqual(
      expect.arrayContaining([owner, sponsor.publicKey.toBase58()]),
    );

    const transaction = VersionedTransaction.deserialize(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const requiredSigners = transaction.message.staticAccountKeys.slice(
      0,
      transaction.message.header.numRequiredSignatures,
    );
    const sponsorSignatureIndex = requiredSigners.findIndex(key =>
      key.equals(sponsor.publicKey),
    );
    const ownerSignatureIndex = requiredSigners.findIndex(
      key => key.toBase58() === owner,
    );

    expect(sponsorSignatureIndex).toBeGreaterThanOrEqual(0);
    expect(ownerSignatureIndex).toBeGreaterThanOrEqual(0);
    expect(
      transaction.signatures[sponsorSignatureIndex]?.every(
        byte => byte === 0,
      ),
    ).toBe(false);
    expect(
      transaction.signatures[ownerSignatureIndex]?.every(byte => byte === 0),
    ).toBe(true);
  });

  it("builds a gasless public transfer with the sponsor as fee payer", async () => {
    const sponsor = Keypair.generate();
    const mint = DEVNET_USDC_MINT;
    const amount = 5_000_000;
    const ownerAta = deriveAssociatedTokenAddress(mint, owner);
    const sponsorAta = deriveAssociatedTokenAddress(
      mint,
      sponsor.publicKey.toBase58(),
    );
    const destinationAta = deriveAssociatedTokenAddress(mint, destination);
    const transferEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL:
        "https://ephemeral.gasless-public-transfer.rpc.test",
      GASLESS_SPONSOR_SECRET_KEY: JSON.stringify(Array.from(sponsor.secretKey)),
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      requiredSigners: string[];
      transactionBase64: string;
      fees: {
        lamports: string;
        tokens: string;
      };
    };
    expect(json.fees).toEqual({
      lamports: "0",
      tokens: "200000",
    });
    expect(json.requiredSigners).toEqual(
      expect.arrayContaining([owner, sponsor.publicKey.toBase58()]),
    );

    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    expect(transaction.feePayer?.toBase58()).toBe(sponsor.publicKey.toBase58());
    expect(transaction.instructions).toHaveLength(2);

    const relayFeeIx = transaction.instructions[0]!;
    expect(relayFeeIx.programId.toBase58()).toBe(TOKEN_PROGRAM_ID.toBase58());
    expect(relayFeeIx.keys.map(key => key.pubkey.toBase58())).toEqual([
      ownerAta,
      sponsorAta,
      owner,
    ]);
    expect(relayFeeIx.data[0]).toBe(3);
    expect(relayFeeIx.data.readBigUInt64LE(1)).toBe(200_000n);

    const publicTransferIx = transaction.instructions[1]!;
    expect(publicTransferIx.programId.toBase58()).toBe(
      TOKEN_PROGRAM_ID.toBase58(),
    );
    expect(publicTransferIx.keys.map(key => key.pubkey.toBase58())).toEqual([
      ownerAta,
      destinationAta,
      owner,
    ]);
    expect(publicTransferIx.data[0]).toBe(3);
    expect(publicTransferIx.data.readBigUInt64LE(1)).toBe(BigInt(amount));
  });

  it("ignores gasless for off-curve transfer senders", async () => {
    const [offCurveSender] = PublicKey.findProgramAddressSync(
      [Buffer.from("off-curve-sender")],
      EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
    );

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(env.EPHEMERAL_DEVNET_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: offCurveSender.toBase58(),
          to: destination,
          mint: DEVNET_USDC_MINT,
          amount: 1,
          cluster: "devnet",
          visibility: "public",
          fromBalance: "base",
          toBalance: "base",
          gasless: true,
        }),
      },
      env,
    );

    expect(response.status).toBe(400);

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
      };
    };
    expect(json.error.code).toBe("TRANSACTION_BUILD_ERROR");
    expect(json.error.message).toBe("Owner public key is off-curve");
  });

  it("rejects gasless transfers when the sponsor key is not configured", async () => {
    const transferEnv = {
      ...env,
      EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.gasless-missing.rpc.test",
    };

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(503);

    const json = (await response.json()) as {
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
    vi.spyOn(Connection.prototype, "getAddressLookupTable").mockResolvedValue(
      createLookupTableResponse(null),
    );
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(globalThis.crypto, "getRandomValues").mockImplementation(
      (array) => {
        if (array instanceof Uint32Array) {
          array.fill(7);
          return array;
        }

        (array as Uint8Array).fill(1);
        return array;
      },
    );

    const withoutClientRefResponse = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify(baseBody),
      },
      transferEnv,
    );

    const withClientRefResponse = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          ...baseBody,
          clientRefId: "42",
        }),
      },
      transferEnv,
    );

    expect(withoutClientRefResponse.status).toBe(200);
    expect(withClientRefResponse.status).toBe(200);

    const withoutClientRefJson = (await withoutClientRefResponse.json()) as {
      instructionCount: number;
      transactionBase64: string;
    };
    const withClientRefJson = (await withClientRefResponse.json()) as {
      instructionCount: number;
      transactionBase64: string;
    };

    expect(withClientRefJson.instructionCount).toBe(
      withoutClientRefJson.instructionCount,
    );
    expect(withClientRefJson.transactionBase64).not.toBe(
      withoutClientRefJson.transactionBase64,
    );

    const withoutClientRefTx = Transaction.from(
      Buffer.from(withoutClientRefJson.transactionBase64, "base64"),
    );
    const withClientRefTx = Transaction.from(
      Buffer.from(withClientRefJson.transactionBase64, "base64"),
    );

    expect(withClientRefTx.instructions).toHaveLength(
      withoutClientRefTx.instructions.length,
    );
    expect(
      withClientRefTx.instructions.map(instruction =>
        Buffer.from(instruction.data).toString("base64"),
      ),
    ).not.toEqual(
      withoutClientRefTx.instructions.map(instruction =>
        Buffer.from(instruction.data).toString("base64"),
      ),
    );
  });

  it("rejects split values above 15", async () => {
    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      env,
    );

    expect(response.status).toBe(422);
  });

  it("validates clientRefId as a bigint string at the API layer", async () => {
    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      env,
    );

    expect(response.status).toBe(422);

    const json = (await response.json()) as {
      error: {
        issues: Array<{
          path: string[];
          message: string;
        }>;
      };
    };

    expect(json.error.issues).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          path: ["clientRefId"],
          message: "Must be a non-negative bigint string",
        }),
      ]),
    );
  });

  it("rejects private transfers with maxDelayMs above 10 minutes", async () => {
    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      env,
    );

    expect(response.status).toBe(400);

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
      };
    };

    expect(json.error.code).toBe("INVALID_PRIVATE_TRANSFER");
    expect(json.error.message).toBe(
      "maxDelayMs must be less than or equal to 600000",
    );
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
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(transferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      transferEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      instructionCount: number;
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const memoInstruction = transaction.instructions.at(-1);

    expect(json.instructionCount).toBe(2);
    expect(memoInstruction?.programId.toBase58()).toBe(
      "MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr",
    );
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
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(unsupportedTransferEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
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
      },
      unsupportedTransferEnv,
    );

    expect(response.status).toBe(400);

    const json = (await response.json()) as { error: { code: string } };
    expect(json.error.code).toBe("UNSUPPORTED_TRANSFER_ROUTE");
  });

  it("builds a zero-amount private base-to-ephemeral setup transfer", async () => {
    const setupEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.zero-setup.rpc.test",
    };

    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createMintAccountInfo(TOKEN_PROGRAM_ID),
    );
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(setupEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      "/v1/spl/transfer",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          from: owner,
          to: destination,
          mint: "So11111111111111111111111111111111111111112",
          amount: 0,
          visibility: "private",
          fromBalance: "base",
          toBalance: "ephemeral",
          initIfMissing: true,
          initAtasIfMissing: true,
          initVaultIfMissing: true,
        }),
      },
      setupEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      sendTo: string;
      transactionBase64: string;
    };
    const transaction = Transaction.from(
      Buffer.from(json.transactionBase64, "base64"),
    );
    const setupInstruction = transaction.instructions.find(
      ix => ix.programId.equals(EPHEMERAL_SPL_TOKEN_PROGRAM_ID) && ix.data[0] === 24,
    );

    expect(json.sendTo).toBe("base");
    expect(setupInstruction).toBeDefined();
    expect(setupInstruction?.data.readBigUInt64LE(5)).toBe(0n);
  });

  it("returns base and private balances from different RPCs", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const balanceEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.balance.rpc.test",
    };
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const validator = new PublicKey(resolvedValidator);

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(balanceEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;

        if (address.toBase58() === delegationRecord.toBase58()) {
          expect(endpoint).toBe(balanceEnv.BASE_RPC_URL);
          return createDelegationAccountInfo(validator);
        }

        return endpoint.includes("ephemeral")
          ? createAccountInfo(9n)
          : createAccountInfo(3n);
      },
    );

    const baseResponse = await app.request(
      `/v1/spl/balance?address=${owner}&mint=${mint}`,
      {},
      balanceEnv,
    );
    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=${mint}`,
      { headers: { authorization: "Bearer 1234567890" } },
      balanceEnv,
    );

    expect(baseResponse.status).toBe(200);
    expect(privateResponse.status).toBe(200);

    const baseJson = (await baseResponse.json()) as {
      location: string;
      balance: string;
    };
    const privateJson = (await privateResponse.json()) as {
      location: string;
      balance: string;
    };

    expect(baseJson.location).toBe("base");
    expect(baseJson.balance).toBe("3");
    expect(privateJson.location).toBe("ephemeral");
    expect(privateJson.balance).toBe("9");
  });

  it("returns Token-2022 private balance from the Token-2022 ATA", async () => {
    const mint = "2b1kV6DkPAnxd5ixfnxCpjxmKwqjjaYmCZfHsFu24GXo";
    const balanceEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.token-2022-balance.rpc.test",
    };
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const validator = new PublicKey(resolvedValidator);
    const token2022Ata = deriveAssociatedTokenAddress(
      mint,
      owner,
      TOKEN_2022_PROGRAM_ID,
    );
    const legacyAta = deriveAssociatedTokenAddress(mint, owner);

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(balanceEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;

        if (endpoint === balanceEnv.BASE_RPC_URL && address.toBase58() === mint) {
          return createMintAccountInfo(TOKEN_2022_PROGRAM_ID);
        }

        if (address.toBase58() === delegationRecord.toBase58()) {
          expect(endpoint).toBe(balanceEnv.BASE_RPC_URL);
          return createDelegationAccountInfo(validator);
        }

        expect(endpoint).toContain(balanceEnv.EPHEMERAL_RPC_URL);
        expect(address.toBase58()).toBe(token2022Ata);
        expect(address.toBase58()).not.toBe(legacyAta);
        return createAccountInfo(4n, TOKEN_2022_PROGRAM_ID);
      },
    );

    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=${mint}`,
      { headers: { authorization: "Bearer 1234567890" } },
      balanceEnv,
    );

    expect(privateResponse.status).toBe(200);

    const privateJson = (await privateResponse.json()) as {
      ata: string;
      location: string;
      balance: string;
    };

    expect(privateJson.ata).toBe(token2022Ata);
    expect(privateJson.location).toBe("ephemeral");
    expect(privateJson.balance).toBe("4");
  });

  it("returns zero private balance when the eATA is not delegated", async () => {
    const mint = DEVNET_USDC_MINT;
    const balanceEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.undelegated-balance.rpc.test",
    };
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockImplementation(async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(balanceEnv.BASE_RPC_URL);
        if (address.toBase58() === mint) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }
        expect(address.toBase58()).toBe(delegationRecord.toBase58());
        return null;
      });

    const response = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=${mint}`,
      { headers: { authorization: "Bearer 1234567890" } },
      balanceEnv,
    );

    expect(response.status).toBe(200);
    expect(getAccountInfoSpy).toHaveBeenCalledTimes(2);

    const json = (await response.json()) as {
      location: string;
      balance: string;
    };

    expect(json.location).toBe("ephemeral");
    expect(json.balance).toBe("0");
  });

  it("returns an error when the eATA is delegated to another validator", async () => {
    const mint = DEVNET_USDC_MINT;
    const balanceEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.wrong-validator-balance.rpc.test",
    };
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const otherValidator = Keypair.generate().publicKey;
    const getAccountInfoSpy = vi
      .spyOn(Connection.prototype, "getAccountInfo")
      .mockImplementation(async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(balanceEnv.BASE_RPC_URL);
        if (address.toBase58() === mint) {
          return createMintAccountInfo(TOKEN_PROGRAM_ID);
        }
        expect(address.toBase58()).toBe(delegationRecord.toBase58());
        return createDelegationAccountInfo(otherValidator);
      });

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(balanceEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });

    const response = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=${mint}`,
      { headers: { authorization: "Bearer 1234567890" } },
      balanceEnv,
    );

    expect(response.status).toBe(400);
    expect(getAccountInfoSpy).toHaveBeenCalledTimes(2);

    const json = (await response.json()) as {
      error: {
        code: string;
        message: string;
        details: {
          eata: string;
          delegatedValidator: string;
          selectedValidator: string;
        };
      };
    };

    expect(json.error.code).toBe("EATA_DELEGATED_ELSEWHERE");
    expect(json.error.message).toBe("eATA is delegated to a different validator");
    expect(json.error.details.eata).toBe(deriveEphemeralAta(new PublicKey(owner), new PublicKey(mint))[0].toBase58());
    expect(json.error.details.delegatedValidator).toBe(otherValidator.toBase58());
    expect(json.error.details.selectedValidator).toBe(resolvedValidator);
  });

  it("returns private balance with the mock auth token", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const balanceEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.mock-balance.rpc.test",
    };
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const validator = new PublicKey(resolvedValidator);

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(balanceEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;

        if (address.toBase58() === delegationRecord.toBase58()) {
          expect(endpoint).toBe(balanceEnv.BASE_RPC_URL);
          return createDelegationAccountInfo(validator);
        }

        return endpoint.includes("ephemeral")
          ? createAccountInfo(9n)
          : createAccountInfo(3n);
      },
    );

    const baseResponse = await app.request(
      `/v1/spl/balance?address=${owner}&mint=${mint}`,
      {},
      balanceEnv,
    );
    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=${mint}`,
      { headers: { authorization: "Bearer mock-auth-token" } },
      balanceEnv,
    );

    expect(baseResponse.status).toBe(200);
    expect(privateResponse.status).toBe(200);

    const baseJson = (await baseResponse.json()) as {
      location: string;
      balance: string;
    };
    const privateJson = (await privateResponse.json()) as {
      location: string;
      balance: string;
    };

    expect(baseJson.location).toBe("base");
    expect(baseJson.balance).toBe("3");
    expect(privateJson.location).toBe("ephemeral");
    expect(privateJson.balance).toBe("9");
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
      expect(String(input)).toBe(
        `${env.EPHEMERAL_RPC_URL}/auth/challenge?pubkey=${owner}`,
      );
      return new Response(
        JSON.stringify({ challenge: "challenge-token-abc" }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    });

    const response = await app.request(
      `/v1/spl/challenge?pubkey=${owner}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as { challenge: string };
    expect(json.challenge).toBe("challenge-token-abc");
  });

  it("returns the mock challenge", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(
        `${env.EPHEMERAL_RPC_URL}/auth/challenge?pubkey=${owner}`,
      );
      return new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          error: {
            code: -32600,
            message: "invalid request: missing request body",
          },
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    });

    const response = await app.request(
      `/v1/spl/challenge?pubkey=${owner}`,
      {},
      env,
    );

    console.log(response);
    expect(response.status).toBe(200);

    const json = (await response.json()) as { challenge: string };
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

    const response = await app.request(
      "/v1/spl/login",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          pubkey: owner,
          challenge: "c1",
          signature: "s1",
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as { token: string };
    expect(json.token).toBe("token-xyz");
  });

  it("returns the mock token", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(`${env.EPHEMERAL_RPC_URL}/auth/login`);
      return new Response(
        JSON.stringify({
          jsonrpc: "2.0",
          error: {
            code: -32700,
            message:
              "error parsing request body: missing field `id` at line 1 column 91\n\n\tre\":\"s1\"}\n\t........^\n",
          },
        }),
        {
          status: 200,
          headers: {
            "content-type": "application/json",
          },
        },
      );
    });

    const response = await app.request(
      "/v1/spl/login",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          pubkey: owner,
          challenge: "c1",
          signature: "s1",
        }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as { token: string };
    expect(json.token).toBe(MOCK_AUTH_TOKEN);
  });

  it("maps upstream login rate limits to 429", async () => {
    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(`${env.EPHEMERAL_RPC_URL}/auth/login`);
      return new Response(
        JSON.stringify({
          error:
            "Unexpected Range status: 429 Too Many Requests; body={\"message\":\"Too many requests\",\"statusCode\":429}",
        }),
        {
          status: 400,
          statusText: "Bad Request",
          headers: {
            "content-type": "text/plain",
          },
        },
      );
    });

    const response = await app.request(
      "/v1/spl/login",
      {
        method: "POST",
        headers: {
          "content-type": "application/json",
        },
        body: JSON.stringify({
          pubkey: owner,
          challenge: "c1",
          signature: "s1",
        }),
      },
      env,
    );

    expect(response.status).toBe(429);

    const json = (await response.json()) as { error: { message: string } };
    expect(json.error.message).toBe("Failed to login: Too Many Requests");
  });

  it("redacts RPC URLs from balance error details", async () => {
    vi.spyOn(Connection.prototype, "getAccountInfo").mockRejectedValue(
      new Error(
        "HTTP status server error (503 Service Unavailable) for url (https://devnet.helius-rpc.com/?api-key=secret-value)",
      ),
    );

    const response = await app.request(
      `/v1/spl/balance?address=${owner}&mint=So11111111111111111111111111111111111111112`,
      {},
      env,
    );

    expect(response.status).toBe(502);

    const json = (await response.json()) as {
      error: {
        details?: {
          message?: string;
        };
      };
    };

    expect(json.error.details?.message).toContain("[redacted-url]");
    expect(json.error.details?.message).not.toContain(
      "https://devnet.helius-rpc.com/",
    );
    expect(json.error.details?.message).not.toContain("api-key=secret-value");
  });

  it("returns a clearer validation error when required balance query params are missing", async () => {
    const response = await app.request("/v1/spl/balance", {}, env);

    expect(response.status).toBe(422);

    const json = (await response.json()) as {
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
    expect(json.error.issues).toEqual(
      expect.arrayContaining([
        expect.objectContaining({
          path: ["address"],
          message: "address is required",
        }),
        expect.objectContaining({
          path: ["mint"],
          message: "mint is required",
        }),
      ]),
    );
  });

  it("returns initialized=true when the mint transfer queue exists", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(validator),
    );
    const fetchSpy = vi.spyOn(globalThis, "fetch");

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(env.BASE_RPC_URL);
        expect(address.toBase58()).toBe(transferQueue.toBase58());
        return {
          data: Buffer.alloc(64),
          executable: false,
          lamports: 1,
          owner: DELEGATION_PROGRAM_ID,
          rentEpoch: 0,
        };
      },
    );

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      {},
      env,
    );

    expect(response.status).toBe(200);
    expect(fetchSpy).not.toHaveBeenCalled();

    const json = (await response.json()) as {
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
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(validator),
    );

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(env.BASE_RPC_URL);
        expect(address.toBase58()).toBe(transferQueue.toBase58());
        return null;
      },
    );

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
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
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(resolvedValidator),
    );

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(mintInitializationEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(mintInitializationEnv.BASE_RPC_URL);
        expect(address.toBase58()).toBe(transferQueue.toBase58());
        return {
          data: Buffer.alloc(64),
          executable: false,
          lamports: 1,
          owner: DELEGATION_PROGRAM_ID,
          rentEpoch: 0,
        };
      },
    );

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}`,
      {},
      mintInitializationEnv,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
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
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(validator),
    );

    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;
        expect(endpoint).toBe(env.BASE_RPC_URL);
        expect(address.toBase58()).toBe(transferQueue.toBase58());
        return {
          data: Buffer.alloc(64),
          executable: false,
          lamports: 1,
          owner: EPHEMERAL_SPL_TOKEN_PROGRAM_ID,
          rentEpoch: 0,
        };
      },
    );

    const response = await app.request(
      `/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      {},
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      initialized: boolean;
    };

    expect(json.initialized).toBe(false);
  });

  it("skips the background crank when recent transfer queue activity exists", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(validator),
    );
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    const getLatestBlockhashSpy = vi.spyOn(
      Connection.prototype,
      "getLatestBlockhash",
    );
    const sendRawTransactionSpy = vi.spyOn(
      Connection.prototype,
      "sendRawTransaction",
    );

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(
      Connection.prototype,
      "getSignaturesForAddress",
    ).mockImplementation(async function getSignaturesForAddress(
      this: Connection & { _rpcEndpoint: string },
      address,
    ) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
        env.EPHEMERAL_RPC_URL,
      );
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return [
        {
          blockTime: Math.floor((nowMs - 30_000) / 1000),
          confirmationStatus: "confirmed",
          err: null,
          memo: null,
          signature: "recent-signature",
          slot: 1,
        },
      ];
    });

    const response = await app.fetch(
      new Request(
        `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      ),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(getLatestBlockhashSpy).not.toHaveBeenCalled();
    expect(sendRawTransactionSpy).not.toHaveBeenCalled();
  });

  it("forces the transfer queue crank endpoint even when recent activity exists", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const validatorPublicKey = new PublicKey(validator);
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      validatorPublicKey,
    );
    const nowMs = 1_700_000_000_000;
    let rawTransaction: Buffer | undefined;

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          env.BASE_RPC_URL,
        );
        expect(address.toBase58()).toBe(transferQueue.toBase58());
        return createQueueAccountInfo(DELEGATION_PROGRAM_ID);
      },
    );
    vi.spyOn(
      Connection.prototype,
      "getSignaturesForAddress",
    ).mockImplementation(async function getSignaturesForAddress(
      this: Connection & { _rpcEndpoint: string },
      address,
    ) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
        env.EPHEMERAL_RPC_URL,
      );
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return [
        {
          blockTime: Math.floor((nowMs - 30_000) / 1000),
          confirmationStatus: "confirmed",
          err: null,
          memo: null,
          signature: "recent-signature",
          slot: 1,
        },
      ];
    });
    vi.spyOn(
      Connection.prototype,
      "getLatestBlockhashAndContext",
    ).mockResolvedValue({
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
    vi.spyOn(Connection.prototype, "sendRawTransaction").mockImplementation(
      async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
        raw,
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          env.EPHEMERAL_RPC_URL,
        );
        rawTransaction = Buffer.from(raw);
        return "forced-crank-signature";
      },
    );
    vi.spyOn(Connection.prototype, "confirmTransaction").mockResolvedValue({
      context: { slot: 1 },
      value: { err: null },
    } as never);

    const response = await app.request(
      "/v1/spl/transfer-queue/ensure-crank",
      {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ mint, validator }),
      },
      env,
    );

    expect(response.status).toBe(200);

    const json = (await response.json()) as {
      mint: string;
      validator: string;
      transferQueue: string;
      crankSignature: string;
    };

    expect(json).toEqual({
      mint,
      validator,
      transferQueue: transferQueue.toBase58(),
      crankSignature: "forced-crank-signature",
    });
    expect(rawTransaction).toBeDefined();

    const transaction = Transaction.from(rawTransaction!);
    expect(transaction.instructions).toHaveLength(1);
    expect(transaction.instructions[0]?.keys[1]?.pubkey.toBase58()).toBe(
      transferQueue.toBase58(),
    );
    expect(transaction.instructions[0]?.keys[2]?.pubkey.toBase58()).toBe(
      magicFeeVaultPdaFromValidator(validatorPublicKey).toBase58(),
    );
  });

  it("does not fail mint initialization when transfer queue activity is auth-filtered", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const executionCtx = createExecutionContext();
    const sendRawTransactionSpy = vi.spyOn(
      Connection.prototype,
      "sendRawTransaction",
    );

    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockRejectedValue(
      new Error("Missing token query param"),
    );

    const response = await app.fetch(
      new Request(
        `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      ),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);
    await executionCtx.drain();

    const json = (await response.json()) as {
      initialized: boolean;
    };

    expect(json.initialized).toBe(true);
    expect(sendRawTransactionSpy).not.toHaveBeenCalled();
  });

  it("forces the transfer queue crank every 100 auth-filtered activity checks", async () => {
    const crankEnv = {
      ...env,
      TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL: "https://crank.devnet.rpc.test",
    };
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const validatorPublicKey = new PublicKey(validator);
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      validatorPublicKey,
    );
    const executionCtx = createExecutionContext();
    const sendRawTransactionSpy = vi
      .spyOn(Connection.prototype, "sendRawTransaction")
      .mockImplementation(async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          crankEnv.TRANSFER_QUEUE_DEVNET_CRANK_RPC_URL,
        );
        return "background-crank-signature";
      });

    vi.spyOn(console, "warn").mockImplementation(() => { });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockImplementation(
      async function getSignaturesForAddress(
        this: Connection & { _rpcEndpoint: string },
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          crankEnv.EPHEMERAL_DEVNET_RPC_URL,
        );
        throw new Error("401 Unauthorized: Missing token query param");
      },
    );
    vi.spyOn(
      Connection.prototype,
      "getLatestBlockhashAndContext",
    ).mockResolvedValue({
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

    for (let index = 0; index < 100; index += 1) {
      const response = await app.fetch(
        new Request(
          `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}&cluster=devnet`,
        ),
        crankEnv,
        executionCtx,
      );

      expect(response.status).toBe(200);
      await executionCtx.drain();
    }

    expect(sendRawTransactionSpy).toHaveBeenCalledOnce();
    const rawTransaction = sendRawTransactionSpy.mock.calls[0]?.[0];
    expect(rawTransaction).toBeDefined();

    const transaction = Transaction.from(Buffer.from(rawTransaction as Uint8Array));
    expect(transaction.instructions).toHaveLength(1);
    expect(transaction.instructions[0]?.keys[1]?.pubkey.toBase58()).toBe(
      transferQueue.toBase58(),
    );
    expect(transaction.instructions[0]?.keys[2]?.pubkey.toBase58()).toBe(
      magicFeeVaultPdaFromValidator(validatorPublicKey).toBase58(),
    );
  });

  it("uses a throwaway auth token when auth-filtered activity checks force the devnet crank", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const executionCtx = createExecutionContext();
    const expectedCrankRpcEndpoint = `${env.EPHEMERAL_DEVNET_RPC_URL}/?token=throwaway-crank-token`;
    const fetchSpy = vi.spyOn(globalThis, "fetch").mockImplementation(async (input, init) => {
      const url = String(input);

      if (url.startsWith(`${env.EPHEMERAL_DEVNET_RPC_URL}/auth/challenge?pubkey=`)) {
        return new Response(
          JSON.stringify({ challenge: "transfer queue crank challenge" }),
          {
            status: 200,
            headers: { "content-type": "application/json" },
          },
        );
      }

      if (url === `${env.EPHEMERAL_DEVNET_RPC_URL}/auth/login`) {
        expect(init?.method).toBe("POST");
        const body = JSON.parse(String(init?.body)) as {
          challenge?: string;
          pubkey?: string;
          signature?: string;
        };
        expect(body.challenge).toBe("transfer queue crank challenge");
        expect(body.pubkey).toEqual(expect.any(String));
        expect(body.signature).toEqual(expect.any(String));

        return new Response(
          JSON.stringify({
            token: "throwaway-crank-token",
            expiresAt: 1_800_000_000_000,
          }),
          {
            status: 200,
            headers: { "content-type": "application/json" },
          },
        );
      }

      throw new Error(`Unexpected fetch ${url}`);
    });
    const sendRawTransactionSpy = vi
      .spyOn(Connection.prototype, "sendRawTransaction")
      .mockImplementation(async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          expectedCrankRpcEndpoint,
        );
        return "background-crank-signature";
      });

    vi.spyOn(console, "warn").mockImplementation(() => { });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockImplementation(
      async function getSignaturesForAddress(
        this: Connection & { _rpcEndpoint: string },
      ) {
        expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
          env.EPHEMERAL_DEVNET_RPC_URL,
        );
        throw new Error("401 Unauthorized: Missing token query param");
      },
    );
    vi.spyOn(
      Connection.prototype,
      "getLatestBlockhashAndContext",
    ).mockImplementation(async function getLatestBlockhashAndContext(
      this: Connection & { _rpcEndpoint: string },
    ) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
        expectedCrankRpcEndpoint,
      );
      return {
        context: { slot: 1 },
        value: {
          blockhash: "11111111111111111111111111111111",
          lastValidBlockHeight: 123,
        },
      };
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

    for (let index = 0; index < 100; index += 1) {
      const response = await app.fetch(
        new Request(
          `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}&cluster=devnet`,
        ),
        env,
        executionCtx,
      );

      expect(response.status).toBe(200);
      await executionCtx.drain();
    }

    expect(sendRawTransactionSpy).toHaveBeenCalledOnce();
    expect(fetchSpy).toHaveBeenCalledTimes(2);
  });

  it("starts the background crank when recent transfer queue signatures failed", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    const sendRawTransactionSpy = vi
      .spyOn(Connection.prototype, "sendRawTransaction")
      .mockResolvedValue("background-crank-signature");

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockResolvedValue([
      {
        blockTime: Math.floor((nowMs - 30_000) / 1000),
        confirmationStatus: "confirmed",
        err: "InvalidWritableAccount",
        memo: null,
        signature: "recent-failed-signature",
        slot: 1,
      },
    ]);
    vi.spyOn(
      Connection.prototype,
      "getLatestBlockhashAndContext",
    ).mockResolvedValue({
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
      new Request(
        `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      ),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(sendRawTransactionSpy).toHaveBeenCalledOnce();
  });

  it("starts the background crank when the transfer queue has no recent transactions", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const validatorPublicKey = new PublicKey(validator);
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      validatorPublicKey,
    );
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    let rawTransaction: Buffer | undefined;

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockResolvedValue(
      [],
    );
    vi.spyOn(
      Connection.prototype,
      "getLatestBlockhashAndContext",
    ).mockResolvedValue({
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
    vi.spyOn(Connection.prototype, "sendRawTransaction").mockImplementation(
      async function sendRawTransaction(
        this: Connection & { _rpcEndpoint: string },
        raw,
      ) {
        expect(
          (this as Connection & { _rpcEndpoint: string })._rpcEndpoint,
        ).toBe(env.EPHEMERAL_RPC_URL);
        rawTransaction = Buffer.from(raw);
        return "background-crank-signature";
      },
    );
    vi.spyOn(Connection.prototype, "confirmTransaction").mockResolvedValue({
      context: { slot: 1 },
      value: { err: null },
    } as never);

    const response = await app.fetch(
      new Request(
        `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      ),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(rawTransaction).toBeDefined();

    const transaction = Transaction.from(rawTransaction!);
    expect(transaction.instructions).toHaveLength(1);
    expect(transaction.instructions[0]?.keys[1]?.pubkey.toBase58()).toBe(
      transferQueue.toBase58(),
    );
    expect(transaction.instructions[0]?.keys[2]?.pubkey.toBase58()).toBe(
      magicFeeVaultPdaFromValidator(validatorPublicKey).toBase58(),
    );
  });

  it("starts the background crank when the latest transfer queue transaction is older than a minute", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(
      new PublicKey(mint),
      new PublicKey(validator),
    );
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    const sendRawTransactionSpy = vi
      .spyOn(Connection.prototype, "sendRawTransaction")
      .mockResolvedValue("background-crank-signature");

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(
      createQueueAccountInfo(DELEGATION_PROGRAM_ID),
    );
    vi.spyOn(
      Connection.prototype,
      "getSignaturesForAddress",
    ).mockImplementation(async function getSignaturesForAddress(
      this: Connection & { _rpcEndpoint: string },
      address,
    ) {
      expect((this as Connection & { _rpcEndpoint: string })._rpcEndpoint).toBe(
        env.EPHEMERAL_RPC_URL,
      );
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return [
        {
          blockTime: Math.floor((nowMs - 61_000) / 1000),
          confirmationStatus: "confirmed",
          err: null,
          memo: null,
          signature: "stale-signature",
          slot: 1,
        },
      ];
    });
    vi.spyOn(
      Connection.prototype,
      "getLatestBlockhashAndContext",
    ).mockResolvedValue({
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
      new Request(
        `http://localhost/v1/spl/is-mint-initialized?mint=${mint}&validator=${validator}`,
      ),
      env,
      executionCtx,
    );

    expect(response.status).toBe(200);

    await executionCtx.drain();

    expect(sendRawTransactionSpy).toHaveBeenCalledOnce();
  });

  it("uses a custom RPC URL only for base RPC calls when cluster is a URL", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const customRpcEnv = {
      ...env,
      EPHEMERAL_RPC_URL: "https://ephemeral.custom-balance.rpc.test",
    };
    const delegationRecord = deriveEataDelegationRecord(owner, mint);
    const validator = new PublicKey(resolvedValidator);

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(customRpcEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(
      async function getAccountInfo(
        this: Connection & { _rpcEndpoint: string },
        address,
      ) {
        const endpoint = (this as Connection & { _rpcEndpoint: string })
          ._rpcEndpoint;

        if (address.toBase58() === delegationRecord.toBase58()) {
          expect(endpoint).toBe("https://custom.rpc.test/");
          return createDelegationAccountInfo(validator);
        }

        return endpoint.includes("custom.rpc.test")
          ? createAccountInfo(7n)
          : endpoint.includes("ephemeral")
            ? createAccountInfo(9n)
            : createAccountInfo(0n);
      },
    );

    const baseResponse = await app.request(
      `/v1/spl/balance?address=${owner}&mint=${mint}&cluster=${encodeURIComponent("https://custom.rpc.test")}`,
      {},
      customRpcEnv,
    );
    const privateResponse = await app.request(
      `/v1/spl/private-balance?address=${owner}&mint=${mint}&cluster=${encodeURIComponent("https://custom.rpc.test")}`,
      { headers: { authorization: "Bearer 1234567890" } },
      customRpcEnv,
    );

    expect(baseResponse.status).toBe(200);
    expect(privateResponse.status).toBe(200);

    const baseJson = (await baseResponse.json()) as {
      location: string;
      balance: string;
    };
    const privateJson = (await privateResponse.json()) as {
      location: string;
      balance: string;
    };

    expect(baseJson.location).toBe("base");
    expect(baseJson.balance).toBe("7");
    expect(privateJson.location).toBe("ephemeral");
    expect(privateJson.balance).toBe("9");
  });
});
