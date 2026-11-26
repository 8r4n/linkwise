import en from "./messages/en.json" with { type: "json" };
import zh_Hans from "./messages/zh-Hans.json" with { type: "json" };
import hi from "./messages/hi.json" with { type: "json" };
import es from "./messages/es.json" with { type: "json" };
import ar from "./messages/ar.json" with { type: "json" };
import fr from "./messages/fr.json" with { type: "json" };
import bn from "./messages/bn.json" with { type: "json" };
import pt from "./messages/pt.json" with { type: "json" };
import id from "./messages/id.json" with { type: "json" };
import ru from "./messages/ru.json" with { type: "json" };
import de from "./messages/de.json" with { type: "json" };

const messages = { en, "zh-Hans": zh_Hans, hi, es, ar, fr, bn, pt, id, ru, de };

export const locales = [
  { code: "en", name: "English" },
  { code: "zh-Hans", name: "简体中文" },
  { code: "hi", name: "हिन्दी" },
  { code: "es", name: "Español" },
  { code: "ar", name: "العربية", dir: "rtl" },
  { code: "fr", name: "Français" },
  { code: "bn", name: "বাংলা" },
  { code: "pt", name: "Português" },
  { code: "id", name: "Bahasa Indonesia" },
  { code: "ru", name: "Русский" },
  { code: "de", name: "Deutsch" },
];
export const pages = ["", "downloads", "mail", "winlink", "varac", "guide", "docs", "capabilities", "demos", "learn", "research"];
export const groups = {
  nav: ["downloads", "mail", "winlink", "guide", "docs"],
  ui: ["skip", "navigation", "language", "agents", "source", "english", "fallback", "translation", "manuals"],
  home: ["title", "lede", "downloads", "setup", "alt", "caption", "status", "guideLede"],
  downloads: ["title", "preview", "release", "verify", "pending", "choose", "get", "checksum", "install", "manifest"],
  mail: ["title", "lede", "bench", "tested", "status", "scope", "architecture"],
  winlink: ["title", "lede", "configure", "step1", "step2", "step3", "step4", "verify", "test", "scope", "full", "video", "getVideo"],
  guide: ["title", "lede", "start", "startText", "chat", "chatText", "files", "filesText", "pki", "pkiText", "qsl", "qslText", "mail", "mailText", "receive", "receiveText", "security", "securityText"],
  docs: ["title", "lede", "operator", "architecture", "testing", "reference"],
  man: ["overview", "reference", "cli", "daemon", "station", "daemonText", "stationText"],
};

export function loadMessages(code) {
  const rows = messages[code];
  if (!rows) throw new Error("Unknown locale: " + code);
  for (const [group, fields] of Object.entries(groups)) {
    if (!Array.isArray(rows[group]) || rows[group].length !== fields.length
        || rows[group].some(line => typeof line !== "string" || !line.trim())) {
      throw new Error("Invalid " + code + " translation group: " + group);
    }
  }
  return Object.fromEntries(Object.entries(groups).map(([group, fields]) =>
    [group, Object.fromEntries(fields.map((key, index) => [key, rows[group][index]]))]));
}

export function pagePath(code, page = "") {
  const parts = [code === "en" ? "" : code, page].filter(Boolean);
  return "/" + (parts.length ? parts.join("/") + "/" : "");
}
