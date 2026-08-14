// eslint 10 flat config（typescript-eslint recommended）。
// 不含 prettier：格式化交给编辑器，避免全量 diff；若日后统一再加。
import { defineConfig } from "eslint/config";
import tseslint from "typescript-eslint";

export default defineConfig([
  {
    ignores: ["dist/**", "src/lib/types.gen.ts"],
  },
  ...tseslint.configs.recommended,
]);
