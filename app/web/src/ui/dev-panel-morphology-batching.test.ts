import { describe, expect, test } from "vitest";

import {
  DEFAULT_MORPH_CONFIG,
  MORPH_CONFIG_LS_KEY,
  loadMorphConfig,
  type MorphologyConfig,
} from "../core/morph-config";
import { DevPanel } from "./dev-panel";

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

  dispatchEvent(event: { type: string; key?: string }): boolean {
    for (const listener of this.listeners.get(event.type) ?? []) {
      listener(event);
    }
    return true;
  }

  setAttribute(name: string, value: string): void {
    this.attributes.set(name, value);
  }

  getAttribute(name: string): string | null {
    return this.attributes.get(name) ?? null;
  }

  closest(selector: string): FakeElement | null {
    if (selector === "[data-tip]" && this.attributes.has("data-tip")) return this;
    return this.parentElement?.closest(selector) ?? null;
  }

  getBoundingClientRect(): DOMRect {
    return {
      x: 0,
      y: 0,
      width: 100,
      height: 24,
      top: 0,
      right: 100,
      bottom: 24,
      left: 0,
      toJSON: () => ({}),
    };
  }
}

class FakeDocument {
  readonly body = new FakeElement();

  createElement(): FakeElement {
    return new FakeElement();
  }

  addEventListener(): void {}

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

function childText(root: FakeElement): string {
  return [root.textContent, root.innerHTML, ...root.children.map(childText)].join("");
}

function findButton(root: FakeElement, text: string): FakeElement {
  const button = allElements(root).find((el) => el.type === "button" && childText(el) === text);
  if (!button) throw new Error(`Button not found: ${text}`);
  return button;
}

function findControl(root: FakeElement, label: string): {
  slider: FakeElement;
  resetButton: FakeElement;
} {
  const row = allElements(root).find((el) =>
    el.className.split(/\s+/).includes("dp-ctrl-row") && childText(el).includes(label)
  );
  if (!row?.parentElement) throw new Error(`Control row not found: ${label}`);
  const siblings = row.parentElement.children;
  const wrap = siblings[siblings.indexOf(row) + 1];
  const slider = wrap?.children.find((el) => el.type === "range");
  const resetButton = row.children.find((el) => el.type === "button" && childText(el) === "Reset");
  if (!slider || !resetButton) throw new Error(`Control inputs not found: ${label}`);
  return { slider, resetButton };
}

function withFakeDom(run: (body: FakeElement) => void): void {
  const previousDocument = globalThis.document;
  const previousWindow = globalThis.window;
  const document = new FakeDocument();

  Object.defineProperty(globalThis, "document", {
    configurable: true,
    value: document,
  });
  Object.defineProperty(globalThis, "window", {
    configurable: true,
    value: {
      location: { search: "" },
      addEventListener: () => {},
      innerWidth: 1200,
      innerHeight: 800,
    },
  });

  try {
    run(document.body);
  } finally {
    Object.defineProperty(globalThis, "document", {
      configurable: true,
      value: previousDocument,
    });
    Object.defineProperty(globalThis, "window", {
      configurable: true,
      value: previousWindow,
    });
  }
}

describe("DevPanel morphology batching", () => {
  test("non-live morphology edits stay pending until the rebuild button", () => {
    installMemoryLocalStorage();
    withFakeDom((body) => {
      const rebuilds: string[] = [];
      const panel = new DevPanel();
      panel.setMorphHandlers({
        onMorphLive: () => {},
        onMorphRebuild: (json) => { rebuilds.push(json); },
      });

      const baseRadius = findControl(body, "Base radius");
      baseRadius.slider.value = "0.008";
      baseRadius.slider.dispatchEvent({ type: "change" });

      expect(rebuilds).toHaveLength(0);
      expect(localStorage.getItem(MORPH_CONFIG_LS_KEY)).toBeNull();

      findButton(body, "Rebuild Morphology").dispatchEvent({ type: "click" });

      expect(rebuilds).toHaveLength(1);
      const rebuilt = JSON.parse(rebuilds[0]) as MorphologyConfig;
      expect(rebuilt.generator.baseRadius).toBe(0.008);
      expect(loadMorphConfig().generator.baseRadius).toBe(0.008);
      panel.destroy();
    });
  });

  test("non-live row reset applies the pending morphology config", () => {
    installMemoryLocalStorage();
    withFakeDom((body) => {
      const rebuilds: string[] = [];
      const panel = new DevPanel();
      panel.setMorphHandlers({
        onMorphLive: () => {},
        onMorphRebuild: (json) => { rebuilds.push(json); },
      });

      const baseRadius = findControl(body, "Base radius");
      baseRadius.slider.value = "0.008";
      baseRadius.slider.dispatchEvent({ type: "change" });
      baseRadius.resetButton.dispatchEvent({ type: "click" });

      expect(rebuilds).toHaveLength(1);
      const rebuilt = JSON.parse(rebuilds[0]) as MorphologyConfig;
      expect(rebuilt.generator.baseRadius).toBe(DEFAULT_MORPH_CONFIG.generator.baseRadius);
      expect(loadMorphConfig().generator.baseRadius).toBe(DEFAULT_MORPH_CONFIG.generator.baseRadius);
      panel.destroy();
    });
  });

  test("live morphology lighting still applies immediately", () => {
    installMemoryLocalStorage();
    withFakeDom((body) => {
      const lives: string[] = [];
      const rebuilds: string[] = [];
      const panel = new DevPanel();
      panel.setMorphHandlers({
        onMorphLive: (json) => { lives.push(json); },
        onMorphRebuild: (json) => { rebuilds.push(json); },
      });

      const activeCoverage = findControl(body, "Active coverage");
      activeCoverage.slider.value = "0.75";
      activeCoverage.slider.dispatchEvent({ type: "input" });

      expect(rebuilds).toHaveLength(0);
      expect(lives).toHaveLength(1);
      const live = JSON.parse(lives[0]) as MorphologyConfig;
      expect(live.lighting.activeOpacity).toBe(0.75);
      expect(loadMorphConfig().lighting.activeOpacity).toBe(0.75);
      panel.destroy();
    });
  });
});
