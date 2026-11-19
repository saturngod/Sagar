import hljs from "highlight.js/lib/core";
import css from "highlight.js/lib/languages/css";
import xml from "highlight.js/lib/languages/xml";

hljs.registerLanguage("xml", xml);
hljs.registerLanguage("css", css);

/** Presentation only: highlight.js escapes source before adding its token spans. */
export function highlightHtml(source: string): string {
  return hljs.highlight(source, { language: "xml", ignoreIllegals: true }).value;
}

/** Presentation only: escaped CSS with highlight.js token spans. */
export function highlightCss(source: string): string {
  return hljs.highlight(source, { language: "css", ignoreIllegals: true }).value;
}
