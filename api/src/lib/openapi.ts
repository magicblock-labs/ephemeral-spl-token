import type { ZodTypeAny } from "zod";

export function jsonContent(schema: ZodTypeAny, description: string, example?: unknown) {
  return {
    content: {
      "application/json": {
        schema,
        ...(example === undefined ? {} : { example }),
      },
    },
    description,
  };
}

export function jsonContentRequired(schema: ZodTypeAny, description: string, example?: unknown) {
  return {
    required: true,
    content: {
      "application/json": {
        schema,
        ...(example === undefined ? {} : { example }),
      },
    },
    description,
  };
}
