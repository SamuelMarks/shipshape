import { Extension } from "@codemirror/state";
import { javascript } from "@codemirror/lang-javascript";
import { json } from "@codemirror/lang-json";
import { html } from "@codemirror/lang-html";
import { css } from "@codemirror/lang-css";
import { markdown } from "@codemirror/lang-markdown";
import { rust } from "@codemirror/lang-rust";
import { yaml } from "@codemirror/lang-yaml";

export function resolveLanguageExtension(language: string): Extension {
  const normalized = language.trim().toLowerCase();

  switch (normalized) {
    case "ts":
    case "typescript":
      return javascript({ typescript: true });
    case "js":
    case "javascript":
      return javascript({ typescript: false });
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
