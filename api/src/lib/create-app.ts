import { OpenAPIHono } from "@hono/zod-openapi";
import { cors } from "hono/cors";

import type { AppBindings } from "../env";
import {
  ApiError,
  errorBody,
  validationErrorBody,
} from "./errors";

export default function createApp() {
  const app = new OpenAPIHono<{ Bindings: AppBindings }>({
    defaultHook: (result, c) => {
      if (!result.success) {
        return c.json(validationErrorBody(result.error), 422);
      }
    },
  });

  app.use("*", async (c, next) => {
    const origin = c.env.CORS_ORIGIN?.trim() || "*";
    return cors({ origin })(c, next);
  });

  app.notFound((c) => c.json(errorBody("NOT_FOUND", "Route not found"), 404));

  app.onError((error, c) => {
    if (error instanceof ApiError) {
      return c.newResponse(
        JSON.stringify(errorBody(error.code, error.message, error.details)),
        error.status as 400 | 404 | 422 | 500 | 502,
        {
          "content-type": "application/json",
        },
      );
    }

    console.error(error);
    return c.json(errorBody("INTERNAL_ERROR", "Unexpected server error"), 500);
  });

  return app;
}
