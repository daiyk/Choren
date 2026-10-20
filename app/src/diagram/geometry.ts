// Card and classic box layout; parent rails share the classic port geometry.

export interface Pt {
  x: number;
  y: number;
}

export const HEADER = 31;
export const PITCH = 20; // vertical distance between ports on classic boxes and rails
export const PORT = 16; // classic port square
export const RAIL_W = 22;

export const CLASSIC_HEADER = 22;
export const CLASSIC_NOTCH = 14;
export const CLASSIC_RADIUS = 8;
/** Height the ports need on a classic box; a text box adds its text below them. */
export const classicPortsHeight = (nIn: number, nOut: number) => Math.max(30 + PITCH * nIn, 14 + PITCH * nOut);
export function classicBoxSize(nIn: number, nOut: number, text = false) {
  const ports = classicPortsHeight(nIn, nOut);
  return text ? { w: 200, h: Math.max(150, ports + 100) } : { w: 90, h: Math.max(96, ports) };
}
// Inputs sit in the left notch below the header; outputs hang off the right edge.
export const classicInputSlot = (i: number): Pt => ({ x: -2, y: CLASSIC_HEADER + 4 + PITCH * i });
export const classicOutputSlot = (w: number, i: number): Pt => ({ x: w + 2, y: 4 + PITCH * i });

/** Link path from the input anchor back to the output anchor: one cubic whose handles leave
 *  each port horizontally, at least 40 px, so backward links loop around the boxes. */
export function buildLink(out: Pt, inp: Pt): string {
  const k = Math.max(40, Math.abs(inp.x - out.x) / 2);
  return `M${inp.x},${inp.y} C${inp.x - k},${inp.y} ${out.x + k},${out.y} ${out.x},${out.y}`;
}

export const CARD_W = 176;
export const CARD_PORT_TOP = 68;
export const CARD_PITCH = 22;

/** Shared by the painted box and React Flow, including connected parameter inputs. */
export function boxSize(nIn: number, nOut: number, text = false, stacked = false) {
  const rows = Math.max(nIn, nOut);
  const portsBottom = rows ? CARD_PORT_TOP + CARD_PITCH * (rows - 1) + 18 : 56;
  const cardH = portsBottom + (text ? 40 : 0);
  const stack = stacked ? 8 : 0;
  return { w: CARD_W + stack, h: cardH + stack, cardW: CARD_W, cardH, textTop: portsBottom };
}

export const cardAnchor = (i: number, output = false): Pt => ({
  x: output ? CARD_W + 10 : -10,
  y: CARD_PORT_TOP + CARD_PITCH * i,
});

/** Link anchors: at the outer edge of a port's type tab, level with the port's middle. */
export const inputAnchor = (s: Pt): Pt => ({ x: s.x - 3, y: s.y + PORT / 2 });
export const outputAnchor = (s: Pt): Pt => ({ x: s.x + PORT + 3, y: s.y + PORT / 2 });
