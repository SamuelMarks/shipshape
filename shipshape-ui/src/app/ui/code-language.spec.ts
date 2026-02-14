import { resolveLanguageExtension } from "./code-language";

describe("resolveLanguageExtension", () => {
  it("returns extensions for supported languages", () => {
    expect(resolveLanguageExtension("typescript")).toBeTruthy();
    expect(resolveLanguageExtension("ts")).toBeTruthy();
    expect(resolveLanguageExtension("javascript")).toBeTruthy();
    expect(resolveLanguageExtension("js")).toBeTruthy();
    expect(resolveLanguageExtension("json")).toBeTruthy();
    expect(resolveLanguageExtension("html")).toBeTruthy();
    expect(resolveLanguageExtension("css")).toBeTruthy();
    expect(resolveLanguageExtension("markdown")).toBeTruthy();
    expect(resolveLanguageExtension("md")).toBeTruthy();
    expect(resolveLanguageExtension("rust")).toBeTruthy();
    expect(resolveLanguageExtension("yaml")).toBeTruthy();
    expect(resolveLanguageExtension("yml")).toBeTruthy();
  });

  it("returns a fallback for unknown languages", () => {
    const extension = resolveLanguageExtension("unknown");
    expect(Array.isArray(extension)).toBeTrue();
  });
});
