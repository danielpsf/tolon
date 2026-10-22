import js from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import globals from "globals";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist", "node_modules"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  reactHooks.configs.flat.recommended,
  { files: ["src/**/*.{ts,tsx}"], languageOptions: { globals: globals.browser } },
  { files: ["*.{ts,js}", "scripts/*.mjs", "help/*.mjs"], languageOptions: { globals: globals.node } },
  // The e2e script runs in Node but passes functions to the page via executeAsyncScript.
  { files: ["e2e/*.mjs"], languageOptions: { globals: { ...globals.node, ...globals.browser } } },
);
