import { describe, expect, test } from "vitest";

import { DEFAULT_MORPH_CONFIG } from "../core/morph-config";
import { DEFAULT_SETTINGS } from "../core/settings";
import { DEFAULT_CONFIG } from "../core/types";
import { HIDDEN_REVIEW_PRESETS } from "./dev-panel";

describe("HIDDEN_REVIEW_PRESETS", () => {
  test("accepted-default matches clean first-load defaults exactly", () => {
    expect(HIDDEN_REVIEW_PRESETS["accepted-default"].appConfig).toEqual(DEFAULT_CONFIG);
    expect(HIDDEN_REVIEW_PRESETS["accepted-default"].visualSettings).toEqual(DEFAULT_SETTINGS);
    expect(HIDDEN_REVIEW_PRESETS["accepted-default"].morphologyConfig).toEqual(DEFAULT_MORPH_CONFIG);
  });

  test("review-only variants stay separate from the accepted default payload", () => {
    expect(HIDDEN_REVIEW_PRESETS["performance-review"].visualSettings).not.toEqual(DEFAULT_SETTINGS);
    expect(HIDDEN_REVIEW_PRESETS["hero-review"].morphologyConfig).not.toEqual(DEFAULT_MORPH_CONFIG);
  });
});
