import { describe, expect, it } from "vitest";
import { boxSize, cardAnchor, CARD_PITCH } from "../src/diagram/geometry";

describe("compact box geometry", () => {
  it("keeps ordinary boxes compact and grows with the longer port column", () => {
    expect(boxSize(2, 1)).toMatchObject({ w: 176, h: 108 });
    expect(boxSize(0, 0).h).toBeLessThan(108);
    expect(boxSize(2, 2).h).toBe(boxSize(2, 1).h);
    expect(boxSize(3, 1).h).toBe(108 + CARD_PITCH);
    expect(boxSize(1, 3).h).toBe(108 + CARD_PITCH);
  });

  it.each([[12, 1], [1, 12], [12, 12]])("contains all ports for %i inputs and %i outputs", (inputs, outputs) => {
    const size = boxSize(inputs, outputs);
    for (const count of [inputs, outputs]) {
      for (let index = 0; index < count; index++) {
        expect(cardAnchor(index).y - 10).toBeGreaterThan(52);
        expect(cardAnchor(index).y + 10).toBeLessThan(size.h);
        if (index) expect(cardAnchor(index).y - cardAnchor(index - 1).y).toBeGreaterThan(20);
      }
    }
    expect(cardAnchor(outputs - 1, true).x).toBe(size.w + 10);
  });

  it("reserves a separate comment area below ports", () => {
    for (const count of [0, 1, 12]) {
      const size = boxSize(count, 2, true);
      expect(size.textTop).toBeGreaterThan(cardAnchor(Math.max(count, 2) - 1).y + 10);
      expect(size.h - size.textTop).toBe(40);
    }
    expect(boxSize(0, 0, true).h).toBeLessThan(108);
  });

  it("includes subgraph layers in measured bounds without moving ports or content", () => {
    for (const rows of [2, 12]) {
      const flat = boxSize(rows, 1);
      const stacked = boxSize(rows, 1, false, true);
      expect(stacked).toEqual({ ...flat, w: flat.w + 8, h: flat.h + 8 });
    }
  });
});
