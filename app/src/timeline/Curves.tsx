import { curvePath, selectedFrameRanges, playbackFrame } from "./curveMath";
import { motionCode } from "./motionExport";
import { Modal } from "../workspace/Modal";
import { writeTextFile } from "@tauri-apps/plugin-fs";
import { useEffect, useRef, useState } from "react";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { ActuatorCurve, CurveKey } from "../format/types";
import type { EditCommand, Interp, SimplifyMode } from "../workspace/authoring";
import { authoring } from "../workspace/authoring";
import { useContextMenu, type MenuAction, type MenuEvent } from "../workspace/ContextMenu";
import { useWorkspace } from "../workspace/context";
import { graphId, resolveAddress } from "../workspace/model";
import { useRobot } from "../robot/store";

/** The open timeline box and its shared playhead view (Timeline and Motion panels). */
export function useActiveTimeline() {
  const ws = useWorkspace();
  const address = ws.state.activeBehavior
    ? ws.state.addresses[ws.state.activeBehavior]
    : undefined;
  const resolved = resolveAddress(ws.state.project, address);
  const robotFrame = useRobot(s => {
    const played = s.played;
    if (!s.status || !played || played.session !== ws.state.project?.session || played.behavior !== address?.behaviorId || !s.running.includes(played.name)) return undefined;
    const frame = s.frames[`${played.name}|${address.levels.slice(1).map(l => l.boxId).join("/")}`];
    const t = resolved?.owner.timeline;
    return t ? playbackFrame(t, frame) : undefined;
  });
  if (!resolved?.owner.timeline?.enabled || !ws.state.project || !address)
    return undefined;
  const key = graphId(ws.state.project.id, {
    ...address,
    levels: [
      ...address.levels.slice(0, -1),
      { ...address.levels.at(-1)!, layer: 0, keyframe: 0 },
    ],
  });
  const value = ws.state.timelines[key] ?? { frame: 1, scale: 8, scroll: 0 };
  return { address, resolved, key, value, robotFrame, t: resolved.owner.timeline };
}

export { autoTangents, curvePath } from "./curveMath";
const interpOf = (k: CurveKey): Interp =>
  !k.right
    ? "BezierAuto"
    : k.right.interp === "linear"
      ? "Linear"
      : k.right.interp === "constant"
        ? "Constant"
        : "Bezier";
const GROUPS = ["Whole body", "Head", "Arms", "Legs"];

function CurvePlot({ curve, frame, robotFrame, last, selected, onSelect, onEdit, editable, onMenu }: {
  curve: ActuatorCurve; frame: number; robotFrame?: number; last: number; selected: number[];
  onMenu: (event: MenuEvent, key?: CurveKey, side?: "left" | "right", frame?: number) => void;
  onSelect: (keys: number[]) => void; onEdit: (label: string, commands: EditCommand[]) => void; editable: boolean;
}) {
  const keys = curve.keys ?? [];
  type Drag = { nodes: number[]; key: CurveKey; side?: "left" | "right"; x: number; y: number; sx: number; sy: number; dx: number; dy: number };
  const drag = useRef<Drag | undefined>(undefined);
  const [preview, setPreview] = useState<Drag>();
  const values = keys.flatMap(k => [k.value, k.value + (k.left?.ordinate ?? 0), k.value + (k.right?.ordinate ?? 0)]);
  const [lo, hi] = [Math.min(0, ...values), Math.max(1, ...values)];
  const pad = (hi - lo) * 0.1 || 1;
  const [W, H] = [1000, 180];
  const sx = W / (Math.max(last, frame, ...keys.map(k => k.frame + (k.right?.abscissa ?? 0)), 10) + 2);
  const sy = H / (hi - lo + 2 * pad);
  const x = (f: number) => f * sx;
  const y = (v: number) => (hi + pad - v) * sy;
  const shown = keys.map(k => preview?.nodes.includes(k.node) && !preview.side ? {...k, frame: k.frame + preview.dx, value: k.value + preview.dy} : k);
  const begin = (e: React.PointerEvent<SVGElement>, k: CurveKey, side?: "left" | "right") => {
    if (e.button !== 0) return;
    e.stopPropagation();
    const nodes = e.shiftKey ? [...new Set([...selected, k.node])] : selected.includes(k.node) ? selected : [k.node];
    onSelect(nodes);
    if (!editable) return;
    const rect = e.currentTarget.ownerSVGElement!.getBoundingClientRect();
    e.currentTarget.setPointerCapture(e.pointerId);
    drag.current = {nodes, key: k, side, x: e.clientX, y: e.clientY, sx: sx * rect.width / W, sy: sy * rect.height / H, dx: 0, dy: 0};
  };
  const move = (e: React.PointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = (e.clientX - d.x) / d.sx;
    const min = Math.min(...keys.filter(k => d.nodes.includes(k.node)).map(k => k.frame));
    drag.current = {...d, dx: d.side ? dx : Math.max(-min, Math.round(dx)), dy: (d.y - e.clientY) / d.sy};
    setPreview(drag.current);
  };
  const end = () => {
    const d = drag.current;
    drag.current = undefined; setPreview(undefined);
    if (!d || (!d.dx && !d.dy)) return;
    if (d.side) {
      const tg = d.key[d.side]!;
      onEdit("Move tangent", [{type: "moveTangent", tangent: tg.node,
        abscissa: d.side === "left" ? Math.min(0, tg.abscissa + d.dx) : Math.max(0, tg.abscissa + d.dx), ordinate: tg.ordinate + d.dy}]);
    } else onEdit("Move curve keys", [
      ...(d.dx ? [{type: "moveKeys" as const, keys: d.nodes, delta: d.dx}] : []),
      ...(d.dy ? [{type: "setCurveValues" as const, keys: d.nodes, scale: 1, offset: d.dy}] : []),
    ]);
  };
  const showMenu = (event: MenuEvent, key?: CurveKey, side?: "left" | "right") => {
    drag.current = undefined; setPreview(undefined);
    const rect = (event.currentTarget instanceof SVGSVGElement ? event.currentTarget : event.currentTarget instanceof SVGElement ? event.currentTarget.ownerSVGElement : undefined)?.getBoundingClientRect();
    const at = !key && "clientX" in event && rect ? Math.max(0, Math.round((event.clientX - rect.left) * W / rect.width / sx)) : frame;
    onMenu(event, key, side, at);
  };
  return <svg className="curve-plot" aria-label={`${curve.actuator} curve`} viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" tabIndex={0}
    onContextMenu={e => showMenu(e)}
    onKeyDown={e => {if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) showMenu(e);}}
    onPointerMove={move} onPointerUp={end} onPointerCancel={() => {drag.current = undefined; setPreview(undefined);}}
    onPointerDown={e => {if (e.button === 0 && e.target === e.currentTarget) onSelect([]);}}>
    <line x1={0} x2={W} y1={y(0)} y2={y(0)} className="curve-axis" />
    <line x1={x(frame)} x2={x(frame)} y1={0} y2={H} className="curve-playhead" />
    {robotFrame !== undefined && <line aria-label="Robot curve playhead" data-frame={robotFrame} x1={x(robotFrame)} x2={x(robotFrame)} y1={0} y2={H} className="robot-playhead" />}
    <path d={curvePath(shown)} transform={`matrix(${sx} 0 0 ${-sy} 0 ${(hi + pad) * sy})`} className="curve-line" vectorEffect="non-scaling-stroke" />
    {shown.map(k => <g key={k.node}>
      {selected.includes(k.node) && (["left", "right"] as const).map(side => {
        const tg = k[side];
        if (tg?.interp !== "bezier") return null;
        const active = preview?.key.node === k.node && preview.side === side;
        const tx = x(k.frame + (active ? (side === "left" ? Math.min(0, tg.abscissa + preview.dx) : Math.max(0, tg.abscissa + preview.dx)) : tg.abscissa));
        const ty = y(k.value + tg.ordinate + (active ? preview.dy : 0));
        return <g key={side}><line x1={x(k.frame)} y1={y(k.value)} x2={tx} y2={ty} className="curve-axis" />
          <circle cx={tx} cy={ty} r={5} className="curve-handle" tabIndex={0} role="button" aria-label={`Edit key ${k.frame} ${side} tangent`}
            onContextMenu={e => showMenu(e, k, side)}
            onKeyDown={e => {if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) showMenu(e, k, side);}}
            onPointerDown={e => begin(e, keys.find(v => v.node === k.node)!, side)}><title>{side} tangent</title></circle></g>;
      })}
      <rect x={x(k.frame) - 5} y={y(k.value) - 5} width={10} height={10} className={`curve-key${selected.includes(k.node) ? " selected" : ""}`}
        tabIndex={0} role="button" aria-label={`Select key at frame ${k.frame}`} aria-pressed={selected.includes(k.node)}
        onContextMenu={e => showMenu(e, k)}
        onPointerDown={e => begin(e, keys.find(v => v.node === k.node)!)}
        onKeyDown={e => {if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {showMenu(e, k); return;} if (e.key === "Enter" || e.key === " ") {e.preventDefault(); onSelect(e.shiftKey ? [...new Set([...selected, k.node])] : [k.node]);}}} />
    </g>)}
  </svg>;
}

function Num({
  label,
  value,
  onCommit,
  disabled,
}: {
  label: string;
  value: number;
  onCommit: (v: number) => void;
  disabled?: boolean;
}) {
  return (
    <input
      type="number"
      step="any"
      aria-label={label}
      disabled={disabled}
      defaultValue={value}
      key={value}
      onBlur={(e) => {
        const v = Number(e.target.value);
        if (e.target.value !== "" && Number.isFinite(v) && v !== value)
          onCommit(v);
      }}
      onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
    />
  );
}

/** "Motion curves / dope sheet": per-joint keys, values, tangents and interpolation. */
export function MotionPanel() {
  const ws = useWorkspace();
  const active = useActiveTimeline();
  const projectRef = useRef(ws.state.project);
  projectRef.current = ws.state.project;
  const angles = useRobot((s) => s.state?.angles);
  const [selected, setSelected] = useState<string>();
  const [group, setGroup] = useState("Whole body");
  const [chosen, setChosen] = useState<number[]>([]);
  const [clipboard, setClipboard] = useState("");
  const [offset, setOffset] = useState(0);
  const [scale, setScale] = useState(1);
  const [tolerance, setTolerance] = useState(0.1);
  const [simplifier, setSimplifier] = useState<SimplifyMode>("Choren");
  const [exportCode, setExportCode] = useState(false);
  const host = useRef<HTMLDivElement>(null);
  const menu = useContextMenu([ws.state.session, active?.key, ws.history?.revision], message => ws.notify(message, "error"));
  useEffect(() => setChosen([]), [active?.key]);
  if (!active)
    return (
      <div className="empty-state">
        <h2>No timeline selected</h2>
        <p>Open a timeline box to edit its motion curves.</p>
      </div>
    );
  const { t, value, key, resolved } = active;
  const frame = value.frame;
  const editable = ws.editable && t.node !== undefined;
  const curve = t.curves.find((c) => c.actuator === selected) ?? t.curves[0];
  const chosenKeys = (curve?.keys ?? []).filter(k => chosen.includes(k.node)).map(k => k.node);
  const last = Math.max(t.size, frame, ...t.curves.flatMap((c) => c.frames), 1);
  const run = (label: string, commands: Parameters<typeof ws.edit>[1]) =>
    void ws.edit(label, commands, resolved.behavior.id);
  const session = ws.state.project?.session;
  const behavior = resolved.behavior.id;
  const qianim = async (direction: "import" | "export") => {
    const expectedProject = projectRef.current;
    if (!session || t.node === undefined) return;
    const filters = [{ name: "Animation", extensions: ["qianim"] }];
    const path =
      direction === "import"
        ? await open({ filters })
        : await save({ filters, defaultPath: `${resolved.owner.name}.qianim` });
    if (!path || projectRef.current !== expectedProject) return;
    if (direction === "export")
      await authoring.exportQianim(session, behavior, t.node, path);
    else
      await ws.applyChange(() =>
        authoring.importQianim(session, behavior, t.node!, path),
      );
  };
  const readOnly = "Open an editable project to change motion keys.";
  const revealKey = (c: ActuatorCurve, k: CurveKey, side?: "left" | "right") => {
    setSelected(c.actuator);
    requestAnimationFrame(() => {
      const row = host.current?.querySelector<HTMLElement>(`[data-curve-key="${k.node}"]`);
      row?.scrollIntoView({block: "nearest"});
      (side ? row?.querySelector<HTMLElement>(`[aria-label="Key ${k.frame} ${side} tangent frames"]`) : row?.querySelector<HTMLElement>("input"))?.focus();
    });
  };
  const revealTransform = (c: ActuatorCurve) => {
    setSelected(c.actuator);
    requestAnimationFrame(() => {
      const details = host.current?.querySelector<HTMLDetailsElement>(".curve-transform");
      if (details) {details.open = true; details.scrollIntoView({block: "nearest"}); details.querySelector("input")?.focus();}
    });
  };
  const curveActions = (c: ActuatorCurve): MenuAction[] => [
    {label: "Select all keys", run: () => {setSelected(c.actuator); setChosen(c.keys?.map(k => k.node) ?? []);}},
    {label: c.mute ? "Unmute curve" : "Mute curve", checked: !!c.mute, disabled: !editable || c.node === undefined, reason: readOnly,
      run: () => run("Mute curve", [{type: "setAttributes", node: c.node!, values: {mute: c.mute ? "0" : "1"}}])},
    {label: "Transform and simplify…", disabled: !editable || !c.keys?.length, reason: !editable ? readOnly : "This curve has no keys.", run: () => revealTransform(c)},
  ];
  const keyActions = (c: ActuatorCurve, k: CurveKey, side?: "left" | "right"): MenuAction[] => {
    const nodes = c === curve && chosenKeys.includes(k.node) ? chosenKeys : [k.node];
    setSelected(c.actuator); setChosen(nodes);
    if (side) return [
      {label: "Edit tangent…", disabled: !editable, reason: readOnly, run: () => revealKey(c, k, side)},
      {label: "Key properties…", run: () => revealKey(c, k)},
    ];
    return [
      {label: nodes.length > 1 ? `Copy ${nodes.length} keys` : "Copy key", disabled: !editable, reason: readOnly, run: () => authoring.copy(session!, behavior, nodes).then(setClipboard)},
      {label: "Key properties…", run: () => revealKey(c, k)},
      ...([ ["Constant", "Constant"], ["Linear", "Linear"], ["Bezier", "Bezier"], ["BezierAuto", "Automatic"] ] as const).map(([interp, label]): MenuAction => ({
        label: `${label} interpolation`, disabled: !editable, reason: readOnly, run: () => run("Change interpolation", [{type: "setInterpolation", keys: nodes, interp}]),
      })),
      {label: "Transform selected keys…", disabled: !editable, reason: readOnly, run: () => revealTransform(c)},
      {label: nodes.length > 1 ? `Delete ${nodes.length} keys` : "Delete key", disabled: !editable, reason: readOnly, run: () => run("Delete selected keys", [{type: "remove", nodes}])},
    ];
  };
  const trackActions = (c: ActuatorCurve, at: number): MenuAction[] => [
    {label: `Add key at frame ${at}`, disabled: !editable || c.node === undefined || c.frames.includes(at), reason: !editable ? readOnly : "A key already exists at this frame.", run: () => run("Add keys", [{type: "insertXml", parent: c.node!, xml: `<Key frame="${at}" value="${c.values?.findLast((_, i) => c.frames[i] <= at) ?? c.values?.[0] ?? 0}" />`}])},
    {label: `Paste keys at frame ${at} (original joints)`, disabled: !editable || !clipboard, reason: !editable ? readOnly : "Copy keys first.", run: () => run("Paste curve keys", [{type: "pasteCurveKeys", timeline: t.node!, xml: clipboard, frame: at}])},
    ...curveActions(c),
  ];
  const backgroundActions = (): MenuAction[] => [
    {label: `Store ${group.toLowerCase()} joints at frame ${frame}`, disabled: !editable || !angles, reason: !editable ? readOnly : "Connect a robot or start the simulator to read joints.", run: () => run("Store joints", [{type: "storeKeys", timeline: t.node!, frame, group, angles: angles!}])},
    {label: `Remove ${group.toLowerCase()} joints at frame ${frame}`, disabled: !editable, reason: readOnly, run: () => run("Remove joints", [{type: "removeKeys", timeline: t.node!, frame, group}])},
    ...([["Flip", "Flip"], ["LeftToRight", "Mirror left to right"], ["RightToLeft", "Mirror right to left"]] as const).map(([mode, label]): MenuAction => ({label: `${label} (${chosenKeys.length ? "selected frames" : "whole motion"})`, disabled: !editable || !t.curves.length, reason: !editable ? readOnly : "This timeline has no motion.", run: () => run(label, chosenKeys.length ? selectedFrameRanges(curve?.keys ?? [], chosenKeys).map(frames => ({type: "mirror", timeline: t.node!, mode, frames})) : [{type: "mirror", timeline: t.node!, mode}])})),
    {label: "Import .qianim…", disabled: !editable, reason: readOnly, run: () => qianim("import")},
    {label: "Export .qianim…", disabled: !editable, reason: readOnly, run: () => qianim("export")},
    {label: "Export motion code…", disabled: !t.curves.some(c => c.keys?.length), reason: "This timeline has no motion keys.", run: () => setExportCode(true)},
  ];
  return (
    <div ref={host} className="panel-column motion-panel" tabIndex={0} aria-label="Motion editor actions" {...menu.trigger(backgroundActions)}>
      {menu.element}
      <div className="panel-toolbar" role="toolbar" aria-label="Dope sheet">
        <label>
          Frame{" "}
          <input
            type="number"
            min={0}
            aria-label="Motion frame"
            value={frame}
            onChange={(e) =>
              ws.saveTimeline(key, {
                ...value,
                frame: Math.max(0, Math.round(Number(e.target.value) || 0)),
              })
            }
          />
        </label>
        <select
          aria-label="Joint group"
          value={group}
          onChange={(e) => setGroup(e.target.value)}
        >
          {GROUPS.map((g) => (
            <option key={g}>{g}</option>
          ))}
        </select>
        <button
          disabled={!editable || !angles}
          title={
            angles
              ? `Store the robot's ${group.toLowerCase()} joints at frame ${frame}`
              : "Connect a robot or start the virtual NAO 6 to read joint values"
          }
          onClick={() =>
            run(`Store joints in keyframe ${frame}`, [
              {
                type: "storeKeys",
                timeline: t.node!,
                frame,
                group,
                angles: angles!,
              },
            ])
          }
        >
          Store joints
        </button>
        <button
          disabled={!editable}
          onClick={() =>
            run(`Remove joints from keyframe ${frame}`, [
              { type: "removeKeys", timeline: t.node!, frame, group },
            ])
          }
        >
          Remove joints
        </button>
        <span className="toolbar-separator" />
        {(
          [
            ["Flip", "Flip"],
            ["LeftToRight", "Mirror L→R"],
            ["RightToLeft", "Mirror R→L"],
          ] as const
        ).map(([mode, title]) => (
          <button
            key={mode}
            disabled={!editable || !t.curves.length}
            onClick={() =>
              run(title.replace("→", " to "), chosenKeys.length
                ? selectedFrameRanges(curve?.keys ?? [], chosenKeys).map(frames => ({ type: "mirror", timeline: t.node!, mode, frames }))
                : [{ type: "mirror", timeline: t.node!, mode }])
            }
          >
            {title}
          </button>
        ))}
        <span className="spacer" />
        <button
          disabled={!editable}
          onClick={() =>
            void qianim("import").catch((e) => ws.notify(String(e), "error"))
          }
        >
          Import .qianim
        </button>
        <button
          disabled={!editable}
          onClick={() =>
            void qianim("export").catch((e) => ws.notify(String(e), "error"))
          }
        >
          Export .qianim
        </button>
      </div>
      <div className="panel-toolbar"><button disabled={!t.curves.some(c => c.keys?.length)} onClick={() => setExportCode(true)}>Export motion code…</button></div>
      {exportCode && <MotionCode key={`${session}:${behavior}:${t.node}`} active={active} onClose={() => setExportCode(false)} />}
      <div
        className="dope-sheet panel-scroll"
        aria-label="Motion keys by joint"
      >
        {t.curves.map((c) => (
          <div
            className={`dope-row${c === curve ? " selected" : ""}`}
            key={c.node ?? c.actuator}
          >
            <button
              className="dope-name"
              {...menu.trigger(() => curveActions(c))}
              onClick={() => {setSelected(c.actuator); setChosen([]);}}
            >
              {c.actuator}
            </button>
            <div className="dope-track" tabIndex={0} aria-label={`${c.actuator} motion track`}
              {...menu.trigger(event => {
                const rect = event.currentTarget.getBoundingClientRect();
                const at = "clientX" in event ? Math.max(0, Math.round((event.clientX - rect.left) / rect.width * (last + 1))) : frame;
                return trackActions(c, at);
              })}>
              {!c.keys && c.frames.map((f, i) => <span key={i} className="dope-key" style={{left: `${(f / (last + 1)) * 100}%`}} title={`${c.actuator} · frame ${f}`} />)}
              {(c.keys ?? []).map(k => (
                <button key={k.node} className="dope-key" style={{left: `${(k.frame / (last + 1)) * 100}%`, padding: 0, border: 0}}
                  title={`${c.actuator} · frame ${k.frame}`} aria-label={`${c.actuator} key at frame ${k.frame}`}
                  onClick={() => {setSelected(c.actuator); setChosen([k.node]); ws.saveTimeline(key, {...value, frame: k.frame});}}
                  {...menu.trigger(() => keyActions(c, k))} />
              ))}
              <span
                className="dope-playhead"
                style={{ left: `${(frame / (last + 1)) * 100}%` }}
              />
              {active.robotFrame !== undefined && <span aria-label="Robot dope playhead" data-frame={active.robotFrame} className="dope-playhead robot-playhead" style={{ left: `${(active.robotFrame / (last + 1)) * 100}%` }} />}
            </div>
          </div>
        ))}
        {!t.curves.length && (
          <p className="panel-message">
            No motion yet. Connect a robot and use Store joints, or import a
            .qianim file.
          </p>
        )}
      </div>
      {curve && (
        <div className="curve-editor">
          <div className="panel-toolbar" role="toolbar" aria-label="Curve editing">
            <button onClick={() => setChosen(curve.keys?.map(k => k.node) ?? [])}>Select all keys</button>
            <button disabled={!editable || !chosenKeys.length} onClick={() => void authoring.copy(session!, behavior, chosenKeys).then(setClipboard).catch(e => ws.notify(String(e), "error"))}>Copy keys</button>
            <button disabled={!editable || !clipboard} onClick={() => run("Paste curve keys", [{type: "pasteCurveKeys", timeline: t.node!, xml: clipboard, frame}])}>Paste keys</button>
            <button disabled={!editable || !chosenKeys.length} onClick={() => run("Delete selected keys", [{type: "remove", nodes: chosenKeys}])}>Delete keys</button>
            <label className="checkbox"><input type="checkbox" checked={!!curve.mute} disabled={!editable}
              onChange={e => run("Mute curve", [{type: "setAttributes", node: curve.node!, values: {mute: e.target.checked ? "1" : "0"}}])} />Mute curve</label>
          </div>
          <CurvePlot key={`${behavior}:${t.node}:${curve.node}`} curve={curve} frame={frame} robotFrame={active.robotFrame} last={last} selected={chosenKeys} onSelect={setChosen} onEdit={run} editable={editable}
            onMenu={(event, k, side, at) => menu.open(event, k ? keyActions(curve, k, side) : trackActions(curve, at ?? frame))} />
          <details className="curve-transform"><summary>Transform and simplify</summary><div className="panel-toolbar">
            <label>Scale<input aria-label="Curve value scale" type="number" step="any" value={scale} onChange={e => setScale(Number(e.target.value))} /></label>
            <label>Offset<input aria-label="Curve value offset" type="number" step="any" value={offset} onChange={e => setOffset(Number(e.target.value))} /></label>
            <button disabled={!editable || !chosenKeys.length} onClick={() => run("Transform curve values", [{type: "setCurveValues", keys: chosenKeys, scale, offset}])}>Transform selected keys</button>
            <label>Tolerance<input aria-label="Simplification tolerance" type="number" min={0} step="any" value={tolerance} onChange={e => setTolerance(Number(e.target.value))} /></label>
            <select aria-label="Simplifier" value={simplifier} onChange={e => setSimplifier(e.target.value as SimplifyMode)}>
              <option value="Choren">Choren</option>
              <option value="Choregraphe">Choregraphe-compatible</option>
            </select>
            <button disabled={!editable || !curve.keys?.length} onClick={() => run("Simplify curve", [{type: "simplifyCurves", timeline: t.node!, curves: [curve.node!], from: t.startFrame ?? 1, to: (t.endFrame ?? -1) < 0 ? last : t.endFrame!, tolerance, mode: simplifier}])}>Simplify playback range</button>
          </div></details>
          <table className="key-table" aria-label={`${curve.actuator} keys`}>
            <thead>
              <tr>
                <th>Frame</th>
                <th>Value {curve.unit === 1 ? "(ratio)" : "(°)"}</th>
                <th>Interpolation</th>
                <th>Left tangent</th>
                <th>Right tangent</th>
                <th>Smooth</th>
                <th>Sym.</th>
                <th />
              </tr>
            </thead>
            <tbody>
              {curve.keys?.map((k) => (
                <tr key={k.node} data-curve-key={k.node} tabIndex={0} {...menu.trigger(() => keyActions(curve, k))}>
                  <td>
                    <Num disabled={!editable}
                      label={`Key ${k.frame} frame`}
                      value={k.frame}
                      onCommit={(v) =>
                        run("Move keys", [
                          {
                            type: "moveKeys",
                            keys: [k.node],
                            delta: Math.round(v - k.frame),
                          },
                        ])
                      }
                    />
                  </td>
                  <td>
                    <Num disabled={!editable}
                      label={`Key ${k.frame} value`}
                      value={k.value}
                      onCommit={(v) =>
                        run("Set key value", [
                          {
                            type: "setAttributes",
                            node: k.node,
                            values: { value: String(v) },
                          },
                        ])
                      }
                    />
                  </td>
                  <td>
                    <select
                      aria-label={`Key ${k.frame} interpolation`}
                      value={interpOf(k)}
                      disabled={!editable}
                      onChange={(e) =>
                        run("Change interpolation", [
                          {
                            type: "setInterpolation",
                            keys: [k.node],
                            interp: e.target.value as Interp,
                          },
                        ])
                      }
                    >
                      <option value="Constant">Constant</option>
                      <option value="Linear">Linear</option>
                      <option value="Bezier">Bezier</option>
                      <option value="BezierAuto">Bezier automatic</option>
                    </select>
                  </td>
                  {(["left", "right"] as const).map((side) => {
                    const tg = k[side];
                    return (
                      <td key={side}>
                        {tg?.interp === "bezier" ? (
                          <span className="tangent-fields">
                            <Num disabled={!editable}
                              label={`Key ${k.frame} ${side} tangent frames`}
                              value={tg.abscissa}
                              onCommit={(v) =>
                                run("Move tangent", [
                                  {
                                    type: "moveTangent", tangent: tg.node, abscissa: v, ordinate: tg.ordinate,
                                  },
                                ])
                              }
                            />
                            <Num disabled={!editable}
                              label={`Key ${k.frame} ${side} tangent value`}
                              value={tg.ordinate}
                              onCommit={(v) => {
                                run("Move tangent", [{type: "moveTangent", tangent: tg.node, abscissa: tg.abscissa, ordinate: v}]);
                              }}
                            />
                          </span>
                        ) : (
                          <span className="muted">{tg?.interp ?? "auto"}</span>
                        )}
                      </td>
                    );
                  })}
                  {(["smooth", "symmetrical"] as const).map((flag) => (
                    <td key={flag}>
                      <input
                        type="checkbox"
                        aria-label={`Key ${k.frame} ${flag}`}
                        checked={k[flag]}
                        disabled={!editable}
                        onChange={(e) =>
                          run(
                            flag === "smooth"
                              ? "Change key smoothness"
                              : "Change key symmetrical status",
                            [
                              {
                                type: "setAttributes",
                                node: k.node,
                                values: {
                                  [flag]: e.target.checked ? "1" : null,
                                },
                              },
                            ],
                          )
                        }
                      />
                    </td>
                  ))}
                  <td>
                    <button
                      className="icon-button"
                      aria-label={`Delete key ${k.frame}`}
                      disabled={!editable}
                      onClick={() =>
                        run("Remove keys", [
                          { type: "remove", nodes: [k.node] },
                        ])
                      }
                    >
                      ×
                    </button>
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
          {editable &&
            curve.node !== undefined &&
            !curve.frames.includes(frame) && (
              <button
                onClick={() =>
                  run("Add keys", [
                    {
                      type: "insertXml",
                      parent: curve.node!,
                      xml: `<Key frame="${frame}" value="${curve.values?.findLast((_, i) => curve.frames[i] <= frame) ?? curve.values?.[0] ?? 0}" />`,
                    },
                  ])
                }
              >
                Add key at frame {frame}
              </button>
            )}
        </div>
      )}
    </div>
  );
}

function MotionCode({ active, onClose }: { active: NonNullable<ReturnType<typeof useActiveTimeline>>; onClose: () => void }) {
  const ws = useWorkspace();
  const session = ws.state.project?.session;
  const { t, resolved } = active;
  const [language, setLanguage] = useState<"python" | "cpp">(session ? "python" : "cpp");
  const [bezier, setBezier] = useState(true);
  const [code, setCode] = useState<string>();
  const [error, setError] = useState("");
  useEffect(() => {
    let live = true;
    setCode(undefined); setError("");
    const result = language === "python"
      ? authoring.exportMotionPython(session!, resolved.behavior.id, t.node!, bezier)
      : Promise.resolve(motionCode(t, bezier));
    void result.then(text => { if (live) setCode(text); }, e => { if (live) setError(String(e)); });
    return () => { live = false; };
  }, [session, resolved.behavior.id, t, language, bezier]);
  return <Modal title="Export motion code" wide onClose={onClose}>
    <div className="settings-content">
      <select aria-label="Motion code language" value={language} onChange={e => setLanguage(e.target.value as "python" | "cpp")}>
        <option value="python" disabled={!session || t.node === undefined}>Python</option><option value="cpp">C++</option>
      </select>
      <label className="checkbox"><input type="checkbox" checked={bezier} onChange={e => setBezier(e.target.checked)} />Bezier interpolation</label>
      {error && <p className="error" role="alert">{error}</p>}
      <textarea className="motion-code" aria-label="Exported motion code" readOnly value={code ?? ""} placeholder={error ? "Export unavailable" : "Generating motion code…"} />
      <button disabled={code === undefined} onClick={() => void navigator.clipboard.writeText(code!).then(() => ws.notify("Motion code copied.")).catch(e => ws.notify(String(e), "error"))}>Copy code</button>
      <button disabled={code === undefined} onClick={() => void (async () => {
        const path = await save({ defaultPath: `motion.${language === "python" ? "py" : "cpp"}` });
        if (path) await writeTextFile(path, code!);
      })().catch(e => ws.notify(String(e), "error"))}>Save code…</button>
    </div>
  </Modal>;
}
