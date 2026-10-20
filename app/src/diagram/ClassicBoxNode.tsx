import { memo, useEffect, useId } from "react";
import { useUpdateNodeInternals, type NodeProps } from "@xyflow/react";
import { useIcon } from "../icons";
import { theme } from "./theme";
import { BoxText, Anchor, PortGlyph, INPUT_GLYPH, OUTPUT_GLYPH, titleText, type BoxNodeType } from "./BoxNode";
import {
  CLASSIC_HEADER as HEADER,
  CLASSIC_NOTCH as NOTCH,
  CLASSIC_RADIUS as R,
  classicBoxSize,
  classicPortsHeight,
  classicInputSlot as inputSlot,
  classicOutputSlot as outputSlot,
  inputAnchor,
  outputAnchor,
} from "./geometry";

// Box icons from an installation are 40 × 60.
const ICON_W = 40;
const ICON_H = 60;

/** Classic box: a rounded header across the top, inputs in a notch on the left below it,
 *  outputs on the right, the icon in the body and the parameter button at its bottom left. */
export const ClassicBoxNode = memo(function ClassicBoxNode({
  id,
  data,
  selected,
  isConnectable,
}: NodeProps<BoxNodeType>) {
  const { box, params } = data;
  const clip = useId().replace(/:/g, "");
  const inputs = [...box.inputs.filter((p) => !p.inner), ...params];
  const outputs = box.outputs.filter((p) => !p.inner);
  const update = useUpdateNodeInternals();
  const handles = `${inputs.map((p) => p.id).join(",")}/${outputs.map((p) => p.id).join(",")}`;
  // Adding or removing a port does not always resize the box.
  const hasText = box.plugin === "textedit_plugin";
  const { w, h } = classicBoxSize(inputs.length, outputs.length, hasText);
  useEffect(() => update(id), [id, handles, w, h, update]);
  const icon = useIcon(box.bitmaps[0]);
  const wrench = useIcon(
    box.params.length ? "media/images/io/wrench.png" : undefined,
  );
  const title = w - 12;
  // the notch's inner corner is rounded too
  const outline =
    `M${R},0 H${w - R} A${R},${R} 0 0 1 ${w},${R} V${h - R} A${R},${R} 0 0 1 ${w - R},${h} ` +
    `H${NOTCH + R} A${R},${R} 0 0 1 ${NOTCH},${h - R} V${HEADER + 4} A4,4 0 0 0 ${NOTCH - 4},${HEADER} ` +
    `H${R} A${R},${R} 0 0 1 0,${HEADER - R} V${R} A${R},${R} 0 0 1 ${R},0 Z`;
  const iconLeft = NOTCH + (w - NOTCH - ICON_W) / 2;
  const iconTop = HEADER + Math.max(4, (h - HEADER - 22 - ICON_H) / 2);
  const label = titleText(box.name, title - 4, "600 11px system-ui");
  return (
    <div className="box-classic" style={{ width: w, height: h }}>
      <svg
        width={w}
        height={h}
        overflow="visible"
        style={{ position: "absolute", inset: 0 }}
      >
        <title>{box.name}</title>
        <defs>
          <clipPath id={clip}>
            <rect x={6} y={2} width={title} height={HEADER - 4} />
          </clipPath>
        </defs>
        {selected && (
          <rect
            x={-10}
            y={-8}
            width={w + 34}
            height={h + 16}
            rx={14}
            fill={theme.halo}
            opacity={0.12}
          />
        )}
        <path
          d={outline}
          fill="var(--surface)"
          stroke={theme.line}
          strokeWidth={1.5}
          strokeLinejoin="round"
        />
        <path d={`M${R},${HEADER} H${w - R}`} stroke={theme.line} strokeOpacity={0.25} />
        <text
          x={6 + title / 2}
          y={HEADER / 2}
          fontSize={11}
          clipPath={`url(#${clip})`}
          fontWeight={600}
          fontFamily="system-ui"
          textAnchor="middle"
          dominantBaseline="central"
          fill={theme.line}
        >
          {label}
        </text>
        {!hasText && (icon ? (
          <image
            href={icon}
            x={iconLeft}
            y={iconTop}
            width={ICON_W}
            height={ICON_H}
          />
        ) : (
          <g transform={`translate(${iconLeft},${iconTop})`}>
            <rect
              width={ICON_W}
              height={ICON_H}
              rx={8}
              fill="var(--node-icon-bg)"
            />
            <g
              transform="translate(8,18)"
              fill="none"
              stroke="var(--node-icon)"
              strokeWidth={1.6}
              strokeLinecap="round"
              strokeLinejoin="round"
            >
              {box.timeline?.enabled ? (
                <>
                  <path d="M2 5 H22 M2 12 H22 M2 19 H22" />
                  <rect x={6} y={2} width={5} height={6} rx={1} />
                  <rect x={14} y={9} width={5} height={6} rx={1} />
                  <rect x={7} y={16} width={5} height={6} rx={1} />
                </>
              ) : box.timeline?.layers.length ? (
                <>
                  <rect x={8} y={1} width={8} height={6} rx={1} />
                  <rect x={1} y={17} width={8} height={6} rx={1} />
                  <rect x={16} y={17} width={8} height={6} rx={1} />
                  <path d="M12 7 V12 H5 V17 M12 12 H20 V17" />
                </>
              ) : (
                <>
                  <path d="M7 6 L1 12 L7 18 M17 6 L23 12 L17 18 M14 3 L10 21" />
                </>
              )}
            </g>
          </g>
        ))}
        {wrench ? (
          <image href={wrench} x={NOTCH + 4} y={h - 20} width={16} height={16} />
        ) : (
          box.params.length > 0 && (
            <circle cx={NOTCH + 12} cy={h - 12} r={3} fill="var(--node-icon)" />
          )
        )}
        {inputs.map((p, i) => (
          <PortGlyph
            key={`i${p.id}`}
            slot={inputSlot(i)}
            port={p}
            rightTab={false}
            glyph={
              i < inputs.length - params.length
                ? INPUT_GLYPH[p.nature]
                : "input_parameter"
            }
          />
        ))}
        {outputs.map((p, j) => (
          <PortGlyph
            key={`o${p.id}`}
            slot={outputSlot(w, j)}
            port={p}
            rightTab
            glyph={OUTPUT_GLYPH[p.nature]}
          />
        ))}
      </svg>
      {hasText && <BoxText box={box} top={classicPortsHeight(inputs.length, outputs.length)} left={NOTCH + 8} bottom={box.params.length ? 26 : 10} />}
      {box.params.length > 0 && data.onParameters && (
        <button
          className="box-parameters nodrag nopan"
          style={{ left: NOTCH + 4, top: h - 20 }}
          aria-label={`Parameters for ${box.name}`}
          title="Box parameters"
          onClick={(event) => {
            event.stopPropagation();
            data.onParameters?.();
          }}
          onDoubleClick={(event) => event.stopPropagation()}
          onKeyDown={(event) => {
            if (event.key === "Enter" || event.key === " ") event.stopPropagation();
          }}
        />
      )}
      {inputs.map((p, i) => (
        <Anchor
          key={`i${p.id}`}
          id={`i${p.id}`}
          onActivate={i < inputs.length - params.length && data.onActivate ? () => data.onActivate?.(p) : undefined}
          at={inputAnchor(inputSlot(i))}
          source={false}
          label={p.name}
          tooltip={p.tooltip}
          isConnectable={isConnectable}
        />
      ))}
      {outputs.map((p, j) => (
        <Anchor
          key={`o${p.id}`}
          id={`o${p.id}`}
          at={outputAnchor(outputSlot(w, j))}
          source
          label={p.name}
          tooltip={p.tooltip}
          isConnectable={isConnectable}
        />
      ))}
    </div>
  );
});
