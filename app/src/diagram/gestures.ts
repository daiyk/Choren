import type { Viewport } from "../workspace/model";

/** What a two-finger scroll or a mouse wheel does in a graph. */
export type ScrollMode = "auto" | "pan" | "zoom";

type Wheel = Pick<WheelEvent, "deltaX" | "deltaY" | "deltaMode"> & { wheelDeltaY?: number };

/**
 * Guesses whether a wheel event comes from a trackpad rather than a mouse wheel. Wheels step in
 * lines, or in pixel notches that Chromium reports as multiples of 120 and WebKit as fractional
 * pixels; trackpads scroll in whole pixels and often on both axes at once.
 */
export function fromTrackpad(e: Wheel) {
  if (e.deltaMode !== 0) return false;
  if (e.deltaX !== 0) return true;
  if (e.wheelDeltaY && e.wheelDeltaY % 120 === 0) return false;
  return Number.isInteger(e.deltaY);
}

/** Zooms to `zoom` (clamped) while the graph point under `point` stays put. */
export function zoomAround(v: Viewport, zoom: number, point: { x: number; y: number }, min: number, max: number): Viewport {
  const next = Math.min(max, Math.max(min, zoom));
  const ratio = next / v.zoom;
  return { x: point.x - (point.x - v.x) * ratio, y: point.y - (point.y - v.y) * ratio, zoom: next };
}
