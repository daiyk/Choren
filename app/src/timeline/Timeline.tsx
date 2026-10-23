import { useEffect, useMemo, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import type { Timeline as TimelineModel } from "../format/types";
import type { TimelineView } from "../workspace/model";
import { useContextMenu, type MenuAction } from "../workspace/ContextMenu";
export interface TimelineMenuTarget { layer?: number; keyframe?: number; frame: number }
const LABEL = 144,
  RULER = 26,
  ROW = 30;
interface Props {
  t: TimelineModel;
  active: { layer: number; keyframe: number };
  value: TimelineView;
  robotFrame?: number;
  onChange: (value: TimelineView) => void;
  onMoveKeyframe?: (node: number, delta: number) => void;
  menuIdentity?: string;
  onMenuError?: (message: string) => void;
  menuActions?: (target: TimelineMenuTarget) => MenuAction[];
  onPick: (layer: number, keyframe: number, frame?: number) => void;
}
export function Timeline({ t, active, value, robotFrame, onChange, onPick, onMoveKeyframe, menuIdentity, menuActions, onMenuError }: Props) {
  const drag = useRef<{node?: number; start: number; x: number; frame: number; moved: boolean} | undefined>(undefined);
  const suppressClick = useRef(false);
  const menu = useContextMenu([menuIdentity, t], onMenuError);
  const host = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState({ width: 640, height: 160 });
  const motion = useMemo(
    () => [...new Set(t.curves.flatMap((c) => c.frames))].sort((a, b) => a - b),
    [t],
  );
  const frames = useMemo(() => {
    let last = Math.max(1, t.size, motion.at(-1) ?? 1);
    for (const layer of t.layers)
      for (const keyframe of layer.keyframes)
        last = Math.max(last, keyframe.index);
    return last;
  }, [t, motion]);
  const maxScroll = Math.max(
    0,
    frames * value.scale - Math.max(1, size.width - LABEL),
  );
  const scroll = Math.min(value.scroll, maxScroll);
  const [rowScroll, setRowScroll] = useState(0);
  const rowCount = t.layers.length + (motion.length ? 1 : 0);
  const maxRowScroll = Math.max(0, rowCount * ROW - (size.height - RULER));
  const rowOffset = Math.min(rowScroll, maxRowScroll);
  useEffect(() => {
    const observer = new ResizeObserver(([entry]) => {
      if (entry.contentRect.width > 0 && entry.contentRect.height > 0)
        setSize({
          width: entry.contentRect.width,
          height: entry.contentRect.height,
        });
    });
    if (host.current) observer.observe(host.current);
    return () => observer.disconnect();
  }, []);
  useEffect(() => {
    const target = canvas.current,
      ctx = target?.getContext("2d");
    if (!target || !ctx) return;
    const draw = () => {
      const dpr = window.devicePixelRatio || 1;
      target.width = Math.ceil(size.width * dpr);
      target.height = Math.ceil(size.height * dpr);
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      const css = getComputedStyle(target);
      const color = (name: string) => css.getPropertyValue(name).trim();
      ctx.fillStyle = color("--surface");
      ctx.fillRect(0, 0, size.width, size.height);
      ctx.font = "11px system-ui";
      ctx.textBaseline = "middle";
      const x = (frame: number) => LABEL + (frame - 1) * value.scale - scroll;
      ctx.save();
      ctx.beginPath();
      ctx.rect(LABEL, 0, Math.max(0, size.width - LABEL), size.height);
      ctx.clip();
      const step = Math.max(1, Math.ceil(64 / value.scale / 5) * 5);
      const first = Math.max(1, Math.floor(scroll / value.scale / step) * step);
      for (let f = first; x(f) < size.width; f += step) {
        ctx.strokeStyle = color("--border");
        ctx.beginPath();
        ctx.moveTo(x(f), RULER);
        ctx.lineTo(x(f), size.height);
        ctx.stroke();
        ctx.fillStyle = color("--muted");
        ctx.fillText(String(f), x(f) + 4, RULER / 2);
      }
      const rangeStart = t.startFrame ?? 1;
      const rangeEnd = (t.endFrame ?? -1) < 0 ? frames : t.endFrame!;
      ctx.fillStyle = color("--timeline-selected");
      ctx.fillRect(x(rangeStart), 0, Math.max(1, (rangeEnd - rangeStart + 1) * value.scale), 3);
      ctx.save();
      ctx.beginPath();
      ctx.rect(LABEL, RULER, size.width, size.height - RULER);
      ctx.clip();
      t.layers.forEach((layer, li) => {
        const y = RULER + li * ROW - rowOffset;
        if (y + ROW < RULER || y > size.height) return;
        layer.keyframes.forEach((kf, ki) => {
          const end = layer.keyframes[ki + 1]?.index ?? frames + 1;
          const left = x(kf.index),
            right = x(end) - 2;
          if (right < LABEL || left > size.width) return;
          ctx.fillStyle = color(
            li === active.layer && ki === active.keyframe
              ? "--timeline-selected"
              : "--timeline-block",
          );
          ctx.fillRect(
            Math.max(LABEL, left),
            y + 3,
            Math.max(1, Math.min(size.width, right) - Math.max(LABEL, left)),
            ROW - 6,
          );
          ctx.save();
          ctx.beginPath();
          ctx.rect(
            Math.max(LABEL, left),
            y,
            Math.max(0, right - Math.max(LABEL, left)),
            ROW,
          );
          ctx.clip();
          ctx.fillStyle = color("--text");
          ctx.fillText(
            kf.name || `Frame ${kf.index}`,
            Math.max(LABEL, left) + 7,
            y + ROW / 2,
          );
          ctx.restore();
        });
      });
      const my = RULER + t.layers.length * ROW - rowOffset;
      if (motion.length && my + ROW > RULER && my < size.height) {
        ctx.fillStyle = color("--accent");
        // Binary search skips offscreen motion keys, even in very long timelines.
        const firstFrame = scroll / value.scale + 1;
        let lo = 0,
          hi = motion.length;
        while (lo < hi) {
          const mid = (lo + hi) >>> 1;
          if (motion[mid] < firstFrame) lo = mid + 1;
          else hi = mid;
        }
        for (let i = lo; i < motion.length && x(motion[i]) <= size.width; i++)
          ctx.fillRect(x(motion[i]) - 2, my + 9, 4, ROW - 18);
      }
      ctx.restore();
      ctx.strokeStyle = color("--accent");
      ctx.lineWidth = 1.5;
      ctx.beginPath();
      if (robotFrame !== undefined) {
        ctx.strokeStyle = color("--warning");
        ctx.setLineDash([5, 3]);
        ctx.moveTo(x(robotFrame), 0); ctx.lineTo(x(robotFrame), size.height); ctx.stroke();
        ctx.setLineDash([]); ctx.strokeStyle = color("--accent"); ctx.beginPath();
      }
      ctx.moveTo(x(value.frame), 0);
      ctx.lineTo(x(value.frame), size.height);
      ctx.stroke();
      ctx.restore();
      ctx.fillStyle = color("--surface-raised");
      ctx.fillRect(0, 0, LABEL, size.height);
      ctx.fillStyle = color("--muted");
      ctx.fillText("LAYERS", 12, RULER / 2);
      ctx.save();
      ctx.beginPath();
      ctx.rect(0, RULER, LABEL - 8, size.height - RULER);
      ctx.clip();
      [
        ...t.layers.map((l) => `${l.mute ? "[muted] " : ""}${l.name}`),
        ...(motion.length ? ["Motion keys"] : []),
      ].forEach((name, i) => {
        const y = RULER + i * ROW - rowOffset;
        if (y + ROW < RULER || y > size.height) return;
        ctx.fillStyle = color("--text");
        ctx.fillText(name, 12, y + ROW / 2, LABEL - 24);
      });
      ctx.restore();
    };
    draw();
    const observer = new MutationObserver(draw);
    observer.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["data-theme"],
    });
    window.addEventListener("resize", draw);
    return () => {
      observer.disconnect();
      window.removeEventListener("resize", draw);
    };
  }, [
    t,
    value,
    robotFrame,
    scroll,
    rowOffset,
    size,
    frames,
    active.layer,
    active.keyframe,
    motion,
  ]);
  const zoom = (factor: number) => {
    const scale = Math.max(0.25, Math.min(48, value.scale * factor));
    onChange({ ...value, scale, scroll: (scroll * scale) / value.scale });
  };
  const actions = (target: TimelineMenuTarget): MenuAction[] => [
    ...(menuActions?.(target) ?? []),
    { label: "Zoom in", run: () => zoom(1.25) },
    { label: "Zoom out", run: () => zoom(0.8) },
  ];
  const openMenu = (event: React.MouseEvent | React.KeyboardEvent, target: TimelineMenuTarget) => {
    drag.current = undefined;
    suppressClick.current = false;
    menu.open(event, actions(target));
  };
  return (
    <section className="timeline-tool" aria-label="Timeline" {...menu.trigger(() => actions({frame: value.frame}))}>
      {menu.element}
      <div className="panel-toolbar timeline-toolbar">
        <label>
          Frame{" "}
          <input
            aria-label="Current frame"
            type="number"
            min={1}
            max={frames}
            value={value.frame}
            onChange={(e) =>
              onChange({
                ...value,
                frame: Math.min(
                  frames,
                  Math.max(1, Math.round(Number(e.target.value) || 1)),
                ),
              })
            }
          />
        </label>
        <span className="muted">
          / {frames} · {t.fps} fps
          {robotFrame !== undefined && <span aria-label="Robot timeline frame"> · Robot: {robotFrame}</span>}
        </span>
        <span className="spacer" />
        <button aria-label="Timeline zoom out" onClick={() => zoom(0.8)}>
          <Minus size={14} />
        </button>
        <button aria-label="Timeline zoom in" onClick={() => zoom(1.25)}>
          <Plus size={14} />
        </button>
      </div>
      <div className="timeline-keyframes">
        <button aria-label="Timeline layer actions" disabled={!t.layers.length}
          onClick={e => openMenu(e, {layer: active.layer, frame: value.frame})}
          {...menu.trigger(() => actions({layer: active.layer, frame: value.frame}))}>Layer actions…</button>
        <button aria-label="Timeline keyframe actions" disabled={!t.layers[active.layer]?.keyframes.length}
          onClick={e => openMenu(e, {layer: active.layer, keyframe: active.keyframe, frame: value.frame})}
          {...menu.trigger(() => actions({layer: active.layer, keyframe: active.keyframe, frame: value.frame}))}>Keyframe actions…</button>
        <label>
          Layer{" "}
          <select
            aria-label="Timeline layer"
            value={t.layers.length ? active.layer : ""}
            disabled={!t.layers.length}
            onChange={(e) => onPick(Number(e.target.value), 0)}
          >
            {!t.layers.length && <option value="">Motion only</option>}
            {t.layers.map((layer, i) => (
              <option key={i} value={i}>
                {layer.name}
              </option>
            ))}
          </select>
        </label>
        <label>
          Keyframe{" "}
          <select
            aria-label="Timeline keyframe"
            value={
              t.layers[active.layer]?.keyframes.length ? active.keyframe : ""
            }
            disabled={!t.layers[active.layer]?.keyframes.length}
            onChange={(e) => {
              const index = Number(e.target.value);
              onPick(active.layer, index);
            }}
          >
            {!t.layers[active.layer]?.keyframes.length && (
              <option value="">No behavior keyframes</option>
            )}
            {t.layers[active.layer]?.keyframes.map((kf, i) => (
              <option key={i} value={i}>
                {kf.index} · {kf.name}
              </option>
            ))}
          </select>
        </label>
      </div>
      <div
        className="timeline-viewport"
        ref={host}
        onWheel={(e) => {
          menu.close();
          if (e.ctrlKey || e.metaKey) zoom(e.deltaY < 0 ? 1.1 : 0.9);
          else if (
            e.shiftKey ||
            Math.abs(e.deltaX) > Math.abs(e.deltaY) ||
            maxRowScroll === 0
          )
            onChange({
              ...value,
              scroll: Math.max(
                0,
                Math.min(maxScroll, scroll + (e.deltaX || e.deltaY)),
              ),
            });
          else
            setRowScroll(
              Math.max(0, Math.min(maxRowScroll, rowOffset + e.deltaY)),
            );
        }}
      >
        <canvas
          ref={canvas}
          style={{ width: size.width, height: size.height }}
          aria-label="Timeline overview; use the frame, layer and keyframe controls to navigate"
          tabIndex={0}
          onKeyDown={e => {
            if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10"))
              openMenu(e, {layer: active.layer, keyframe: active.keyframe, frame: value.frame});
          }}
          onContextMenu={e => {
            const rect = e.currentTarget.getBoundingClientRect();
            const x = e.clientX - rect.left, y = e.clientY - rect.top;
            const frame = x < LABEL ? value.frame : Math.min(frames, Math.max(1, Math.round((x - LABEL + scroll) / value.scale) + 1));
            const li = Math.floor((y - RULER + rowOffset) / ROW);
            const layer = y >= RULER && li >= 0 && li < t.layers.length ? li : undefined;
            const ki = layer !== undefined && x >= LABEL ? t.layers[layer].keyframes.findIndex(k => Math.abs(LABEL + (k.index - 1) * value.scale - scroll - x) <= 7) : -1;
            openMenu(e, {frame, layer, keyframe: ki >= 0 ? ki : undefined});
          }}
          onPointerDown={e => {
            const rect = e.currentTarget.getBoundingClientRect();
            const x = e.clientX - rect.left, y = e.clientY - rect.top;
            if (x < LABEL || e.button !== 0) return;
            const frame = Math.max(1, Math.round((x - LABEL + scroll) / value.scale) + 1);
            const li = Math.floor((y - RULER + rowOffset) / ROW);
            const kf = y >= RULER && onMoveKeyframe ? t.layers[li]?.keyframes.find(k => Math.abs(LABEL + (k.index - 1) * value.scale - scroll - x) <= 7) : undefined;
            if (y >= RULER && !kf) return;
            e.currentTarget.setPointerCapture(e.pointerId);
            drag.current = {node: kf?.node, start: kf?.index ?? frame, frame: kf?.index ?? frame, x: e.clientX, moved: false};
          }}
          onPointerMove={e => {
            const d = drag.current; if (!d) return;
            const delta = Math.round((e.clientX - d.x) / value.scale);
            d.frame = Math.max(1, d.start + delta); d.moved ||= Math.abs(e.clientX - d.x) > 3;
            if (d.moved) onChange({...value, frame: d.frame});
          }}
          onPointerUp={() => {
            const d = drag.current; drag.current = undefined;
            suppressClick.current = !!d?.moved;
            if (d?.moved && d.node !== undefined && d.frame !== d.start) onMoveKeyframe?.(d.node, d.frame - d.start);
          }}
          onPointerCancel={() => {drag.current = undefined; suppressClick.current = false;}}
          onClick={(e) => {
            if (suppressClick.current) {suppressClick.current = false; return;}
            const rect = e.currentTarget.getBoundingClientRect();
            const x = e.clientX - rect.left,
              y = e.clientY - rect.top;
            if (x < LABEL) return;
            const frame = Math.min(
              frames,
              Math.max(1, Math.floor((x - LABEL + scroll) / value.scale) + 1),
            );
            onChange({ ...value, frame });
            if (y < RULER) return;
            const layer = Math.floor((y - RULER + rowOffset) / ROW);
            const keyframe =
              t.layers[layer]?.keyframes.findLastIndex(
                (kf) => kf.index <= frame,
              ) ?? -1;
            if (keyframe >= 0) onPick(layer, keyframe, frame);
          }}
        />
      </div>
      <input
        className="timeline-scroll"
        aria-label="Timeline horizontal position"
        type="range"
        min={0}
        max={maxScroll || 1}
        step={1}
        value={scroll}
        disabled={!maxScroll}
        onChange={(e) => onChange({ ...value, scroll: Number(e.target.value) })}
      />
    </section>
  );
}
