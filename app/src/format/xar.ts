import { XMLParser } from "fast-xml-parser";
import type { Box, Diagram, Link, Port, Timeline } from "./types";
import { xmlOptions } from "./xml";

const ARRAYS = new Set([
  "Box",
  "Input",
  "Output",
  "Parameter",
  "Link",
  "Resource",
  "bitmap",
  "BehaviorLayer",
  "BehaviorKeyframe",
  "ActuatorList",
  "ActuatorCurve",
  "Key",
]);

const parser = new XMLParser({
  ...xmlOptions,
  cdataPropName: "#cdata",
  isArray: (name) => ARRAYS.has(name),
});

type X = Record<string, any>;
const num = (v: unknown, d = 0) =>
  v === undefined || v === "" ? d : Number(v);
const text = (v: unknown): string => {
  if (typeof v === "string") return v;
  const content = (v as X | undefined)?.["#cdata"] ?? (v as X | undefined)?.["#text"] ?? "";
  return Array.isArray(content) ? content.join("") : content;
};

function port(p: X): Port {
  return {
    id: num(p.id),
    name: p.name ?? "",
    type: num(p.type),
    typeSize: num(p.type_size, 1),
    stm: p.stm_value_name,
    nature: num(p.nature),
    inner: p.inner === "1",
    tooltip: p.tooltip ?? "",
  };
}

function diagram(d: X | string | undefined): Diagram {
  const o: X = typeof d === "object" ? d : {};
  return {
    scale: num(o.scale, 100),
    boxes: (o.Box ?? []).map(box),
    links: (o.Link ?? []).map((l: X): Link => ({
      outOwner: num(l.outputowner),
      outPort: num(l.indexofoutput),
      inOwner: num(l.inputowner),
      inPort: num(l.indexofinput),
    })),
  };
}

function timeline(t: X): Timeline {
  return {
    enabled: t.enable === "1",
    fps: num(t.fps, 25),
    size: num(t.size),
    startFrame: num(t.start_frame, 1),
    endFrame: num(t.end_frame, -1),
    resourcesAcquisition: num(t.resources_acquisition),
    layers: (t.BehaviorLayer ?? []).map((l: X) => ({
      name: l.name ?? "",
      mute: l.mute === "1",
      keyframes: (l.BehaviorKeyframe ?? []).map((k: X) => ({
        name: k.name ?? "",
        index: num(k.index, 1),
        diagram: "Diagram" in k ? diagram(k.Diagram) : undefined,
      })),
    })),
    curves: (t.ActuatorList ?? []).flatMap((a: X) =>
      (a.ActuatorCurve ?? []).map((c: X) => ({
        actuator: c.actuator ?? "",
        frames: (c.Key ?? []).map((k: X) => num(k.frame)),
      })),
    ),
  };
}

function box(b: X): Box {
  return {
    id: num(b.id),
    name: b.name ?? "",
    x: num(b.x),
    y: num(b.y),
    category: b.choren_category,
    tooltip: b.tooltip ?? "",
    bitmaps: (b.bitmap ?? []).map(text),
    script: text(b.script?.content),
    plugin: b.plugin ?? "",
    pluginText: b.plugin === "textedit_plugin" ? text(b.pluginContent?.text) : undefined,
    pluginLanguage: b.plugin === "setlanguage_plugin"
      ? text(b.pluginContent?.language) || (/^\s*self\.language = "([^"]*)"$/m.exec(text(b.script?.content))?.[1] ?? "")
      : undefined,
    dialogFile: b.dialogFile === undefined ? undefined : text(b.dialogFile),
    inputs: (b.Input ?? []).map(port),
    outputs: (b.Output ?? []).map(port),
    params: (b.Parameter ?? []).map((p: X) => ({
      id: num(p.id),
      name: p.name ?? "",
      contentType: num(p.content_type),
      value: p.value ?? "",
      defaultValue: p.default_value ?? "",
      inheritsFromParent: p.inherits_from_parent === "1",
      tooltip: p.tooltip ?? "",
    })),
    resources: (b.Resource ?? []).map((r: X) => ({
      name: r.name ?? "",
      type: r.type ?? "",
      timeout: r.timeout ?? "",
    })),
    timeline: b.Timeline ? timeline(b.Timeline) : undefined,
  };
}

/** Parses a behavior.xar / box.xar document into its root box. */
export function parseXar(xml: string): Box {
  const doc = parser.parse(xml);
  const top: X | undefined = doc.ChoregrapheProject ?? doc.ChoregrapheBox;
  const root = top?.Box?.[0];
  if (!root) throw new Error("Not a Choregraphe XAR file (no root Box)");
  return box(root);
}

export interface Endpoint {
  owner: number; // 0 = the parent box (rails)
  port: Port;
}

// Port type each ParameterContent constructor assigns: bool → dynamic, int/double → number, else string.
const PARAM_IO_TYPE = [0, 2, 2, 3, 3, 3];

/** A parameter exposed as an input port (it only shows when a link targets it). */
export const paramPort = (p: Box["params"][number]): Port => ({
  node: p.node,
  id: p.id,
  name: p.name,
  type: PARAM_IO_TYPE[p.contentType] ?? 0,
  nature: 1,
  inner: false,
  tooltip: p.tooltip,
});

/** Resolves a link to its two ports, or null if an id points nowhere. Port ids are shared across Input/Output/Parameter. */
export function resolveLink(
  link: Link,
  d: Diagram,
  parent: Box,
): { from: Endpoint; to: Endpoint } | null {
  const ports = (owner: number) => {
    const b = owner === 0 ? parent : d.boxes.find((x) => x.id === owner);
    return b ? [...b.inputs, ...b.outputs, ...b.params.map(paramPort)] : [];
  };
  const fromPort = ports(link.outOwner).find((p) => p.id === link.outPort);
  const toPort = ports(link.inOwner).find((p) => p.id === link.inPort);
  return fromPort && toPort
    ? {
        from: { owner: link.outOwner, port: fromPort },
        to: { owner: link.inOwner, port: toPort },
      }
    : null;
}

/** The diagram a box shows when opened: its first behavior keyframe's diagram. */
export const mainDiagram = (b: Box): Diagram | undefined =>
  b.timeline?.layers[0]?.keyframes[0]?.diagram;
