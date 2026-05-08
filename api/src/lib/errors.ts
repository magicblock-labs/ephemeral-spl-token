import { z } from "@hono/zod-openapi";
import type { ZodError, ZodIssue } from "zod";
import { errorPayloadSchema, validationIssueSchema } from "../schema";

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

export const errorResponseSchema = z.object({
  error: errorPayloadSchema,
}).openapi("ErrorResponse");
export type ErrorResponse = z.infer<typeof errorResponseSchema>;

export const validationErrorResponseSchema = z.object({
  error: z.object({
    code: z.literal("VALIDATION_ERROR"),
    message: z.string(),
    issues: z.array(validationIssueSchema),
  }),
}).openapi("ValidationErrorResponse");
export type ValidationErrorResponse = z.infer<typeof validationErrorResponseSchema>;

export const notFoundResponseSchema = z.object({
  error: z.object({
    code: z.literal("NOT_FOUND"),
    message: z.string(),
  }),
}).openapi("NotFoundResponse");
export type NotFoundResponse = z.infer<typeof notFoundResponseSchema>;

export function errorBody(
  code: string,
  message: string,
  details?: unknown,
): ErrorResponse {
  return details === undefined
    ? { error: { code, message } }
    : { error: { code, message, details } };
}

function stringifyIssuePath(path: ZodIssue["path"]) {
  return path
    .map((segment: string | number | symbol) => typeof segment === "number" ? segment : String(segment))
    .join(".");
}

function isMissingRequiredIssue(issue: ZodIssue) {
  return issue.code === "invalid_type" && issue.message.includes("received undefined");
}

export function validationErrorBody(error: ZodError): ValidationErrorResponse {
  const issues = error.issues.map((issue: ZodIssue) => {
    const path = issue.path.map((segment: string | number | symbol) => typeof segment === "number" ? segment : String(segment));
    const pathLabel = stringifyIssuePath(issue.path);

    return {
      code: issue.code,
      message: isMissingRequiredIssue(issue) && pathLabel
        ? `${pathLabel} is required`
        : issue.message,
      path,
    };
  });

  const missingRequiredPaths = issues
    .filter(issue => issue.code === "invalid_type" && issue.message.endsWith("is required") && issue.path.length > 0)
    .map(issue => issue.path.join("."));

  return {
    error: {
      code: "VALIDATION_ERROR",
      message: missingRequiredPaths.length > 0
        ? `Missing required fields: ${missingRequiredPaths.join(", ")}`
        : "Request validation failed",
      issues,
    },
  };
}
