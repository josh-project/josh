// Language picker and "outdated translation" banner for the josh mdbook.
//
// Loaded on every page via book.toml additional-js. Both books deploy under
// the same site, with an optional language segment right after a "docs"
// segment in the path:
//   .../docs/<page>.html         -> English (canonical, no segment)
//   .../docs/zh_CN/<page>.html   -> Simplified Chinese
//
// The script detects the current language from the URL path, injects a
// <select> picker into the navbar, and on non-English pages prepends a
// warning banner with a link to the matching English page.

"use strict";

// The path segment that anchors the (optional) language segment. English is
// the default and has no segment; other languages use their `code`.
const DOCS_SEGMENT = "docs";

const LANGUAGES = [
  { code: "en_001", label: "English" },
  { code: "zh_CN", label: "简体中文" },
];

const DEFAULT_LANG = LANGUAGES[0];

const WARNINGS = {
  zh_CN: { message: "⚠️ 本翻译可能已过时。仅英文版会持续维护，", linkText: "查看英文版" },
};

const DEFAULT_WARNING = {
  message: "⚠️ This translation may be outdated. Only the English version is " +
    "actively maintained, ",
  linkText: "see the English version",
};

// Find the language by scanning path segments from the end for "docs". The
// segment immediately after it is the language code; if it is not a known
// language (i.e. it is a page), the language is the default (English).
function detectLang(pathname) {
  const segments = pathname.split("/").filter(Boolean);
  for (let i = segments.length - 2; i >= 0; i--) {
    if (segments[i] === DOCS_SEGMENT) {
      return LANGUAGES.find((lang) => lang.code === segments[i + 1]) ?? DEFAULT_LANG;
    }
  }
  return DEFAULT_LANG;
}

// Return the pathname for the same page in `target` language: drop the current
// language's segment after "docs", then insert the target's (if any).
function rewritePath(pathname, current, target) {
  const segments = pathname.split("/");
  const docsIndex = segments.lastIndexOf(DOCS_SEGMENT);
  if (docsIndex === -1) return pathname;

  const langIndex = docsIndex + 1;
  if (current !== DEFAULT_LANG && segments[langIndex] === current.code) {
    segments.splice(langIndex, 1);
  }
  if (target !== DEFAULT_LANG) {
    segments.splice(langIndex, 0, target.code);
  }

  return segments.join("/");
}

function buildPicker(current) {
  const select = document.createElement("select");
  select.className = "lang-picker";
  select.setAttribute("aria-label", "Language");

  for (const lang of LANGUAGES) {
    const option = new Option(lang.label, lang.code, false, lang.code === current.code);
    select.add(option);
  }

  select.addEventListener("change", () => {
    const target = LANGUAGES.find((lang) => lang.code === select.value);
    if (target && target.code !== current.code) {
      window.location.pathname = rewritePath(window.location.pathname, current, target);
    }
  });

  return select;
}

function buildWarning(current) {
  const { message, linkText } = WARNINGS[current.code] ?? DEFAULT_WARNING;

  const banner = document.createElement("div");
  banner.className = "lang-warning";
  banner.setAttribute("role", "note");

  const link = document.createElement("a");
  link.href = rewritePath(window.location.pathname, current, DEFAULT_LANG);
  link.textContent = linkText;

  banner.append(message, link, ".");
  return banner;
}

function inject() {
  const current = detectLang(window.location.pathname);

  const host = document.querySelector(".right-buttons") ||
    document.querySelector(".menu-bar");
  host?.appendChild(buildPicker(current));

  if (current.code !== DEFAULT_LANG.code) {
    const content = document.querySelector("main") ||
      document.querySelector(".content");
    content?.insertBefore(buildWarning(current), content.firstChild);
  }
}

if (document.readyState === "loading") {
  document.addEventListener("DOMContentLoaded", inject);
} else {
  inject();
}
