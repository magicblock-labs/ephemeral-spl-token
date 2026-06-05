import { createRoute, OpenAPIHono, z } from "@hono/zod-openapi";
import { McpServer } from "@modelcontextprotocol/sdk/server/mcp.js";
import { WebStandardStreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/webStandardStreamableHttp.js";

import { getEnv, type AppBindings, type AppEnv } from "../env";
import { openApiDefaultHook } from "../lib/create-app";
import {
  jsonContent,
} from "../lib/openapi";
import {
  buildDepositTransaction,
  buildTransferTransaction,
  buildWithdrawTransaction,
  getBaseBalance,
  getPrivateBalance,
} from "../lib/solana";
import {
  balanceRequestSchema,
  balanceResponseSchema,
  depositRequestSchema,
  transactionResponseSchema,
  transferRequestSchema,
  withdrawRequestSchema,
} from "./spl/spl.schemas";

const tags = ["MCP"];
const mcpTools = [
  {
    name: "spl.deposit",
    description: "Build an unsigned base-chain deposit transaction using delegateSpl(...).",
  },
  {
    name: "spl.withdraw",
    description: "Withdraw SPL tokens from an ephemeral rollup back to Solana.",
  },
  {
    name: "spl.transfer",
    description: "Build an unsigned SPL transfer transaction using transferSpl(...).",
  },
  {
    name: "spl.getBalance",
    description: "Read the owner ATA balance on the base RPC.",
  },
  {
    name: "spl.getPrivateBalance",
    description: "Read the private balance when the eATA is delegated to the ephemeral RPC.",
  },
] as const;

const mcpInitializeRequestExample = {
  jsonrpc: "2.0",
  id: 1,
  method: "initialize",
  params: {
    protocolVersion: "2025-11-25",
    capabilities: {},
    clientInfo: {
      name: "curl-example",
      version: "1.0.0",
    },
  },
};

const mcpInitializeResponseExample = {
  jsonrpc: "2.0",
  id: 1,
  result: {
    protocolVersion: "2025-11-25",
    capabilities: {
      tools: {
        listChanged: true,
      },
    },
    serverInfo: {
      name: "spl-private-payments-api",
      version: "0.1.0",
    },
  },
};

const mcpToolCallRequestExample = {
  jsonrpc: "2.0",
  id: 3,
  method: "tools/call",
  params: {
    name: "spl.deposit",
    arguments: {
      owner: "3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE",
      amount: 1,
      initIfMissing: true,
      initAtasIfMissing: true,
      initVaultIfMissing: false,
      idempotent: true,
    },
  },
};

const mcpToolCallResponseExample = {
  jsonrpc: "2.0",
  id: 3,
  result: {
    content: [
      {
        type: "text",
        text: "Built an unsigned SPL deposit transaction.",
      },
    ],
    structuredContent: {
      kind: "deposit",
      version: "legacy",
      transactionBase64: "AQAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABAAIDKmcfsS5XfSOLaLlaBHJry50iH2Ufk2TMz4STC2fHzIcFKkerg3q2DD3Yn8TISmGeKoxSLz+BiP7iQ4pYqXYXsgu8D8C7R8ovdMQRLpSrE8+jxjTl3BfqywPNGiPNfnh8eS+smowIxqKDcCjw5liNXQkkCbBSDCBDFwtrgCKqoQ0DAgEBBAECAwQCAQEEAgIDBAIBAQQDAgME",
      sendTo: "base",
      recentBlockhash: "9A4VhP8M8fQZxP4h7rB6mP6eM8w2pJkYh7QdZk7V4r2x",
      lastValidBlockHeight: 284512337,
      instructionCount: 3,
      requiredSigners: ["3rXKwQ1kpjBd5tdcco32qsvqUh1BnZjcYnS5kYrP7AYE"],
      validator: "MAS1Dt9qreoRMQ14YQuhg8UTZMMzDdKhmkZMECCzk57",
    },
  },
};

const mcpParseErrorExample = {
  jsonrpc: "2.0",
  error: {
    code: -32700,
    message: "Parse error",
  },
  id: null,
};

const mcpContentTypeErrorExample = {
  jsonrpc: "2.0",
  error: {
    code: -32000,
    message: "Unsupported Media Type: Content-Type must be application/json",
  },
  id: null,
};

const mcpInternalErrorExample = {
  jsonrpc: "2.0",
  error: {
    code: -32603,
    message: "Internal server error",
  },
  id: null,
};

const mcpResponseSchema = z.object({
  jsonrpc: z.literal("2.0"),
  id: z.union([z.string(), z.number(), z.null()]).optional(),
  result: z.any().optional(),
  error: z.object({
    code: z.number(),
    message: z.string(),
  }).passthrough().optional(),
}).passthrough().openapi("McpResponse", {
  example: mcpToolCallResponseExample,
});

const mcpRoute = createRoute({
  path: "/mcp",
  method: "post",
  tags,
  description: [
    "Stateless Streamable HTTP MCP endpoint.",
    "",
    "Implementation details:",
    "- Each `POST /mcp` request creates a fresh `McpServer`, handles the JSON-RPC exchange, then closes it.",
    "- The transport runs with `sessionIdGenerator: undefined` and `enableJsonResponse: true`, so this server does not issue or require `mcp-session-id` headers.",
    "- `GET /mcp` returns the info document and `GET /.well-known/mcp.json` returns the discovery document.",
    "- Clients can use `Accept: application/json` with `Content-Type: application/json`.",
    "",
    "Typical flow:",
    "1. `initialize`",
    "2. `notifications/initialized`",
    "3. `tools/list` or `tools/call`",
    "",
    "Initialize request example:",
    "```json",
    JSON.stringify(mcpInitializeRequestExample, null, 2),
    "```",
    "",
    "Initialize response example:",
    "```json",
    JSON.stringify(mcpInitializeResponseExample, null, 2),
    "```",
    "",
    "Tool call request example:",
    "```json",
    JSON.stringify(mcpToolCallRequestExample, null, 2),
    "```",
  ].join("\n"),
  responses: {
    200: jsonContent(mcpResponseSchema, "MCP JSON-RPC response", mcpToolCallResponseExample),
    202: {
      description: "Notification accepted; no response body is returned.",
    },
    400: jsonContent(mcpResponseSchema, "Invalid JSON or invalid JSON-RPC request", mcpParseErrorExample),
    415: jsonContent(mcpResponseSchema, "Content-Type must be application/json", mcpContentTypeErrorExample),
    500: jsonContent(mcpResponseSchema, "MCP JSON-RPC error response", mcpInternalErrorExample),
  },
});

function getMcpTools() {
  return mcpTools;
}

function createTextResult<T extends Record<string, unknown>>(text: string, structuredContent: T) {
  return {
    content: [
      {
        type: "text" as const,
        text,
      },
    ],
    structuredContent,
  };
}

function normalizeMcpRequest(request: Request) {
  const headers = new Headers(request.headers);
  const accept = headers.get("accept") || "";
  const contentType = headers.get("content-type") || "";
  const acceptedTypes = new Set(
    accept
      .split(",")
      .map(value => value.trim())
      .filter(Boolean),
  );

  acceptedTypes.add("application/json");
  acceptedTypes.add("text/event-stream");
  headers.set("accept", [...acceptedTypes].join(", "));

  if (!contentType || contentType.startsWith("text/plain")) {
    headers.set("content-type", "application/json");
  }

  if (request.bodyUsed) {
    return new Request(request.url, {
      method: request.method,
      headers,
    });
  }

  return new Request(request, { headers });
}

function buildMcpInfoDocument(requestUrl: string) {
  const url = new URL(requestUrl);
  const origin = url.origin;

  return {
    name: "spl-private-payments-api",
    version: "0.1.0",
    transport: "streamable-http",
    mode: "stateless-json-response",
    endpoint: `${origin}/mcp`,
    discovery: `${origin}/.well-known/mcp.json`,
    methods: ["POST"],
    inspector: {
      transport: "Streamable HTTP",
      url: `${origin}/mcp`,
    },
    tools: getMcpTools(),
  };
}

function buildMcpDiscoveryDocument(requestUrl: string) {
  const url = new URL(requestUrl);
  const origin = url.origin;

  return {
    name: "spl-private-payments-api",
    version: "0.1.0",
    description: "Stateless MCP server for building SPL token transactions and reading base or private balances.",
    websiteUrl: "https://www.magicblock.xyz/",
    documentationUrl: `${origin}/reference`,
    openApiUrl: `${origin}/doc`,
    transport: {
      type: "streamable-http",
      endpoint: `${origin}/mcp`,
      stateless: true,
      jsonResponse: true,
      methods: ["POST"],
    },
    capabilities: {
      tools: true,
      resources: false,
      prompts: false,
    },
    tools: getMcpTools(),
  };
}

function createMcpServer(env: AppEnv) {
  const server = new McpServer({
    name: "spl-private-payments-api",
    version: "0.1.0",
    websiteUrl: "https://www.magicblock.xyz/",
  });

  server.registerTool("spl.deposit", {
    title: "Deposit SPL",
    description: "Build an unsigned base-chain deposit transaction using delegateSpl(...).",
    inputSchema: depositRequestSchema,
    outputSchema: transactionResponseSchema,
  }, async (input) => {
    const response = await buildDepositTransaction(env, input);
    return createTextResult("Built an unsigned SPL deposit transaction.", response);
  });

  server.registerTool("spl.withdraw", {
    title: "Withdraw SPL",
    description: "Withdraw SPL tokens from an ephemeral rollup back to Solana.",
    inputSchema: withdrawRequestSchema,
    outputSchema: transactionResponseSchema,
  }, async (input) => {
    const response = await buildWithdrawTransaction(env, input);
    return createTextResult("Built an unsigned SPL withdraw transaction.", response);
  });

  server.registerTool("spl.transfer", {
    title: "Transfer SPL",
    description: "Build an unsigned SPL transfer transaction using transferSpl(...).",
    inputSchema: transferRequestSchema,
    outputSchema: transactionResponseSchema,
  }, async (input) => {
    const response = await buildTransferTransaction(env, input);
    return createTextResult("Built an unsigned SPL transfer transaction.", response);
  });

  server.registerTool("spl.getBalance", {
    title: "Get Base Balance",
    description: "Read the owner ATA balance on the base RPC.",
    inputSchema: balanceRequestSchema,
    outputSchema: balanceResponseSchema,
  }, async (input) => {
    const response = await getBaseBalance(env, input);
    return createTextResult("Fetched the base SPL balance.", response);
  });

  server.registerTool("spl.getPrivateBalance", {
    title: "Get Private Balance",
    description: "Read the private balance when the eATA is delegated to the ephemeral RPC.",
    inputSchema: balanceRequestSchema,
    outputSchema: balanceResponseSchema,
  }, async (input) => {
    const response = await getPrivateBalance(env, input);
    return createTextResult("Fetched the private SPL balance.", response);
  });

  return server;
}

const app = new OpenAPIHono<{ Bindings: AppBindings }>({
  defaultHook: openApiDefaultHook,
});

app.get("/mcp", (c) => {
  const accept = c.req.header("accept") || "";

  if (accept.includes("text/event-stream")) {
    return c.text("Method Not Allowed", 405, {
      Allow: "POST",
    });
  }

  return c.json(buildMcpInfoDocument(c.req.url));
});

app.openapi(mcpRoute, async (c) => {
  const env = getEnv(c.env);
  const server = createMcpServer(env);
  const transport = new WebStandardStreamableHTTPServerTransport({
    sessionIdGenerator: undefined,
    enableJsonResponse: true,
  });

  try {
    await server.connect(transport);
    const response = await transport.handleRequest(normalizeMcpRequest(c.req.raw));
    await server.close();
    return response;
  } catch (error) {
    console.error("Error handling MCP request:", error);
    await server.close().catch(() => undefined);

    return c.json({
      jsonrpc: "2.0",
      error: {
        code: -32603,
        message: "Internal server error",
      },
      id: null,
    }, 500);
  }
});

app.get("/.well-known/mcp.json", c => c.json(buildMcpDiscoveryDocument(c.req.url)));

app.delete("/mcp", c => c.text("Method Not Allowed", 405, {
  Allow: "POST",
}));

export default app;
