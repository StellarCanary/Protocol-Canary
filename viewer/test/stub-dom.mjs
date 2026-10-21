/* Minimal DOM stub so viewer/app.js can be executed under Node without a
 * browser. It only implements what the page actually uses — getElementById,
 * createElement, listeners, attributes, textContent, focus and the dialog
 * methods — and deliberately makes everything else impossible:
 *
 * - only the tags a textContent-only page may create; anything else throws,
 * - `innerHTML`, `outerHTML`, `insertAdjacentHTML` and `document.write`
 *   throw if touched, so any HTML sink iscaught at runtime,
 * - localStorage can be a real map or a throwing stub (private browsing),
 * - matchMedia answers the `prefers-color-scheme` query statically.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";

const here = path.dirname(fileURLToPath(import.meta.url));
const viewerDir = path.join(here, "..");

/* Exactly the element kinds the page is allowed to build: the fact list,
 * the findings and skipped rows, the collapsed-details blocks, and the
 * comparison table's rows and cells. */
export const ALLOWED_TAGS = new Set([
  "dt",
  "dd",
  "li",
  "span",
  "details",
  "summary",
  "p",
  "tr",
  "td",
]);

const HTML_TRAPS = ["innerHTML", "outerHTML"];

export function createStubDom({ allowStorage = true, systemDark = false } = {}) {
  const createdTags = new Set();
  const attributes = []; // every setAttribute: { element, name, value }
  const focusLog = []; // focused element ids, in order
  const modalOps = []; // "show"/"close" for the dialog
  const storageAttempts = { reads: 0, writes: 0 };
  const storage = new Map();

  class StubElement {
    constructor(id) {
      this.tagName = ""; // set for created elements; id-backed nodes stay ""
      this.id = id || "";
      this.attributes = new Map();
      this.children = [];
      this.listeners = new Map();
      this.hidden = false;
      this.value = "";
      this.files = null;
      this.text = "";
      this.open = false;
      this.parentNode = null;
    }

    get textContent() {
      /* A real DOM concatenates all descendant text. */
      if (this.children.length > 0) {
        return this.children.map((child) => child.textContent).join("");
      }
      return this.text;
    }

    set textContent(value) {
      this.text = String(value);
      this.children = [];
    }

    get firstChild() {
      return this.children[0] ?? null;
    }

    appendChild(child) {
      this.children.push(child);
      child.parentNode = this;
      return child;
    }

    removeChild(child) {
      const index = this.children.indexOf(child);
      if (index === -1) throw new Error("removeChild: node is not a child");
      this.children.splice(index, 1);
      child.parentNode = null;
      return child;
    }

    setAttribute(name, value) {
      const nv = String(name);
      this.attributes.set(nv, String(value));
      attributes.push({ element: this, name: nv, value: String(value) });
    }

    getAttribute(name) {
      const value = this.attributes.get(name);
      return value === undefined ? null : value;
    }

    hasAttribute(name) {
      return this.attributes.has(name);
    }

    removeAttribute(name) {
      this.attributes.delete(name);
    }

    addEventListener(type, handler) {
      if (!this.listeners.has(type)) this.listeners.set(type, []);
      this.listeners.get(type).push(handler);
    }

    dispatch(type, event = {}) {
      const handlers = this.listeners.get(type);
      if (!handlers) return;
      const base = {
        type,
        target: this,
        preventDefault() {},
        stopPropagation() {},
      };
      const merged = Object.assign(base, event);
      merged.target = merged.target ?? this;
      handlers.forEach((handler) => handler(merged));
    }

    focus() {
      focusLog.push(this.id);
    }

    showModal() {
      modalOps.push("show");
      this.open = true;
    }

    close() {
      modalOps.push("close");
      this.open = false;
    }
  }

  for (const trap of HTML_TRAPS) {
    Object.defineProperty(StubElement.prototype, trap, {
      get() {
        throw new Error("illegal DOM sink touched: " + trap);
      },
      set() {
        throw new Error("illegal DOM sink written: " + trap);
      },
    });
  }
  StubElement.prototype.insertAdjacentHTML = function () {
    throw new Error("illegal DOM sink written: insertAdjacentHTML");
  };

  const html = readFileSync(path.join(viewerDir, "index.html"), "utf8");
  const byId = new Map();
  /* Copy the statically declared attributes of every id element into the
   * stub so markup state (aria-selected, hidden, tabindex, aria-label...)
   * is visible before app.js runs. */
  for (const tag of html.matchAll(/<[a-zA-Z0-9]+\b[^>]*\bid="([^"]+)"[^>]*>/g)) {
    const id = tag[1];
    if (byId.has(id)) continue;
    const element = new StubElement(id);
    if (/\bhidden\b/.test(tag[0])) element.hidden = true;
    for (const attr of tag[0].matchAll(/\s([a-zA-Z-]+)="([^"]*)"/g)) {
      if (attr[1] === "id") continue;
      element.attributes.set(attr[1], attr[2]);
    }
    byId.set(id, element);
  }
  if (byId.size === 0) throw new Error("stub found no ids in index.html");

  const documentElement = new StubElement("");

  const document = {
    documentElement,
    getElementById(id) {
      return byId.get(id) ?? null;
    },
    createElement(tagName) {
      const tag = String(tagName).toLowerCase();
      if (!ALLOWED_TAGS.has(tag)) {
        throw new Error("createElement of a disallowed tag: <" + tag + ">");
      }
      createdTags.add(tag);
      const node = new StubElement("");
      node.tagName = tag.toUpperCase();
      return node;
    },
  };
  const writeTrap = () => {
    throw new Error("illegal DOM sink written: document.write");
  };
  document.write = writeTrap;
  document.writeln = writeTrap;

  const localStorage = allowStorage
    ? {
        getItem(key) {
          storageAttempts.reads++;
          return storage.has(key) ? storage.get(key) : null;
        },
        setItem(key, value) {
          storageAttempts.writes++;
          storage.set(key, String(value));
        },
      }
    : {
        getItem() {
          storageAttempts.reads++;
          throw new Error("SecurityError: localStorage access denied");
        },
        setItem() {
          storageAttempts.writes++;
          throw new Error("SecurityError: localStorage access denied");
        },
      };

  const self = {
    CanaryViewer: createRequire(import.meta.url)(path.join(viewerDir, "report.js")),
    localStorage,
    matchMedia(query) {
      return { matches: /dark/.test(String(query)) ? systemDark : false };
    },
  };

  const appSource = readFileSync(path.join(viewerDir, "app.js"), "utf8");
  const runApp = new Function("self", "document", appSource);

  return {
    byId: (id) => {
      if (!byId.has(id)) throw new Error("no element with id " + id + " in index.html");
      return byId.get(id);
    },
    ids: [...byId.keys()],
    documentElement,
    attributes,
    createdTags,
    focusLog,
    modalOps,
    storageAttempts,
    storage,
    runApp() {
      runApp(self, document);
    },
  };
}

/* Drive a report file through the file input and let all pending promise
 * work settle (File.text() -> then -> render). */
export async function loadReport(stub, text, size = text.length) {
  let textCalls = 0;
  const file = {
    size,
    text() {
      textCalls++;
      return Promise.resolve(text);
    },
  };
  stub.byId("file").files = [file];
  stub.byId("file").dispatch("change", {});
  for (let i = 0; i < 10; i++) await Promise.resolve();
  return { file, textCalls };
}
