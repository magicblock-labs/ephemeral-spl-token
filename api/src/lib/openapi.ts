import type { ZodTypeAny } from "zod";

/**
 * A named OpenAPI 3 example. Pass a map to `jsonContent(Required)` to render
 * a dropdown of cases in Scalar/Swagger UI. Regular `example` still works.
 */
export type OpenApiExamples = Record<
  string,
  { summary?: string; description?: string; value: unknown }
>;

function buildMedia(
  schema: ZodTypeAny,
  example?: unknown,
  examples?: OpenApiExamples,
) {
  if (examples !== undefined) {
    return { schema, examples };
  }

  if (example !== undefined) {
    return { schema, example };
  }

  return { schema };
}

export function jsonContent(
  schema: ZodTypeAny,
  description: string,
  example?: unknown,
  examples?: OpenApiExamples,
) {
  return {
    content: {
      "application/json": buildMedia(schema, example, examples),
    },
    description,
  };
}

export function jsonContentRequired(
  schema: ZodTypeAny,
  description: string,
  example?: unknown,
  examples?: OpenApiExamples,
) {
  return {
    required: true,
    content: {
      "application/json": buildMedia(schema, example, examples),
    },
    description,
  };
}
