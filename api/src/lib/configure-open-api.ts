import { apiReference } from "@scalar/hono-api-reference";
import type { OpenAPIHono } from "@hono/zod-openapi";

import type { AppBindings } from "../env";

const MAGICBLOCK_LOGO_URL = "https://cdn.prod.website-files.com/67dd3f471f62a240dd544dd8/682efe2b89d00ecb838fa333_Frame%2085.svg";
const MAGICBLOCK_DOCS_FAVICON_URL = "https://docs.magicblock.gg/mintlify-assets/_mintlify/favicons/magicblock-42/U_0PfsrxUNdGUMiY/_generated/favicon-dark/favicon.ico";
const MCP_INITIALIZE_REQUEST_EXAMPLE = {
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

const MAGICBLOCK_CUSTOM_CSS = `
  body::before {
    content: "";
    position: fixed;
    left: 0;
    bottom: 0;
    width: 170px;
    height: 32px;
    background-color: rgba(56, 56, 56, 0.98);
    background-image: url("${MAGICBLOCK_LOGO_URL}");
    background-repeat: no-repeat;
    background-position: center;
    background-size: 126px auto;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
    border-right: 1px solid rgba(255, 255, 255, 0.07);
    border-radius: 0 8px 0 0;
    box-shadow:
      0 4px 12px rgba(0, 0, 0, 0.22),
      inset 0 1px 0 rgba(255, 255, 255, 0.06);
    filter: drop-shadow(0 4px 10px rgba(0, 0, 0, 0.14));
    pointer-events: none;
    z-index: 20;
  }

  @media (max-width: 1023px) {
    body::before {
      left: 0;
      bottom: 0;
      width: 158px;
      height: 30px;
      background-size: 116px auto;
    }
  }
`;

export default function configureOpenAPI(app: OpenAPIHono<{ Bindings: AppBindings }>) {
  const openApiConfig = {
    openapi: "3.1.0" as const,
    info: {
      title: "SPL Private Payments API",
      version: "0.1.0",
      description: "REST API for building private SPL token transactions on Solana and MagicBlock ephemeral rollups.\n\n"
        + "Documentation: https://docs.magicblock.gg/pages/private-ephemeral-rollups-pers/how-to-guide/quickstart",
    },
    tags: [
      {
        name: "Swap",
        description: "Provide quoting and execution for public and private swaps.",
      },
    ],
  };

  app.get("/doc", (c) => {
    const document = app.getOpenAPI31Document(openApiConfig);
    const mcpPost = document.paths?.["/mcp"]?.post;

    if (mcpPost) {
      mcpPost.requestBody = {
        required: true,
        description: "MCP JSON-RPC request",
        content: {
          "application/json": {
            schema: {
              type: "object",
              additionalProperties: true,
              properties: {
                jsonrpc: {
                  type: "string",
                  enum: ["2.0"],
                },
                id: {
                  anyOf: [
                    { type: "string" },
                    { type: "number" },
                    { type: "null" },
                  ],
                },
                method: {
                  type: "string",
                },
                params: {},
              },
              required: ["jsonrpc", "method"],
            },
            example: MCP_INITIALIZE_REQUEST_EXAMPLE,
          },
        },
      };
    }

    return c.json(document);
  });

  app.get("/reference", apiReference({
    url: "/doc",
    pageTitle: "MagicBlock | SPL Private Payments API",
    favicon: MAGICBLOCK_DOCS_FAVICON_URL,
    defaultOpenAllTags: true,
    customCss: MAGICBLOCK_CUSTOM_CSS,
  }));
}
