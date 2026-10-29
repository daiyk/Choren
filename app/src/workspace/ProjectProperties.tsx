import { useEffect, useRef, useState } from "react";
import { Plus, X } from "lucide-react";
import type { BehaviorManifest, Manifest, Requirements } from "./authoring";
import { Modal } from "./Modal";
import { languageCodes, languageName } from "./languages";

type Rows = [string, string][];

/** A labelled line in the properties grid. */
function Field({ label, hint, children }: { label: string; hint?: string; children: React.ReactNode }) {
  return <label className="pp-field">
    <span>{label}{hint && <small>{hint}</small>}</span>
    {children}
  </label>;
}

/** "+ Add language" and a menu of the robot languages not yet chosen. */
function AddLanguage({ taken, onPick }: { taken: string[]; onPick: (code: string) => void }) {
  const [open, setOpen] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (!open) return;
    root.current?.querySelector<HTMLElement>("[role=menuitem]")?.focus();
    const outside = (event: Event) => { if (!root.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", outside, true);
    return () => document.removeEventListener("pointerdown", outside, true);
  }, [open]);
  const left = languageCodes.filter(code => !taken.includes(code));
  const close = () => { setOpen(false); root.current?.querySelector<HTMLElement>(".pp-add")?.focus(); };
  return <div className="pp-language-add" ref={root}>
    <button type="button" className="pp-add" aria-haspopup="menu" aria-expanded={open} disabled={!left.length} onClick={() => setOpen(!open)}><Plus size={12} />Add language</button>
    {open && <div className="pp-language-menu" role="menu" aria-label="Robot languages" onKeyDown={event => {
      const items = [...event.currentTarget.querySelectorAll<HTMLElement>("[role=menuitem]")];
      const at = items.indexOf(document.activeElement as HTMLElement);
      if (event.key === "ArrowDown" || event.key === "ArrowUp") { event.preventDefault(); items[(at + (event.key === "ArrowDown" ? 1 : -1) + items.length) % items.length]?.focus(); }
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); close(); }
    }}>
      {left.map(code => <button type="button" role="menuitem" key={code} onClick={() => { onPick(code); close(); }}>
        {languageName(code)}<small>{code}</small>
      </button>)}
    </div>}
  </div>;
}

/** The manifest's supported languages as removable chips. */
function SupportedLanguages({ value, onChange }: { value: string[]; onChange: (value: string[]) => void }) {
  return <div className="pp-languages" role="group" aria-label="Supported languages">
    {value.map(code => <span className="pp-language" key={code} title={code}>
      {languageName(code)}<small>{code}</small>
      <button type="button" className="icon-button" aria-label={`Remove ${languageName(code)}`} title="Remove" onClick={() => onChange(value.filter(c => c !== code))}><X size={12} /></button>
    </span>)}
    {!value.length && <span className="pp-empty">None yet.</span>}
    <AddLanguage taken={value} onPick={code => onChange([...value, code])} />
  </div>;
}

/** A language column cell: project languages first, then the other robot languages. */
function LanguageSelect({ label, value, preferred, onChange }: { label: string; value: string; preferred: string[]; onChange: (code: string) => void }) {
  const others = languageCodes.filter(code => !preferred.includes(code));
  const unknown = value && !preferred.includes(value) && !others.includes(value);
  return <select aria-label={label} required value={value} onChange={e => onChange(e.target.value)}>
    <option value="" disabled>Choose…</option>
    {unknown && <option value={value}>{value}</option>}
    {preferred.length > 0 && <optgroup label="Project languages">{preferred.map(code => <option key={code} value={code}>{languageName(code)}</option>)}</optgroup>}
    <optgroup label={preferred.length ? "Other languages" : "Languages"}>{others.map(code => <option key={code} value={code}>{languageName(code)}</option>)}</optgroup>
  </select>;
}

/** Per-language texts (or launch conditions) as an editable table. */
function LocalizedRows({ label, rows, onChange, conditions = false, languages = [] }: {
  label: string; rows: Rows; onChange: (rows: Rows) => void; conditions?: boolean; languages?: string[];
}) {
  const key = conditions ? "purpose" : "language";
  // A new row takes the first project language without a text yet.
  const next = conditions ? "" : languages.find(code => !rows.some(([lang]) => lang === code)) ?? "";
  return <div className="pp-list" role="group" aria-label={label}>
    <div className="pp-list-head">
      <span>{label}</span>
      <button type="button" className="pp-add" onClick={() => onChange([...rows, [next, ""]])}><Plus size={12} />Add {label.toLowerCase()}</button>
    </div>
    {rows.length > 0 && <div className="pp-table">
      <span className="pp-col">{conditions ? "Purpose" : "Language"}</span>
      <span className="pp-col">{conditions ? "Expression" : "Text"}</span>
      <span />
      {rows.map(([lang, text], i) => {
        const invalid = conditions && text.includes("]]>");
        return <div className="pp-table-row" key={i}>
          {conditions
            ? <input aria-label={`${label} ${key} ${i + 1}`} required value={lang} placeholder="launchTrigger"
              onChange={e => onChange(rows.map((r, n) => n === i ? [e.target.value, text] : r))} />
            : <LanguageSelect label={`${label} ${key} ${i + 1}`} value={lang} preferred={languages}
              onChange={code => onChange(rows.map((r, n) => n === i ? [code, text] : r))} />}
          <span className="pp-text">
            <textarea aria-label={`${label} text ${i + 1}`} rows={conditions ? 2 : 1} value={text} aria-invalid={invalid}
              ref={el => el?.setCustomValidity(invalid ? "Conditions cannot contain ]]>" : "")}
              onChange={e => onChange(rows.map((r, n) => n === i ? [lang, e.target.value] : r))} />
            {invalid && <span className="error">Conditions cannot contain ]]&gt;</span>}
          </span>
          <button type="button" className="icon-button" aria-label={`Remove ${label.toLowerCase()} ${i + 1}`} title="Remove"
            onClick={() => onChange(rows.filter((_, n) => n !== i))}><X size={13} /></button>
        </div>;
      })}
    </div>}
    {!rows.length && <p className="pp-empty">None yet.</p>}
  </div>;
}

const permissions: BehaviorManifest["permissions"] = ["canRunInSleep", "canRunOnPod", "canStandUp", "canSitDown"];
const localized = [["names", "Names"], ["descriptions", "Descriptions"], ["tags", "Tags"], ["triggerSentences", "Trigger sentences"], ["loadingResponses", "Loading responses"]] as const;
const robotFields = [["minHeadVersion", "Minimum head version"], ["maxHeadVersion", "Maximum head version"], ["minBodyVersion", "Minimum body version"], ["maxBodyVersion", "Maximum body version"]] as const;

export function ProjectProperties({ initial, initialBehaviorPath, onApply, onClose, onRaw }: {
  initial: Manifest; initialBehaviorPath?: string; onApply: (manifest: Manifest) => Promise<unknown>; onClose: () => void; onRaw: () => void;
}) {
  const form = useRef<HTMLFormElement>(null);
  useEffect(() => {
    const section = [...(form.current?.querySelectorAll<HTMLDetailsElement>(".behavior-properties") ?? [])].find(element => element.dataset.path === initialBehaviorPath);
    if (section) {section.open = true; section.scrollIntoView({block: "center"}); section.querySelector("input")?.focus({preventScroll: true});}
  }, [initialBehaviorPath]);
  const [manifest, setManifest] = useState(initial);
  const [languages, setLanguages] = useState(initial.supportedLanguages);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const requirements = manifest.requirements ?? { naoqi: [], robots: [] };
  const setRequirements = (value: Requirements) => setManifest({ ...manifest, requirements: value.naoqi.length || value.robots.length ? value : null });
  const setBehavior = (i: number, patch: Partial<BehaviorManifest>) => setManifest({ ...manifest, behaviors: manifest.behaviors!.map((b, n) => n === i ? { ...b, ...patch } : b) });
  const setRobot = (i: number, patch: Partial<Requirements["robots"][number]>) =>
    setRequirements({ ...requirements, robots: requirements.robots.map((r, n) => n === i ? { ...r, ...patch } : r) });
  return <Modal title="Project properties" wide onClose={() => !busy && onClose()}>
    <form ref={form} className="settings-content project-properties" onSubmit={async e => {
      e.preventDefault();
      if (busy) return;
      setBusy(true); setError("");
      try {
        const result = await onApply({ ...manifest, supportedLanguages: languages });
        if (result) onClose(); else setError("Could not apply project properties. See the error notification for details.");
      } catch (e) { setError(String(e)); }
      finally { setBusy(false); }
    }}>
      <fieldset disabled={busy}>
        <section className="pp-section">
          <h3>Application</h3>
          <Field label="Application ID"><input value={manifest.uuid} onChange={e => setManifest({ ...manifest, uuid: e.target.value })} /></Field>
          <Field label="Version"><input value={manifest.version} placeholder="0.0.0" onChange={e => setManifest({ ...manifest, version: e.target.value })} /></Field>
          <div className="pp-field">
            <span>Supported languages<small>Languages the robot can run this app in</small></span>
            <SupportedLanguages value={languages} onChange={setLanguages} />
          </div>
        </section>
        <section className="pp-section">
          <h3>Store listing</h3>
          <LocalizedRows label="Titles" languages={languages} rows={manifest.names} onChange={names => setManifest({ ...manifest, names })} />
          <LocalizedRows label="Descriptions" languages={languages} rows={manifest.descriptions ?? []} onChange={descriptions => setManifest({ ...manifest, descriptions })} />
        </section>
        <section className="pp-section">
          <h3>Behaviors <span className="pp-count">{manifest.behaviors?.length ?? 0}</span></h3>
          {(manifest.behaviors ?? []).map((b, i) => <details key={b.path} className="behavior-properties" data-path={b.path}>
            <summary><span className="pp-summary-title">{b.path}</span><span className="badge">{b.nature === "solitary" ? "Solitary" : "Interactive"}</span>{b.userRequestable && <span className="badge">User-requestable</span>}</summary>
            <div className="pp-behavior">
              <Field label="Nature"><select aria-label={`${b.path} nature`} value={b.nature} onChange={e => setBehavior(i, { nature: e.target.value as BehaviorManifest["nature"] })}><option value="interactive">Interactive</option><option value="solitary">Solitary</option></select></Field>
              <div className="pp-field">
                <span>Launch</span>
                <label className="checkbox"><input type="checkbox" checked={b.userRequestable} onChange={e => setBehavior(i, { userRequestable: e.target.checked })} />User-requestable</label>
              </div>
              <div className="pp-field">
                <span>Permissions</span>
                <div className="pp-checks">{permissions.map(permission => <label className="checkbox" key={permission}>
                  <input type="checkbox" checked={b.permissions.includes(permission)} onChange={e => setBehavior(i, { permissions: e.target.checked ? [...b.permissions, permission] : b.permissions.filter(p => p !== permission) })} />{permission}
                </label>)}</div>
              </div>
              {localized.map(([key, label]) => <LocalizedRows key={key} label={label} languages={languages} rows={b[key]} onChange={rows => setBehavior(i, { [key]: rows })} />)}
              <LocalizedRows label="Launch conditions" conditions rows={b.conditions} onChange={conditions => setBehavior(i, { conditions })} />
            </div>
          </details>)}
          {!manifest.behaviors?.length && <p className="pp-empty">No behavior properties in this manifest.</p>}
        </section>
        <section className="pp-section">
          <details className="requirements-properties"><summary><h3>Requirements</h3><span className="pp-count">{requirements.naoqi.length + requirements.robots.length}</span></summary>
            <div className="pp-list">
              <div className="pp-list-head">
                <span>NAOqi versions</span>
                <button type="button" className="pp-add" onClick={() => setRequirements({ ...requirements, naoqi: [...requirements.naoqi, ["", ""]] })}><Plus size={12} />Add NAOqi requirement</button>
              </div>
              {requirements.naoqi.length > 0 && <div className="pp-table">
                <span className="pp-col">Minimum</span><span className="pp-col">Maximum</span><span />
                {requirements.naoqi.map(([min, max], i) => <div className="pp-table-row" key={i}>
                  <input aria-label={`NAOqi minimum ${i + 1}`} placeholder="2.8" value={min} onChange={e => setRequirements({ ...requirements, naoqi: requirements.naoqi.map((r, n) => n === i ? [e.target.value, max] : r) })} />
                  <input aria-label={`NAOqi maximum ${i + 1}`} placeholder="Any" value={max} onChange={e => setRequirements({ ...requirements, naoqi: requirements.naoqi.map((r, n) => n === i ? [min, e.target.value] : r) })} />
                  <button type="button" className="icon-button" aria-label={`Remove NAOqi requirement ${i + 1}`} title="Remove" onClick={() => setRequirements({ ...requirements, naoqi: requirements.naoqi.filter((_, n) => n !== i) })}><X size={13} /></button>
                </div>)}
              </div>}
            </div>
            <div className="pp-list">
              <div className="pp-list-head">
                <span>Robots</span>
                <button type="button" className="pp-add" onClick={() => setRequirements({ ...requirements, robots: [...requirements.robots, { model: "NAO", minHeadVersion: "", maxHeadVersion: "", minBodyVersion: "", maxBodyVersion: "" }] })}><Plus size={12} />Add robot requirement</button>
              </div>
              {requirements.robots.map((r, i) => <div className="pp-robot" key={i}>
                <div className="pp-robot-head">
                  <label className="pp-inline">Robot {i + 1}<input aria-label={`Robot ${i + 1} model`} value={r.model} onChange={e => setRobot(i, { model: e.target.value })} /></label>
                  <button type="button" className="icon-button" aria-label={`Remove robot requirement ${i + 1}`} title="Remove robot requirement" onClick={() => setRequirements({ ...requirements, robots: requirements.robots.filter((_, n) => n !== i) })}><X size={13} /></button>
                </div>
                <div className="pp-robot-grid">
                  <span />
                  <span className="pp-col">Minimum</span>
                  <span className="pp-col">Maximum</span>
                  {(["Head", "Body"] as const).map(part => <div className="pp-robot-row" key={part}>
                    <span>{part} version</span>
                    {robotFields.filter(([key]) => key.includes(part)).map(([key, label]) =>
                      <input key={key} aria-label={`Robot ${i + 1} ${label.toLowerCase()}`} placeholder="Any" value={r[key]} onChange={e => setRobot(i, { [key]: e.target.value })} />)}
                  </div>)}
                </div>
              </div>)}
              {!requirements.robots.length && <p className="pp-empty">Any robot.</p>}
            </div>
          </details>
        </section>
        <div className="pp-footer">
          <button type="button" onClick={onRaw}>Edit requirements and package contents…</button>
          <span className="spacer" />
          {error && <p className="error" role="alert">{error}</p>}
          <button type="button" onClick={onClose}>Cancel</button>
          <button type="submit" className="primary">{busy ? "Applying…" : "Apply"}</button>
        </div>
      </fieldset>
    </form>
  </Modal>;
}
