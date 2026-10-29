import { memo, useEffect, useId, type CSSProperties } from "react";
import { Settings2 } from "lucide-react";
import { boxCategory, categories, categoryStyle, durationSummary } from "./appearance";
import {
  Handle,
  Position,
  useUpdateNodeInternals,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import type { Box, Port } from "../format/types";
import type { GraphStyle } from "../workspace/storage";
import { useIcon } from "../icons";
import { speechLanguageName } from "../workspace/languages";
import { theme, typeColor } from "./theme";
import {
  HEADER,
  CARD_W,
  PORT,
  boxSize,
  cardAnchor,
  type Pt,
} from "./geometry";

// Glyph per port nature. Missing entries draw no glyph.
export const INPUT_GLYPH: Record<number, string> = {
  0: "input_onload",
  2: "input_onstart",
  3: "input_onstop",
  4: "input_stm",
};
export const OUTPUT_GLYPH: Record<number, string> = { 1: "output_onstopped" };
const io = (name: string | undefined) =>
  name ? `media/images/io/${name}.png` : undefined;

/** One port square with its type tab, on the side where the link attaches. */
export function PortGlyph({
  slot,
  port,
  rightTab,
  glyph,
}: {
  slot: Pt;
  port: Port;
  rightTab: boolean;
  glyph?: string;
}) {
  const url = useIcon(io(glyph));
  return (
    <g>
      <title>{`${port.name}${port.tooltip ? `\n${port.tooltip}` : ""}`}</title>
      <rect
        x={rightTab ? slot.x + PORT - 1 : slot.x - 3}
        y={slot.y + 2}
        width={4}
        height={PORT - 4}
        rx={1.5}
        fill={typeColor(port.type)}
      />
      <rect
        x={slot.x}
        y={slot.y}
        width={PORT}
        height={PORT}
        rx={3}
        fill="var(--surface)"
        stroke={theme.line}
      />
      {url ? (
        <image
          className="port-icon"
          href={url}
          x={slot.x + 2}
          y={slot.y + 2}
          width={12}
          height={12}
        />
      ) : (
        <g
          transform={`translate(${slot.x + 3},${slot.y + 3})`}
          fill="var(--node-line)"
          stroke="var(--node-line)"
          strokeWidth={1}
        >
          {glyph === "input_onstop" ? (
            <path d="M2 1 L9 8 M9 1 L2 8" fill="none" />
          ) : glyph === "input_parameter" ? (
            <path d="M1 5 H10 M5 1 V9" fill="none" />
          ) : glyph === "input_onload" ? (
            <path d="M5 0 L10 5 L5 10 L0 5 Z" fill="none" />
          ) : (
            <path d="M2 0 L10 5 L2 10 Z" />
          )}
        </g>
      )}
    </g>
  );
}

/** Invisible React Flow endpoint with a hit area covering its painted port button. */
export function Anchor({
  id,
  at,
  source,
  label,
  tooltip,
  isConnectable,
  zoom = 1,
  card = false,
  onActivate,
}: {
  id: string;
  at: Pt;
  source: boolean;
  label: string;
  tooltip?: string;
  isConnectable: boolean;
  zoom?: number;
  card?: boolean;
  onActivate?: () => void;
}) {
  return (
    <Handle
      id={id}
      className={onActivate ? "activatable" : undefined}
      type={source ? "source" : "target"}
      position={source ? Position.Right : Position.Left}
      isConnectable={isConnectable}
      isConnectableStart={isConnectable}
      isConnectableEnd={isConnectable}
      style={{ left: at.x, top: at.y, "--port-zoom": zoom, "--port-hit-width": card ? "20px" : "22px", "--port-hit-height": card ? "20px" : "18px" } as CSSProperties}
      role="button"
      aria-label={`${source ? "Connect from" : "Connect to"} ${label}`}
      aria-disabled={!isConnectable && !onActivate}
      title={[label, tooltip, onActivate && "Double-click or Alt+Enter to activate input"].filter(Boolean).join("\n")}
      tabIndex={isConnectable || onActivate ? 0 : -1}
      onKeyDown={(event) => {
        if (event.altKey && event.key === "Enter" && onActivate) {
          event.preventDefault();
          event.stopPropagation();
          onActivate();
          return;
        }
        if (event.key !== "Enter" && event.key !== " ") return;
        event.preventDefault();
        event.stopPropagation();
        if (isConnectable) event.currentTarget.click();
      }}
      onDoubleClick={(event) => {
        event.stopPropagation();
        onActivate?.();
      }}
    />
  );
}

let measure: CanvasRenderingContext2D | null = null;
export function titleText(name: string, width: number, font = "12px system-ui") {
  measure ??= document.createElement("canvas").getContext("2d");
  if (!measure) return name;
  measure.font = font;
  if (measure.measureText(name).width <= width) return name;
  let result = name;
  while (result.length && measure.measureText(`${result}…`).width > width)
    result = result.slice(0, -1);
  return `${result}…`;
}

function CardPort({ port, index, output, parameter }: {
  port: Port; index: number; output?: boolean; parameter?: boolean;
}) {
  const at = cardAnchor(index, output);
  const label = ({ onStart: "Start", onStop: "Stop", onStopped: "Done" } as Record<string, string>)[port.name] ?? port.name;
  const glyph = parameter ? "input_parameter" : (output ? OUTPUT_GLYPH : INPUT_GLYPH)[port.nature];
  return <g className="box-port" data-port={`${output ? "o" : "i"}${port.id}`} transform={`translate(${at.x + (output ? -10 : 10)},${at.y})`}>
    <title>{`${port.name}${port.tooltip ? `\n${port.tooltip}` : ""}`}</title>
    <rect x={output ? 8 : -11} y={-5} width={3} height={10} rx={1} fill={typeColor(port.type)} />
    <rect className="box-port-button" x={-8} y={-8} width={16} height={16} rx={4} />
    <g transform="scale(0.8)" fill="currentColor" stroke="currentColor" strokeWidth={1.5} strokeLinecap="round" strokeLinejoin="round">
      {glyph === "input_onstop" ? <rect x={-4.5} y={-4.5} width={9} height={9} rx={0.5} />
        : glyph === "input_parameter" ? <path d="M-5 0 H5 M0 -5 V5" fill="none" />
        : glyph === "input_onload" ? <path d="M0 -5 L5 0 L0 5 L-5 0 Z" fill="none" />
        : glyph === "input_stm" ? <path d="M-3 -5 L3 0 L-3 5 M2 -5 L7 0 L2 5" fill="none" />
        : glyph ? <path d="M-3 -5 L5 0 L-3 5 Z" />
        : <circle r={3} fill="none" />}
    </g>
    <text x={output ? -14 : 14} y={4} textAnchor={output ? "end" : "start"}>
      {titleText(label, (CARD_W - 36) / 2, "11px system-ui")}
    </text>
  </g>;
}

export type BoxNodeType = Node<{ box: Box; params: Port[]; graphStyle?: GraphStyle; onParameters?: () => void; onActivate?: (port: Port) => void }, "box">;

export const BoxNode = memo(function BoxNode({ id, data, selected, isConnectable }: NodeProps<BoxNodeType>) {
  const { box, params } = data;
  const clip = useId().replace(/:/g, "");
  const category = boxCategory(box);
  const { label, Icon } = categories[category];
  const inputs = [...box.inputs.filter((p) => !p.inner), ...params];
  const outputs = box.outputs.filter((p) => !p.inner);
  const hasText = box.plugin === "textedit_plugin";
  const stacked = category === "subgraph";
  const { w, h, cardW, cardH, textTop } = boxSize(inputs.length, outputs.length, hasText, stacked);
  const update = useUpdateNodeInternals();
  const handles = `${inputs.map((p) => p.id).join(",")}/${outputs.map((p) => p.id).join(",")}`;
  useEffect(() => update(id), [id, handles, w, h, update]);
  const customIcon = useIcon(category === "script" && !box.bitmaps[0]?.startsWith("media/images/box/box-") ? box.bitmaps[0] : undefined);
  const duration = durationSummary(box);
  const parameterLabel = `${box.params.length} param${box.params.length === 1 ? "" : "s"}`;
  const detail = box.plugin === "setlanguage_plugin" ? (box.pluginLanguage ? speechLanguageName(box.pluginLanguage) : "Language not set")
    : duration?.replace("Duration:", "Timer ·") ?? ({
    timeline: "Animation", script: "Custom script", comment: "Comment",
  } as Partial<Record<typeof category, string>>)[category] ?? label;
  return (
    <div className="box-card" data-category={category} style={{ ...categoryStyle(category), width: w, height: h }}>
      <svg width={w} height={h} overflow="visible" className="box-art">
        <title>{box.name}</title>
        <defs><clipPath id={clip}><rect width={cardW} height={cardH} rx={7} /></clipPath></defs>
        {stacked && [8, 4].map((offset) => <rect key={offset} className="box-stack" x={offset} y={offset} width={cardW} height={cardH} rx={7} />)}
        <rect className="box-surface" width={cardW} height={cardH} rx={7} />
        <rect className="box-header" width={cardW} height={3} clipPath={`url(#${clip})`} />
        {customIcon ? <image href={customIcon} x={9} y={10} width={14} height={14} />
          : <Icon className="box-category-icon" x={9} y={10} width={14} height={14} aria-hidden="true" />}
        <text className="box-title" x={29} y={21}>
          {titleText(box.name, cardW - 38, "600 12px system-ui")}
        </text>
        <g className="box-category">
          <title>{box.plugin === "setlanguage_plugin" ? `Robot language: ${detail}` : duration ? `${label} · ${duration}` : label}</title>
          <text x={9} y={44}>{titleText(detail, cardW - (box.params.length ? 88 : 18), "11px system-ui")}</text>
        </g>
        <path className="box-divider" d={`M1 52 H${cardW - 1}`} />
        {box.params.length > 0 && !data.onParameters && <text className="box-summary" x={cardW - 9} y={44} textAnchor="end">{parameterLabel}</text>}
        {inputs.map((port, index) => <CardPort key={`i${port.id}`} port={port} index={index} parameter={index >= inputs.length - params.length} />)}
        {outputs.map((port, index) => <CardPort key={`o${port.id}`} port={port} index={index} output />)}
        {selected && <rect className="box-selected" x={-3} y={-3} width={w + 6} height={h + 6} rx={10} />}
      </svg>
      {hasText && <BoxText box={box} top={textTop} bottom={h - cardH + 8} left={9} />}
      {box.params.length > 0 && data.onParameters && <button
        className="box-parameters nodrag nopan"
        style={{ left: cardW - 78, top: HEADER }}
        aria-label={`Parameters for ${box.name}`}
        title="Box parameters"
        onClick={(event) => { event.stopPropagation(); data.onParameters?.(); }}
        onDoubleClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => { if (event.key === "Enter" || event.key === " ") event.stopPropagation(); }}
      ><span>{parameterLabel}</span><Settings2 size={12} aria-hidden="true" /></button>}
      {inputs.map((port, index) => <Anchor
        key={`i${port.id}`} id={`i${port.id}`} card
        at={cardAnchor(index)} source={false} label={port.name} tooltip={port.tooltip}
        isConnectable={isConnectable}
        onActivate={index < inputs.length - params.length && data.onActivate ? () => data.onActivate?.(port) : undefined}
      />)}
      {outputs.map((port, index) => <Anchor
        key={`o${port.id}`} id={`o${port.id}`} card
        at={cardAnchor(index, true)} source label={port.name} tooltip={port.tooltip}
        isConnectable={isConnectable}
      />)}
    </div>
  );
});

/** Plain text only: imported comments must never execute markup. */
export function BoxText({ box, top, bottom = 8, left = 14 }: { box: Box; top: number; bottom?: number; left?: number }) {
  return <div className="box-comment nodrag nopan nowheel" style={{ top, bottom, left, right: 12 }}
    role="note" aria-label={`Comment in ${box.name}`} tabIndex={0}
    onDoubleClick={(event) => event.stopPropagation()}>
    {box.pluginText || <span className="muted">Empty comment</span>}
  </div>;
}
