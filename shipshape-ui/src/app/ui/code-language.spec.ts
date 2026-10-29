/**
 * @file Unit tests for {@link resolveLanguageExtension}.
 */

import { resolveLanguageExtension } from "./code-language";

describe("resolveLanguageExtension", () => {
  it("returns non-empty extensions for all supported languages", () => {
    const supported = [
      "ts",
      "typescript",
      "js",
      "javascript",
      "json",
      "html",
      "css",
      "md",
      "markdown",
      "rust",
      "yaml",
      "yml",
    ];
    for (const lang of supported) {
      const ext = resolveLanguageExtension(lang);
      expect(Array.isArray(ext)).toBeFalse();
    }
  });

  it("returns an empty array fallback for unknown languages", () => {
    const extension = resolveLanguageExtension("unknown");
    expect(Array.isArray(extension)).toBeTrue();
    expect((extension as readonly unknown[]).length).toBe(0);
  });
});
