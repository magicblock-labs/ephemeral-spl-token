import { z } from "@hono/zod-openapi";
import type { ZodError, ZodIssue } from "zod";

export class ApiError extends Error {
  status: number;
  code: string;
  details?: unknown;

  constructor(status: number, code: string, message: string, details?: unknown) {
    super(message);
    this.name = "ApiError";
    this.status = status;
    this.code = code;
    this.details = details;
  }
}

const validationIssueSchema = z.object({
  code: z.string(),
  message: z.string(),
  path: z.array(z.union([z.string(), z.number()])),
});

const errorPayloadSchema = z.object({
  code: z.string(),
  message: z.string(),
  details: z.unknown().optional(),
});

export const errorResponseSchema = z.object({
  error: errorPayloadSchema,
}).openapi("ErrorResponse");

export const validationErrorResponseSchema = z.object({
  error: z.object({
    code: z.literal("VALIDATION_ERROR"),
    message: z.string(),
    issues: z.array(validationIssueSchema),
  }),
}).openapi("ValidationErrorResponse");

export const notFoundResponseSchema = z.object({
  error: z.object({
    code: z.literal("NOT_FOUND"),
    message: z.string(),
  }),
}).openapi("NotFoundResponse");

export function errorBody(
  code: string,
  message: string,
  details?: unknown,
): z.infer<typeof errorResponseSchema> {
  return details === undefined
    ? { error: { code, message } }
    : { error: { code, message, details } };
}

export function validationErrorBody(error: ZodError): z.infer<typeof validationErrorResponseSchema> {
  return {
    error: {
      code: "VALIDATION_ERROR",
      message: "Request validation failed",
      issues: error.issues.map((issue: ZodIssue) => ({
        code: issue.code,
        message: issue.message,
        path: issue.path.map((segment: string | number | symbol) => typeof segment === "number" ? segment : String(segment)),
      })),
    },
  };
}
