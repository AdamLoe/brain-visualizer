import { describe, expect, test } from "vitest";

import { DevPanel } from "./dev-panel";

// Two-tier curation: ?dev=1 opens Essentials (13-row keep-list), ?dev=true opens
// Advanced (every row). These tests drive the panel through the same minimal
// fake DOM the morphology-batching suite uses, but parameterized by the boot
// URL so each tier's rendered control set can be asserted. No browser needed.

type Listener = (event: { type: string; key?: string }) => void;

class FakeClassList {
  constructor(private readonly element: FakeElement) {}
  add(name: string): void {
    const names = new Set(this.element.className.split(/\s+/).filter(Boolean));
    names.add(name);
    this.element.className = [...names].join(" ");
  }
  remove(name: string): void {
    this.element.className = this.element.className
      .split(/\s+/)
      .filter((part) => part && part !== name)
      .join(" ");
  }
  contains(name: string): boolean {
    return this.element.className.split(/\s+/).includes(name);
  }
}

class FakeElement {
  id = "";
  className = "";
  textContent = "";
  innerHTML = "";
  title = "";
  type = "";
  value = "";
  min = "";
  max = "";
  step = "";
  checked = false;
  selected = false;
  hidden = false;
  tabIndex = 0;
  style: Record<string, string> = {};
  dataset: Record<string, string> = {};
  readonly children: FakeElement[] = [];
  readonly classList = new FakeClassList(this);
  parentElement: FakeElement | null = null;
  private readonly attributes = new Map<string, string>();
  private readonly listeners = new Map<string, Listener[]>();

  appendChild(child: FakeElement): FakeElement {
    child.parentElement = this;
    this.children.push(child);
    return child;
  }
  addEventListener(type: string, listener: Listener): void {
    const listeners = this.listeners.get(type) ?? [];
    listeners.push(listener);
    this.listeners.set(type, listeners);
  }
  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }
  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }
  querySelector(): FakeElement | null {
    return null;
  }
  closest(selector: string): FakeElement | null {
    if (selector === "[data-tip]" && this.attributes.has("data-tip")) return this;
    return this.parentElement?.closest(selector) ?? null;
  }
  getBoundingClientRect(): DOMRect {
    return {
      x: 0, y: 0, width: 100, height: 24, top: 0, right: 100, bottom: 24, left: 0,
      toJSON: () => ({}),
    };
  }
}

class FakeDocument {
  readonly body = new FakeElement();
  readonly activeElement: FakeElement | null = null;
  createElement(): FakeElement {
    return new FakeElement();
  }
  addEventListener(): void {}
  contains(): boolean {
    return false;
  }
  getElementById(id: string): FakeElement | null {
    return allElements(this.body).find((el) => el.id === id) ?? null;
  }
}

function installMemoryLocalStorage(): void {
  const store = new Map<string, string>();
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => { store.set(key, value); },
      removeItem: (key: string) => { store.delete(key); },
    },
  });
}

function allElements(root: FakeElement): FakeElement[] {
  return [root, ...root.children.flatMap((child) => allElements(child))];
}

/** Every control's accessible label: slider/select/checkbox aria-label across the panel. */
function controlLabels(body: FakeElement): Set<string> {
  const labels = new Set<string>();
  for (const el of allElements(body)) {
    if (el.type === "range" || el.type === "checkbox") {
      const label = el.getAttribute("aria-label");
      if (label) labels.add(label);
    }
    if (el.className.split(/\s+/).includes("dp-select")) {
      const label = el.getAttribute("aria-label");
      if (label) labels.add(label);
    }
  }
  return labels;
}

function tabIds(body: FakeElement): Set<string> {
  const ids = new Set<string>();
  for (const el of allElements(body)) {
    if (el.className.split(/\s+/).includes("dp-tab") && el.dataset.tabId) {
      ids.add(el.dataset.tabId);
    }
  }
  return ids;
}

function withTier(search: string, run: (body: FakeElement) => void): void {
  installMemoryLocalStorage();
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;
  const previousHtmlElement = (globalThis as { HTMLElement?: unknown }).HTMLElement;
  const document = new FakeDocument();
  // _setOpen(true) runs at construction when the URL opens the panel; it tests
  // `document.activeElement instanceof HTMLElement`, so HTMLElement must exist.
  Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: FakeElement });
  Object.defineProperty(globalThis, "document", { configurable: true, value: document });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      location: { search },
      addEventListener: () => {},
      requestAnimationFrame: () => 0,
      innerWidth: 1200,
      innerHeight: 800,
    },
  });
  try {
    new DevPanel();
    run(document.body);
  } finally {
    Object.defineProperty(globalThis, "document", { configurable: true, value: previousDocument });
    Object.defineProperty(globalThis, "window", { configurable: true, value: previousWindow });
    Object.defineProperty(globalThis, "HTMLElement", { configurable: true, value: previousHtmlElement });
  }
}

// The 13 Essentials control labels (aria-label text) — 8 Appearance settings rows
// plus 5 morphology-lighting rows. Must all be present in both tiers.
const ESSENTIALS_LABELS = [
  "Color by",
  "Neurons",
  "Glow decay (τ)",
  "Visual radius",
  "Active boost ×",
  "Inactive opacity",
  "Connections",
  "Reveal on arrival",
  "Ambient",
  "Diffuse intensity",
  "Rim intensity",
  "Active boost",
  "Resting brightness",
];

// Representative Advanced-only control labels that must NOT render in Essentials.
const ADVANCED_LABELS = [
  "I_ext (drive)",       // Network tab setting (iExt)
  "N (neurons)",         // Network tab N
  "Base radius",         // generator descriptor
  "Light dir X",         // non-essential lighting
  "Voltage glow",        // non-essential appearance setting
  "Curve",               // connectionCurveLift
];

describe("DevPanel Essentials tier (?dev=1)", () => {
  test("renders exactly the 13-row keep-list and omits Advanced tabs", () => {
    withTier("?dev=1", (body) => {
      const labels = controlLabels(body);
      for (const keep of ESSENTIALS_LABELS) {
        expect(labels.has(keep)).toBe(true);
      }
      for (const adv of ADVANCED_LABELS) {
        expect(labels.has(adv)).toBe(false);
      }
      // Network + Morphology tabs are omitted entirely in Essentials.
      const tabs = tabIds(body);
      expect(tabs.has("network")).toBe(false);
      expect(tabs.has("morphology")).toBe(false);
      // Diagnostic/read-only tabs stay.
      expect(tabs.has("monitor")).toBe(true);
      expect(tabs.has("dynamics")).toBe(true);
      expect(tabs.has("storage")).toBe(true);
      expect(tabs.has("debugview")).toBe(true);
      expect(tabs.has("appearance")).toBe(true);
    });
  });
});

describe("DevPanel Advanced tier (?dev=true)", () => {
  test("renders every keep-list and Advanced control plus all tabs", () => {
    withTier("?dev=true", (body) => {
      const labels = controlLabels(body);
      for (const keep of ESSENTIALS_LABELS) {
        expect(labels.has(keep)).toBe(true);
      }
      for (const adv of ADVANCED_LABELS) {
        expect(labels.has(adv)).toBe(true);
      }
      const tabs = tabIds(body);
      expect(tabs.has("network")).toBe(true);
      expect(tabs.has("morphology")).toBe(true);
    });
  });
});
