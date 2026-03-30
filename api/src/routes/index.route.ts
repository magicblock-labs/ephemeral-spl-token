import { createRoute, OpenAPIHono, z } from "@hono/zod-openapi";

import type { AppBindings } from "../env";
import { openApiDefaultHook } from "../lib/create-app";
import { jsonContent } from "../lib/openapi";

const tags = ["Meta"];

const healthResponseSchema = z.object({
  status: z.literal("ok"),
}).openapi("HealthResponse");

const healthRoute = createRoute({
  path: "/health",
  method: "get",
  tags,
  responses: {
    200: jsonContent(healthResponseSchema, "Health check"),
  },
});

const app = new OpenAPIHono<{ Bindings: AppBindings }>({
  defaultHook: openApiDefaultHook,
});

app.get("/", (c) => c.redirect("/reference"));

app.openapi(healthRoute, (c) => c.json({ status: "ok" }));

export default app;
