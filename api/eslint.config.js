import { defineConfig } from "eslint/config";
import stylistic from "@stylistic/eslint-plugin";
import tseslint from "typescript-eslint";

export default defineConfig(
  {
    ignores: [
      "coverage/**",
      "dist/**",
      "eslint.config.js",
      "node_modules/**",
      ".wrangler/**",
    ],
  },
  ...tseslint.configs.recommended,
  stylistic.configs["disable-legacy"],
  stylistic.configs.customize({
    indent: 2,
    quotes: "double",
    semi: true,
    braceStyle: "1tbs",
    commaDangle: "always-multiline",
    severity: "error",
  }),
  {
    files: ["src/**/*.ts", "vitest.config.ts"],
    rules: {
      "@typescript-eslint/no-explicit-any": "off",
      "@typescript-eslint/no-unused-vars": "error",
    },
  },
);
