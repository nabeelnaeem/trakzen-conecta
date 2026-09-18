import hljs from "highlight.js/lib/core";
import javascript from "highlight.js/lib/languages/javascript";
import typescript from "highlight.js/lib/languages/typescript";
import python from "highlight.js/lib/languages/python";
import rust from "highlight.js/lib/languages/rust";
import go from "highlight.js/lib/languages/go";
import java from "highlight.js/lib/languages/java";
import csharp from "highlight.js/lib/languages/csharp";
import sql from "highlight.js/lib/languages/sql";
import bash from "highlight.js/lib/languages/bash";
import json from "highlight.js/lib/languages/json";
import yaml from "highlight.js/lib/languages/yaml";
import xml from "highlight.js/lib/languages/xml";
import css from "highlight.js/lib/languages/css";
import markdown from "highlight.js/lib/languages/markdown";
import diff from "highlight.js/lib/languages/diff";

// Only the languages people paste in a work chat; keeps the bundle small.
const langs: Record<string, Parameters<typeof hljs.registerLanguage>[1]> = {
  javascript, typescript, python, rust, go, java, csharp, sql, bash, json, yaml, xml, css, markdown, diff,
};
for (const [name, def] of Object.entries(langs)) hljs.registerLanguage(name, def);
hljs.registerAliases(["js", "jsx", "mjs"], { languageName: "javascript" });
hljs.registerAliases(["ts", "tsx"], { languageName: "typescript" });
hljs.registerAliases(["py"], { languageName: "python" });
hljs.registerAliases(["rs"], { languageName: "rust" });
hljs.registerAliases(["cs"], { languageName: "csharp" });
hljs.registerAliases(["sh", "shell", "zsh", "console"], { languageName: "bash" });
hljs.registerAliases(["yml"], { languageName: "yaml" });
hljs.registerAliases(["html", "svg", "vue"], { languageName: "xml" });
hljs.registerAliases(["md"], { languageName: "markdown" });

/** Returns highlighted, HTML-escaped code. Unknown languages are auto-detected. */
export function highlightCode(code: string, lang: string): { html: string; language: string } {
  try {
    if (lang && hljs.getLanguage(lang)) {
      return { html: hljs.highlight(code, { language: lang, ignoreIllegals: true }).value, language: lang };
    }
    if (code.length < 4000) {
      const r = hljs.highlightAuto(code, Object.keys(langs));
      if ((r.relevance ?? 0) >= 5 && r.language) return { html: r.value, language: r.language };
    }
  } catch {
    /* fall through to plain */
  }
  const esc = code.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  return { html: esc, language: lang };
}
