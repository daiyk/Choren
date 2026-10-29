import { useEffect, useState } from "react";
import { Code2, FolderOpen, Info, Plus, SlidersHorizontal, X } from "lucide-react";
import { PROJECT_FILE_MIME } from "./platform";
import type { EditCommand } from "./workspace/authoring";
import { scriptOpenable } from "./workspace/model";
import { categories, CategoryBadge, isBoxCategory } from "./diagram/appearance";
import type { Box, Param, ProjectDocument, ProjectFile } from "./format/types";
import { behaviorRelative } from "./format/xml";
import { useContextMenu, type MenuAction } from "./workspace/ContextMenu";
import { speechLanguageName, speechLanguages } from "./workspace/languages";
import {
  CONTENT_TYPE,
  INPUT_NATURE,
  IO_TYPE,
  OUTPUT_NATURE,
} from "./format/types";

/** Text field that commits on Enter or blur; Escape restores the stored value. */
function Field({
  label,
  value,
  multiline,
  list,
  placeholder,
  onCommit,
}: {
  label: string;
  value: string;
  multiline?: boolean;
  list?: string;
  placeholder?: string;
  onCommit: (value: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    if (draft !== value) onCommit(draft);
  };
  const props = {
    "aria-label": label,
    placeholder,
    value: draft,
    onChange: (e: { target: { value: string } }) => setDraft(e.target.value),
    onBlur: commit,
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === "Escape") setDraft(value);
      if (e.key === "Enter" && !(multiline && e.shiftKey)) {
        e.preventDefault();
        commit();
      }
    },
  };
  return multiline ? (
    <textarea className="inspector-field" rows={3} {...props} />
  ) : (
    <input className="inspector-field" list={list} {...props} />
  );
}

/** A labelled line in an Inspector form grid. */
function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="form-row">
      <span>{label}</span>
      {children}
    </label>
  );
}

/** Parameter dialog fields: Int/Double range, String choices and "custom value allowed". */
function ParamSettings({
  p,
  onEdit,
}: {
  p: Param;
  onEdit: (label: string, commands: EditCommand[]) => void;
}) {
  const label = `Edit parameter: ${p.name}`;
  const set = (values: Record<string, string | null>) =>
    onEdit(label, [{ type: "setAttributes", node: p.node!, values }]);
  if (p.contentType === 1 || p.contentType === 2)
    return (
      <Row label="Range">
        <span className="param-range">
          <Field
            label={`${p.name} minimum`}
            value={p.min ?? ""}
            onCommit={(v) => set({ min: v || null })}
          />
          <span className="muted">to</span>
          <Field
            label={`${p.name} maximum`}
            value={p.max ?? ""}
            onCommit={(v) => set({ max: v || null })}
          />
        </span>
      </Row>
    );
  if (p.contentType !== 3) return null;
  return (
    <>
      <Row label="Choices">
        <span className="param-choices">
          {p.choices?.map((c) => (
            <span className="choice" key={c.node}>
              {c.value}
              <button
                className="icon-button"
                aria-label={`Delete choice ${c.value}`}
                onClick={(e) => {
                  e.preventDefault();
                  onEdit(label, [{ type: "remove", nodes: [c.node] }]);
                }}
              >
                <X size={10} />
              </button>
            </span>
          ))}
          <Field
            label={`${p.name} new choice`}
            value=""
            onCommit={(v) =>
              v &&
              onEdit(label, [
                {
                  type: "insertXml",
                  parent: p.node!,
                  xml: `<Choice value="${escapeAttr(v)}" />`,
                },
              ])
            }
          />
        </span>
      </Row>
      <label className="checkbox form-check">
        <input
          type="checkbox"
          aria-label={`${p.name} custom value allowed`}
          checked={!!p.customChoice}
          onChange={(e) => set({ custom_choice: e.target.checked ? "1" : "0" })}
        />
        Custom value allowed
      </label>
    </>
  );
}

/** A Resource parameter: type a path, pick a project file, browse (imports it) or drop a file from the Project panel. */
function ResourceField({
  p,
  files,
  behavior,
  onCommit,
  onImport,
}: {
  p: Param;
  files: ProjectFile[];
  behavior?: string;
  onCommit: (value: string) => void;
  onImport?: () => Promise<string | undefined>;
}) {
  const [over, setOver] = useState(false);
  const [error, setError] = useState("");
  // Resource values point from the behavior's folder, prefixed with "/" (e.g. `/../sounds/a.ogg`).
  const toValue = (path: string) => `/${behaviorRelative(behavior ?? "", path)}`;
  const current = files.find((f) => toValue(f.path) === p.value)?.path ?? "";
  return (
    <div
      className={`resource-field${over ? " drop-target" : ""}`}
      onDragOver={(e) => {
        if (!e.dataTransfer.types.includes(PROJECT_FILE_MIME)) return;
        e.preventDefault();
        e.dataTransfer.dropEffect = "link";
        setOver(true);
      }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => {
        setOver(false);
        const path = e.dataTransfer.getData(PROJECT_FILE_MIME);
        if (!path) return;
        e.preventDefault();
        onCommit(toValue(path));
      }}
    >
      <Field label={`${p.name} value`} value={p.value} placeholder="File path, or drop a file" onCommit={onCommit} />
      <span className="resource-actions">
        <select
          aria-label={`${p.name} project file`}
          value={current}
          onChange={(e) => e.target.value && onCommit(toValue(e.target.value))}
        >
          <option value="">{files.length ? "Project file…" : "No project files"}</option>
          {files.map((f) => (
            <option key={f.path} value={f.path}>
              {f.path}
            </option>
          ))}
        </select>
        {onImport && (
          <button
            title="Choose a file; files outside the project are imported next to this behavior"
            onClick={async () => {
              setError("");
              try {
                const path = await onImport();
                if (path) onCommit(toValue(path));
              } catch (e) {
                setError(String(e));
              }
            }}
          >
            <FolderOpen size={13} />
            Browse…
          </button>
        )}
      </span>
      {error && <small className="error" role="alert">{error}</small>}
    </div>
  );
}

/** The value control for one parameter, matched to its content type. */
function ParamValue({
  p,
  editable,
  files,
  behavior,
  onCommit,
  onImport,
}: {
  p: Param;
  editable: boolean;
  files: ProjectFile[];
  behavior?: string;
  onCommit: (value: string) => void;
  onImport?: () => Promise<string | undefined>;
}) {
  if (!editable)
    return <span className="parameter-readonly">{p.value || <span className="muted">Empty</span>}</span>;
  if (p.contentType === 0)
    return (
      <label className="checkbox">
        <input
          type="checkbox"
          aria-label={`${p.name} value`}
          checked={p.value === "1" || p.value === "true"}
          onChange={(e) => onCommit(e.target.checked ? "1" : "0")}
        />
        Enabled
      </label>
    );
  if (p.contentType === 4)
    return <ResourceField p={p} files={files} behavior={behavior} onCommit={onCommit} onImport={onImport} />;
  if (p.contentType === 3 && p.choices?.length && !p.customChoice)
    return (
      <select aria-label={`${p.name} value`} value={p.value} onChange={(e) => onCommit(e.target.value)}>
        {!p.choices.some((c) => c.value === p.value) && <option value={p.value}>{p.value || "—"}</option>}
        {p.choices.map((c) => (
          <option key={c.node} value={c.value}>
            {c.value}
          </option>
        ))}
      </select>
    );
  return (
    <span className="parameter-input">
      <Field label={`${p.name} value`} value={p.value} list={p.choices?.length ? `choices-${p.id}` : undefined} onCommit={onCommit} />
      {p.choices?.length ? (
        <datalist id={`choices-${p.id}`}>
          {p.choices.map((c) => <option key={c.node} value={c.value} />)}
        </datalist>
      ) : null}
      {(p.min || p.max) && (
        <small className="muted">
          {p.min || "…"} – {p.max || "…"}
        </small>
      )}
    </span>
  );
}

const escapeAttr = (v: string) =>
  v
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");

/** Common resource names for box resource declarations. */
const RESOURCES = [
  "All motors",
  "Speech",
  "Standing",
  "Stiffness",
  "Audio player",
  "Left eye leds",
  "Right eye leds",
  "Head",
  "Left arm",
  "Right arm",
];

export function Inspector({
  boxes,
  edges = [],
  project,
  behavior,
  file,
  onScript,
  onEdit,
  onParameterEdit,
  onImportResource,
}: {
  /** Picks a file for a Resource parameter (importing it if needed); resolves to its project path. */
  onImportResource?: () => Promise<string | undefined>;
  onParameterEdit?: (parameter: Param, value: string) => void;
  onEdit?: (label: string, commands: EditCommand[]) => void;
  boxes: Box[];
  edges?: string[];
  project?: ProjectDocument;
  /** The edited behavior's file, which a Dialog box's `dialogFile` is relative to. */
  behavior?: string;
  file?: ProjectFile;
  onScript: (box: Box) => void;
}) {
  const [menuError, setMenuError] = useState("");
  const menu = useContextMenu([project, boxes[0], boxes.map(b => b.node ?? b.id).join(","), edges.join(","), !!onEdit], setMenuError);
  const showProperties = (element: Element) => {
    const details = element.querySelector("details");
    if (details) details.open = true;
    ((details ?? element).querySelector<HTMLElement>("input,select,textarea") ?? details?.querySelector<HTMLElement>("summary"))?.focus();
  };
  if (file)
    return (
      <div className="inspector panel-scroll">
        <div className="inspector-heading">
          <Info size={18} />
          <h2>Project file</h2>
        </div>
        <dl>
          <dt>Path</dt>
          <dd>{file.path}</dd>
          <dt>Kind</dt>
          <dd>{file.kind}</dd>
          <dt>Status</dt>
          <dd>
            {file.missing
              ? "Missing or inaccessible"
              : "Referenced by this project"}
          </dd>
        </dl>
        <p className="muted">
          This file is listed for inspection. Editing and resource previews will
          be added in a later phase.
        </p>
      </div>
    );
  if (boxes.length + edges.length > 1)
    return (
      <div className="inspector panel-scroll">
        <h2>{boxes.length + edges.length} items selected</h2>
        <p className="muted">
          {boxes.length} boxes · {edges.length} connections
        </p>
        {boxes.map((b) => (
          <div className="selection-item" key={b.id}>
            {b.name}
          </div>
        ))}
        <p className="muted">Select one box to inspect its properties.</p>
      </div>
    );
  if (edges.length)
    return (
      <div className="inspector panel-scroll">
        <h2>Connection</h2>
        {edges.map((label) => (
          <p key={label}>{label}</p>
        ))}
        <p className="muted">
          {onEdit
            ? "Press Delete in the diagram to remove a connection."
            : "Connections are read-only."}
        </p>
      </div>
    );
  const box = boxes[0];
  const set = (
    node: number | undefined,
    label: string,
    key: string,
    value: string,
  ) =>
    node !== undefined &&
    onEdit?.(label, [
      { type: "setAttributes", node, values: { [key]: value } },
    ]);
  const setParameter = (p: Param, value: string) => {
    if (onParameterEdit) onParameterEdit(p, value);
    else set(p.node, `Set parameter: ${p.name}`, "value", value);
  };
  const editBox = onEdit && box?.node !== undefined && box.id !== -1;
  const remove = (node: number, label: string) =>
    onEdit?.(label, [{ type: "remove", nodes: [node] }]);
  const addPort = (
    kind: "Input" | "Output" | "Parameter",
    base: string,
    values: Record<string, string>,
  ) => {
    const taken = new Set(
      [...box.inputs, ...box.outputs, ...box.params].map((p) => p.name),
    );
    let name = base;
    for (let i = 2; taken.has(name); i++) name = `${base}${i}`;
    // An ALMemory event input is named after its key.
    if (values.nature === "4") values = { ...values, stm_value_name: name };
    onEdit?.(`Add ${kind.toLowerCase()}: ${name}`, [
      { type: "addPort", box: box.node!, kind, name, values },
    ]);
  };
  const reorder = (nodes: {node?: number}[], node: number, delta: number) => {
    const ports = nodes.map(p => p.node!);
    const at = ports.indexOf(node);
    if (at + delta < 0 || at + delta >= ports.length) return;
    [ports[at], ports[at + delta]] = [ports[at + delta], ports[at]];
    onEdit?.("Reorder ports", [{type: "sortPorts", box: box.node!, ports}]);
  };
  const setMany = (
    node: number | undefined,
    label: string,
    values: Record<string, string | null>,
  ) =>
    node !== undefined &&
    onEdit?.(label, [{ type: "setAttributes", node, values }]);
  if (!box)
    return (
      <div className="inspector panel-scroll">
        <div className="inspector-heading">
          <SlidersHorizontal size={18} />
          <h2>{project ? "Project overview" : "Inspector"}</h2>
        </div>
        <p className="muted">
          Select a box or connection to inspect its details.
        </p>
        {project && (
          <>
            <h3>{project.name}</h3>
            <dl>
              <dt>Behaviors</dt>
              <dd>{project.behaviors.length}</dd>
              <dt>Files</dt>
              <dd>{project.files.length}</dd>
              <dt>Source</dt>
              <dd>{project.source}</dd>
              {Object.entries(project.metadata).map(([name, value]) => (
                <div className="property-pair" key={name}>
                  <dt>{name}</dt>
                  <dd>{value}</dd>
                </div>
              ))}
            </dl>
            {!onEdit && <span className="badge">Read-only project</span>}
          </>
        )}
      </div>
    );
  const editable = (p: { node?: number }) => !!editBox && p.node !== undefined;
  const resourceFiles = (project?.files ?? []).filter((f) => f.kind === "resource");
  const portRow = (p: Box["inputs"][number], direction: "Input" | "Output") => {
    const natures = direction === "Input" ? INPUT_NATURE : OUTPUT_NATURE;
    const ports = direction === "Input" ? box.inputs : box.outputs;
    const label = `Edit ${direction.toLowerCase()}: ${p.name}`;
    const index = ports.findIndex(x => x.node === p.node);
    const portActions = (element: Element): MenuAction[] => [
      {label: "Port properties", run: () => showProperties(element), disabled: !editable(p), reason: "This port is read-only."},
      ...[-1, 1].map(delta => ({label: delta < 0 ? "Move up" : "Move down", run: () => reorder(ports, p.node!, delta), disabled: !editable(p) || index + delta < 0 || index + delta >= ports.length, reason: !editable(p) ? "This port is read-only." : "Already at the end of the list."})),
      {label: `Delete ${direction.toLowerCase()}`, run: () => remove(p.node!, `Remove ${direction.toLowerCase()}: ${p.name}`), disabled: !editable(p), reason: "This port is read-only."},
    ];
    if (!editable(p))
      return (
        <div className="port-row" key={`${direction}${p.id}`} title={p.tooltip} tabIndex={0} {...menu.trigger(event => portActions(event.currentTarget))}>
          <span className="port-indicator" data-type={p.type} />
          <strong>{p.name}</strong>
          <small>
            {IO_TYPE[p.type] ?? "Unknown"} · {natures[p.nature] ?? "Unknown"}
            {p.stm && ` · ${p.stm}`}
          </small>
        </div>
      );
    return (
      <div className="port-row" key={`${direction}${p.id}`} title={p.tooltip} tabIndex={0} {...menu.trigger(event => portActions(event.currentTarget))}>
        <span className="port-edit">
          <span className="port-indicator" data-type={p.type} />
          <Field
            label={`${direction} ${p.name} name`}
            value={p.name}
            onCommit={(v) => set(p.node, label, "name", v)}
          />
          <span className="port-kind">
          <select
            aria-label={`${p.name} type`}
            value={p.type}
            onChange={(e) => set(p.node, label, "type", e.target.value)}
          >
            {IO_TYPE.map((t, i) => (
              <option key={t} value={i}>
                {t}
              </option>
            ))}
          </select>
          <select
            aria-label={`${p.name} nature`}
            value={p.nature}
            onChange={(e) =>
              setMany(
                p.node,
                label,
                e.target.value === "4"
                  ? { nature: "4", stm_value_name: p.stm || p.name }
                  : { nature: e.target.value, stm_value_name: null },
              )
            }
          >
            {natures.map((t, i) => (
              <option key={t} value={i}>
                {t}
              </option>
            ))}
          </select>
          </span>
          <button
            className="icon-button"
            aria-label={`Delete ${p.name}`}
            title={`Delete ${direction.toLowerCase()} ${p.name}`}
            onClick={() => remove(p.node!, `Remove ${direction.toLowerCase()}: ${p.name}`)}
          >
            <X size={12} />
          </button>
        </span>
        {p.nature === 4 && (
          <Row label="Memory key">
            <Field
              label={`${p.name} memory key`}
              value={p.stm ?? ""}
              onCommit={(v) => set(p.node, `Edit input from ALMemory`, "stm_value_name", v)}
            />
          </Row>
        )}
        <details className="more">
          <summary>Port properties</summary>
          <div className="form-grid">
            <Row label="Description">
              <Field
                label={`${p.name} description`}
                value={p.tooltip}
                multiline
                onCommit={(v) => set(p.node, "Edit port description", "tooltip", v)}
              />
            </Row>
            <Row label="Value count">
              <Field
                label={`${p.name} value count`}
                value={String(p.typeSize ?? 1)}
                onCommit={(v) => set(p.node, "Edit port value count", "type_size", v)}
              />
            </Row>
            <label className="checkbox form-check">
              <input
                type="checkbox"
                aria-label={`${p.name} inner`}
                checked={p.inner}
                onChange={(e) => set(p.node, "Edit inner port", "inner", e.target.checked ? "1" : "0")}
              />
              Inner port
            </label>
            <span className="form-actions">
              {[-1, 1].map((delta) => (
                <button
                  key={delta}
                  aria-label={`Move ${p.name} ${delta < 0 ? "up" : "down"}`}
                  disabled={index + delta < 0 || index + delta >= ports.length}
                  onClick={() => reorder(ports, p.node!, delta)}
                >
                  {delta < 0 ? "Move up" : "Move down"}
                </button>
              ))}
            </span>
          </div>
        </details>
      </div>
    );
  };
  return (
    <div className="inspector panel-scroll box-inspector">
      {menu.element}
      {menuError && <p role="alert" className="error">{menuError}</p>}
      <div className="inspector-heading">
        <SlidersHorizontal size={18} />
        <div className="inspector-title">
          {editBox ? (
            <Field
              label="Box name"
              value={box.name}
              onCommit={(v) => set(box.node, "Edit box name", "name", v)}
            />
          ) : (
            <h2>{box.name}</h2>
          )}
          <CategoryBadge box={box} />
        </div>
      </div>
      {box.tooltip ? (
        <p className="description">{box.tooltip}</p>
      ) : (
        !editBox && <p className="muted description-empty">No description.</p>
      )}
      {scriptOpenable(box) && (
        <button className="wide-button" onClick={() => onScript(box)}>
          <Code2 size={15} />
          Open script{!onEdit && <span className="badge">Read-only</span>}
        </button>
      )}
      {box.plugin === "textedit_plugin" && (
        <details open>
          <summary>Comment text</summary>
          {editBox ? (
            <Field
              label="Comment text"
              value={box.pluginText ?? ""}
              multiline
              onCommit={(pluginText) => onEdit!("Edit comment", [{ type: "setBoxContent", box: box.node!, pluginText }])}
            />
          ) : (
            <p className="comment-content">{box.pluginText || "Empty comment"}</p>
          )}
        </details>
      )}
      {box.plugin === "setlanguage_plugin" && (
        <details open>
          <summary>Language</summary>
          <div className="form-grid">
            <Row label="Robot language">
              {editBox ? (
                <select
                  aria-label="Set Language language"
                  value={box.pluginLanguage ?? ""}
                  onChange={(e) => onEdit!("Change language", [{ type: "setBoxContent", box: box.node!, pluginLanguage: e.target.value }])}
                >
                  {!box.pluginLanguage && <option value="" disabled>Choose a language…</option>}
                  {box.pluginLanguage && !speechLanguages.includes(box.pluginLanguage) && <option value={box.pluginLanguage}>{box.pluginLanguage}</option>}
                  {speechLanguages.map((name) => <option key={name} value={name}>{speechLanguageName(name)}</option>)}
                </select>
              ) : (
                <span>{box.pluginLanguage ? speechLanguageName(box.pluginLanguage) : "Not set"}</span>
              )}
            </Row>
          </div>
          <p className="muted">{box.pluginLanguage ? "Speech, recognition and dialog switch to this language when onSet fires." : "Choose a language: until then the box has no script and does nothing on the robot."}</p>
        </details>
      )}
      <details open>
        <summary {...menu.trigger(() => [{label: "Add parameter", run: () => addPort("Parameter", "parameter", {content_type: "3"}), disabled: !editBox, reason: "This box is read-only."}])}>
          Parameters <span>{box.params.length}</span>
        </summary>
        {box.params.map((p) => (
          <div className="parameter" key={p.id} tabIndex={0} {...menu.trigger(event => { const row = event.currentTarget; return [
            {label: "Parameter properties", run: () => showProperties(row), disabled: !editable(p), reason: "This parameter is read-only."},
            ...[-1, 1].map(delta => ({label: delta < 0 ? "Move up" : "Move down", run: () => reorder(box.params, p.node!, delta), disabled: !editable(p) || box.params.indexOf(p) + delta < 0 || box.params.indexOf(p) + delta >= box.params.length, reason: !editable(p) ? "This parameter is read-only." : "Already at the end of the list."})),
            {label: "Delete parameter", run: () => remove(p.node!, `Remove parameter: ${p.name}`), disabled: !editable(p), reason: "This parameter is read-only."},
          ]; })}>
            <div className="parameter-head">
              <strong>{p.name}</strong>
              <span className="type-chip">{CONTENT_TYPE[p.contentType] ?? "Unknown"}</span>
              {editable(p) && (
                <button
                  className="icon-button"
                  aria-label={`Delete parameter ${p.name}`}
                  title={`Delete parameter ${p.name}`}
                  onClick={() => remove(p.node!, `Remove parameter: ${p.name}`)}
                >
                  <X size={12} />
                </button>
              )}
            </div>
            <ParamValue
              p={p}
              editable={!!onEdit && p.node !== undefined}
              files={resourceFiles}
              behavior={behavior}
              onCommit={(v) => setParameter(p, v)}
              onImport={onImportResource}
            />
            {p.tooltip && <p className="parameter-help">{p.tooltip}</p>}
            {editable(p) && (
              <details className="more">
                <summary>Parameter properties</summary>
                <div className="form-grid">
                  <Row label="Name">
                    <Field label={`${p.name} parameter name`} value={p.name} onCommit={(v) => set(p.node, "Rename parameter", "name", v)} />
                  </Row>
                  <Row label="Type">
                    <select
                      aria-label={`${p.name} parameter type`}
                      value={p.contentType}
                      onChange={(e) => set(p.node, "Change parameter type", "content_type", e.target.value)}
                    >
                      {CONTENT_TYPE.map((name, i) => (
                        <option key={name} value={i}>
                          {name}
                        </option>
                      ))}
                    </select>
                  </Row>
                  <Row label="Default">
                    <Field label={`${p.name} default`} value={p.defaultValue ?? ""} onCommit={(v) => set(p.node, "Edit parameter default", "default_value", v)} />
                  </Row>
                  <Row label="Description">
                    <Field
                      label={`${p.name} parameter description`}
                      value={p.tooltip}
                      multiline
                      onCommit={(v) => set(p.node, "Edit parameter description", "tooltip", v)}
                    />
                  </Row>
                  <ParamSettings p={p} onEdit={(label, commands) => onEdit!(label, commands)} />
                  <label className="checkbox form-check">
                    <input
                      type="checkbox"
                      aria-label={`${p.name} inherit from parent`}
                      checked={!!p.inheritsFromParent}
                      onChange={(e) => set(p.node, "Edit parameter inheritance", "inherits_from_parent", e.target.checked ? "1" : "0")}
                    />
                    Inherit from parent
                  </label>
                  <span className="form-actions">
                    {[-1, 1].map((delta) => (
                      <button
                        key={delta}
                        disabled={box.params.indexOf(p) + delta < 0 || box.params.indexOf(p) + delta >= box.params.length}
                        onClick={() => reorder(box.params, p.node!, delta)}
                      >
                        {delta < 0 ? "Move up" : "Move down"}
                      </button>
                    ))}
                  </span>
                </div>
              </details>
            )}
          </div>
        ))}
        {!box.params.length && <p className="muted">No parameters.</p>}
        {editBox && (
          <div className="add-row">
            <button onClick={() => addPort("Parameter", "parameter", { content_type: "3" })}>
              <Plus size={12} />
              Parameter
            </button>
          </div>
        )}
      </details>
      <details open>
        <summary {...menu.trigger(() => [
          {label: "Add input", run: () => addPort("Input", "input", {nature: "1"}), disabled: !editBox, reason: "This box is read-only."},
          {label: "Add event", run: () => addPort("Input", "event", {nature: "4", type: "0", inner: "1"}), disabled: !editBox, reason: "This box is read-only."},
          {label: "Add output", run: () => addPort("Output", "output", {nature: "2"}), disabled: !editBox, reason: "This box is read-only."},
        ])}>
          Ports <span>{box.inputs.length + box.outputs.length}</span>
        </summary>
        {box.inputs.length > 0 && <div className="inspector-group">Inputs</div>}
        {box.inputs.map((p) => portRow(p, "Input"))}
        {box.outputs.length > 0 && <div className="inspector-group">Outputs</div>}
        {box.outputs.map((p) => portRow(p, "Output"))}
        {!box.inputs.length && !box.outputs.length && <p className="muted">No ports.</p>}
        {editBox && (
          <div className="add-row">
            <button onClick={() => addPort("Input", "input", { nature: "1" })}>
              <Plus size={12} />
              Input
            </button>
            <button
              title="Add event from ALMemory"
              onClick={() => addPort("Input", "event", { nature: "4", type: "0", inner: "1" })}
            >
              <Plus size={12} />
              Event
            </button>
            <button onClick={() => addPort("Output", "output", { nature: "2" })}>
              <Plus size={12} />
              Output
            </button>
          </div>
        )}
      </details>
      <details open={box.resources.length > 0}>
        <summary {...menu.trigger(event => {
          const section = event.currentTarget.parentElement!;
          return [{label: "Add resource…", run: () => { (section as HTMLDetailsElement).open = true; section.querySelector<HTMLElement>('[aria-label="Add resource"]')?.focus(); }, disabled: !editBox, reason: "This box is read-only."}];
        })}>
          Resources <span>{box.resources.length}</span>
        </summary>
        {box.resources.map((r, i) => (
          <div className="resource-row" key={i} tabIndex={0} {...menu.trigger(event => {
            const row = event.currentTarget;
            return [
              {label: "Edit requirement", run: () => showProperties(row), disabled: !editable(r), reason: "This requirement is read-only."},
              {label: "Delete requirement", run: () => remove(r.node!, `Edit resource for box ${box.name}`), disabled: !editable(r), reason: "This requirement is read-only."},
            ];
          })}>
            <strong>{r.name}</strong>
            {editable(r) ? (
              <>
                <select aria-label={`${r.name} policy`} value={r.type} onChange={(e) => set(r.node, "Edit resource policy", "type", e.target.value)}>
                  {["Lock", "Wait", "Stop on demand", "Callback on demand"].map((v) => (
                    <option key={v}>{v}</option>
                  ))}
                </select>
                <label className="resource-timeout">
                  <Field label={`${r.name} timeout`} value={r.timeout || "0"} onCommit={(v) => set(r.node, "Edit resource timeout", "timeout", v)} />
                  <span className="muted">s</span>
                </label>
                <button
                  className="icon-button"
                  aria-label={`Delete resource ${r.name}`}
                  onClick={() => remove(r.node!, `Edit resource for box ${box.name}`)}
                >
                  <X size={12} />
                </button>
              </>
            ) : (
              <small>
                {r.type} · Timeout {r.timeout || "0"} s
              </small>
            )}
          </div>
        ))}
        {!box.resources.length && <p className="muted">No resource requirements.</p>}
        {editBox && (
          <div className="add-row">
            <select
              aria-label="Add resource"
              value=""
              onChange={(e) =>
                e.target.value &&
                onEdit!(`Edit resource for box ${box.name}`, [
                  {
                    type: "insertXml",
                    parent: box.node!,
                    xml: `<Resource name="${e.target.value}" type="Lock" timeout="0" />`,
                  },
                ])
              }
            >
              <option value="">Add resource…</option>
              {RESOURCES.filter((r) => !box.resources.some((x) => x.name === r)).map((r) => (
                <option key={r}>{r}</option>
              ))}
            </select>
          </div>
        )}
      </details>
      {(editBox || box.plugin) && (
        <details open={!!editBox}>
          <summary>Properties</summary>
          <div className="form-grid">
            {editBox && (
              <>
                <Row label="Description">
                  <Field label="Box description" value={box.tooltip} multiline onCommit={(v) => set(box.node, "Edit box description", "tooltip", v)} />
                </Row>
                <Row label="Appearance">
                  <select
                    aria-label="Appearance category"
                    value={isBoxCategory(box.category) ? box.category : ""}
                    onChange={(event) => setMany(box.node, "Change box category", { choren_category: event.target.value || null })}
                  >
                    <option value="">Automatic</option>
                    {Object.entries(categories).map(([value, category]) => (
                      <option key={value} value={value}>
                        {category.label}
                      </option>
                    ))}
                  </select>
                </Row>
                <Row label="Bitmaps">
                  <Field
                    label="Box bitmap paths"
                    value={box.bitmaps.join("\n")}
                    multiline
                    onCommit={(text) =>
                      onEdit!("Edit box bitmaps", [
                        { type: "setBoxContent", box: box.node!, bitmaps: text.split("\n").map((v) => v.trim()).filter(Boolean) },
                      ])
                    }
                  />
                </Row>
                {box.dialogFile !== undefined && (
                  <Row label="Dialog file">
                    <select
                      aria-label="Box dialog file"
                      value={box.dialogFile}
                      onChange={(e) => onEdit!("Change dialog file", [{ type: "setDialogFile", box: box.node!, file: e.target.value }])}
                    >
                      {[
                        ...new Set([
                          box.dialogFile,
                          ...(project?.files ?? []).filter((f) => f.kind === "dialog").map((f) => behaviorRelative(behavior ?? "", f.path)),
                        ]),
                      ].map((f) => (
                        <option key={f}>{f}</option>
                      ))}
                    </select>
                  </Row>
                )}
              </>
            )}
            {box.plugin && (
              <div className="form-row">
                <span>Plugin</span>
                <span>{box.plugin}</span>
              </div>
            )}
          </div>
        </details>
      )}
    </div>
  );
}
