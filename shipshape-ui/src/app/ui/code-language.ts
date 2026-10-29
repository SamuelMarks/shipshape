/**
 * @file Language syntax extension resolver for CodeMirror 6 editors.
 */

import { Extension } from "@codemirror/state";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";

/**
 * Map a language identifier string to its corresponding CodeMirror 6 {@link Extension}.
 *
 * @param language - Case-insensitive language name or file extension (e.g., `"rust"`, `"ts"`, `"yaml"`).
 * @returns CodeMirror syntax extension, or an empty array `[]` for unknown/plaintext languages.
 */
export function resolveLanguageExtension(language: string): Extension {
  switch (language.toLowerCase()) {
    case "ts":
    case "typescript":
      return javascript({ typescript: true });
    case "js":
    case "javascript":
      return javascript();
    case "json":
      return json();
    case "html":
      return html();
    case "css":
      return css();
    case "md":
    case "markdown":
      return markdown();
    case "rust":
      return rust();
    case "yaml":
    case "yml":
      return yaml();
    default:
      return [];
  }
}
