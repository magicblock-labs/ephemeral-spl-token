import { afterEach, describe, expect, it, vi } from "vitest";
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StreamableHTTPClientTransport } from "@modelcontextprotocol/sdk/client/streamableHttp.js";
import {
  deriveEphemeralAta,
  deriveRentPda,
  deriveTransferQueue,
  deriveVault,
  deriveVaultAta,
  magicFeeVaultPdaFromValidator,
} from "@magicblock-labs/ephemeral-rollups-sdk";
import {
  AccountInfo,
  Connection,
  Keypair,
  LAMPORTS_PER_SOL,
  PublicKey,
  SystemInstruction,
  SystemProgram,
  Transaction,
} from "@solana/web3.js";

import app from "./app";
import { TOKEN_PROGRAM_ID } from "./lib/solana";

const env = {
  BASE_RPC_URL: "https://base.rpc.test",
  EPHEMERAL_RPC_URL: "https://ephemeral.rpc.test",
  BASE_DEVNET_RPC_URL: "https://base.devnet.rpc.test",
  EPHEMERAL_DEVNET_RPC_URL: "https://ephemeral.devnet.rpc.test",
  CORS_ORIGIN: "*",
};

const DEVNET_USDC_MINT = "4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU";
const owner = Keypair.generate().publicKey.toBase58();
const destination = Keypair.generate().publicKey.toBase58();
const resolvedValidator = Keypair.generate().publicKey.toBase58();

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
    passThroughOnException() {},
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

    const json = await response.json() as {
      paths: Record<string, {
        get?: unknown;
        post?: {
          requestBody?: {
            content?: Record<string, {
              schema?: unknown;
            }>;
          };
          responses?: Record<string, {
            content?: Record<string, {
              example?: {
                kind?: string;
                instructionCount?: number;
              };
            }>;
          }>;
        };
      }>;
    };
    expect(json.paths["/v1/spl/deposit"]).toBeDefined();
    expect(json.paths["/mcp"]?.post).toBeDefined();
    expect(json.paths["/mcp"]?.get).toBeUndefined();
    expect(json.paths["/.well-known/mcp.json"]).toBeUndefined();
    expect(json.paths["/mcp"]?.post?.requestBody?.content?.["application/json"]?.schema).toBeDefined();
    expect(json.paths["/v1/spl/private-balance"]).toBeDefined();
    expect(json.paths["/v1/spl/is-mint-initialized"]).toBeDefined();
    expect(json.paths["/v1/spl/initialize-mint"]).toBeDefined();
    expect(json.paths["/v1/spl/deposit"]?.post?.responses?.["200"]?.content?.["application/json"]?.example).toMatchObject({
      kind: "deposit",
      instructionCount: 3,
    });
    expect(json.paths["/v1/spl/withdraw"]?.post?.responses?.["200"]?.content?.["application/json"]?.example).toMatchObject({
      kind: "withdraw",
      instructionCount: 2,
    });
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
  });

  it("builds an initialize mint transaction with the expected queue setup instructions", async () => {
    const validator = Keypair.generate().publicKey.toBase58();
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
    expect(json.instructionCount).toBe(2);
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
    };

    expect(json.sendTo).toBe("base");
    expect(json.recentBlockhash).toBe("11111111111111111111111111111111");
    expect(json.validator).toBe(resolvedValidator);
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

    expect(json.instructionCount).toBe(3);
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
      {},
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
      expect(endpoint).toBe(env.EPHEMERAL_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return createAccountInfo(0n);
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
      expect(endpoint).toBe(env.EPHEMERAL_RPC_URL);
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
      EPHEMERAL_RPC_URL: "https://ephemeral.mint-init.rpc.test",
    };
    const mint = "So11111111111111111111111111111111111111112";
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(resolvedValidator));

    vi.spyOn(globalThis, "fetch").mockImplementation(async (input) => {
      expect(String(input)).toBe(mintInitializationEnv.EPHEMERAL_RPC_URL);
      return createIdentityResponse(resolvedValidator);
    });
    vi.spyOn(Connection.prototype, "getAccountInfo").mockImplementation(async function getAccountInfo(this: Connection & { _rpcEndpoint: string }, address) {
      const endpoint = (this as Connection & { _rpcEndpoint: string })._rpcEndpoint;
      expect(endpoint).toBe(mintInitializationEnv.EPHEMERAL_RPC_URL);
      expect(address.toBase58()).toBe(transferQueue.toBase58());
      return createAccountInfo(0n);
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

  it("skips the background crank when recent transfer queue activity exists", async () => {
    const mint = "So11111111111111111111111111111111111111112";
    const validator = Keypair.generate().publicKey.toBase58();
    const [transferQueue] = deriveTransferQueue(new PublicKey(mint), new PublicKey(validator));
    const executionCtx = createExecutionContext();
    const nowMs = 1_700_000_000_000;
    const getLatestBlockhashSpy = vi.spyOn(Connection.prototype, "getLatestBlockhash");
    const sendRawTransactionSpy = vi.spyOn(Connection.prototype, "sendRawTransaction");

    vi.spyOn(Date, "now").mockReturnValue(nowMs);
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createAccountInfo(0n));
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
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createAccountInfo(0n));
    vi.spyOn(Connection.prototype, "getSignaturesForAddress").mockResolvedValue([]);
    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
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
    vi.spyOn(Connection.prototype, "getAccountInfo").mockResolvedValue(createAccountInfo(0n));
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
    vi.spyOn(Connection.prototype, "getLatestBlockhash").mockResolvedValue({
      blockhash: "11111111111111111111111111111111",
      lastValidBlockHeight: 123,
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
      {},
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
