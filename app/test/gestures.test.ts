import { describe, expect, it } from "vitest";
import { fromTrackpad, zoomAround } from "../src/diagram/gestures";

describe("graph gestures", () => {
  it("tells trackpad scrolls from mouse wheel notches", () => {
    // Trackpads: whole pixels, often on both axes.
    expect(fromTrackpad({ deltaX: 0, deltaY: 3, deltaMode: 0, wheelDeltaY: -9 })).toBe(true);
    expect(fromTrackpad({ deltaX: -2, deltaY: 0.5, deltaMode: 0 })).toBe(true);
    // Chromium wheel notches, WebKit wheel notches, Firefox line steps.
    expect(fromTrackpad({ deltaX: 0, deltaY: 100, deltaMode: 0, wheelDeltaY: -120 })).toBe(false);
    expect(fromTrackpad({ deltaX: 0, deltaY: 4.000244140625, deltaMode: 0, wheelDeltaY: -12 })).toBe(false);
    expect(fromTrackpad({ deltaX: 0, deltaY: 3, deltaMode: 1 })).toBe(false);
  });

  it("zooms around the pinch point and clamps the zoom", () => {
    const v = zoomAround({ x: 0, y: 0, zoom: 1 }, 2, { x: 100, y: 50 }, 0.1, 4);
    expect(v).toEqual({ x: -100, y: -50, zoom: 2 });
    // The graph point under the pinch stays under it.
    expect((100 - v.x) / v.zoom).toBe(100);
    expect(zoomAround({ x: 0, y: 0, zoom: 1 }, 10, { x: 0, y: 0 }, 0.1, 4).zoom).toBe(4);
  });
});
