import { describe, expect, test } from "vitest";

import { DEFAULT_SETTINGS, SETTINGS_LENGTH, toFloat32Array } from "../core/settings";
import { COLOR_BY_LABELS, COLOR_BY_OPTIONS } from "./dev-panel";

describe("Brain color mode", () => {
  test("is the default color mode and keeps the settings layout locked", () => {
    expect(DEFAULT_SETTINGS.colorBy).toBe(6);

    const values = toFloat32Array({ ...DEFAULT_SETTINGS, colorBy: 6 });

    expect(values.length).toBe(SETTINGS_LENGTH);
    expect(values[18]).toBe(6);
  });

  test("is exposed in the color selector and debug labels", () => {
    expect(COLOR_BY_OPTIONS.map((option) => option.value)).toEqual([0, 1, 2, 3, 4, 5, 6, 7]);
    expect(COLOR_BY_OPTIONS.at(-1)).toEqual({ value: 7, label: "Brain 2" });
    expect(COLOR_BY_LABELS[6]).toBe("Brain");
    expect(COLOR_BY_LABELS[7]).toBe("Brain 2");
  });
});
