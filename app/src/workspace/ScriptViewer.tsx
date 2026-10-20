import { useEffect, useRef, useState } from "react";
import { autocompletion, completionKeymap } from "@codemirror/autocomplete";
import { readPreference, savePreference } from "./storage";
import { Compartment, EditorState } from "@codemirror/state";
import {
  EditorView,
  lineNumbers,
  highlightActiveLineGutter,
  keymap,
} from "@codemirror/view";
import {
  ensureSyntaxTree,
  indentUnit,
  defaultHighlightStyle,
  syntaxHighlighting,
  bracketMatching,
} from "@codemirror/language";
import { python } from "@codemirror/lang-python";
import {
  search,
  searchKeymap,
  highlightSelectionMatches,
} from "@codemirror/search";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import type { Box } from "../format/types";
export function ScriptViewer({
  box,
  onSave,
  initialDraft,
  onDraft,
}: {
  /** The box as in the current document; its script follows Undo/Redo. */
  box: Box;
  /** Present when the script can be edited; receives the new text. */
  onSave?: (text: string) => void;
  /** Unapplied text kept from an earlier editor of this tab (it outlives the closed tab). */
  initialDraft?: string;
  /** Reports the editor text when it differs from the document, `undefined` when it does not. */
  onDraft?: (text: string | undefined) => void;
}) {
  const [fontSize, setFontSize] = useState(() => Number(readPreference("choren.script-font")) || 12);
  const [tabSize, setTabSize] = useState(() => Number(readPreference("choren.script-indent")) || 4);
  const [wrap, setWrap] = useState(() => readPreference("choren.script-wrap") === true);
  const [validation, setValidation] = useState("");
  const preferences = useRef(new Compartment());
  const preferenceExtensions = () => [EditorState.tabSize.of(tabSize), indentUnit.of(" ".repeat(tabSize)),
    EditorView.theme({".cm-scroller": {fontSize: `${fontSize}px`}}), ...(wrap ? [EditorView.lineWrapping] : [])];
  const host = useRef<HTMLDivElement>(null);
  const view = useRef<EditorView>(undefined);
  const committed = useRef(box.script);
  const [changed, setChanged] = useState(false);
  const editable = !!onSave;
  const draft = useRef(onDraft);
  draft.current = onDraft;
  const report = (text: string) => {
    const dirty = text !== committed.current;
    setChanged(dirty);
    draft.current?.(dirty ? text : undefined);
  };
  // A committed change (Apply, Undo, Redo) replaces the text unless the user has a draft.
  useEffect(() => {
    committed.current = box.script;
    const v = view.current;
    if (!v) return;
    const text = v.state.doc.toString();
    if (!changed && text !== box.script)
      v.dispatch({ changes: { from: 0, to: text.length, insert: box.script } });
    else report(text);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [box.script]);
  useEffect(() => {
    if (!host.current) return;
    committed.current = box.script;
    // An empty draft is still a draft (the user cleared the script).
    const initial =
      editable && initialDraft !== undefined ? initialDraft : box.script;
    setChanged(initial !== box.script);
    const editor = new EditorView({
      parent: host.current,
      state: EditorState.create({
        doc: initial,
        extensions: [
          EditorState.readOnly.of(!editable),
          EditorView.editable.of(editable),
          EditorView.contentAttributes.of({
            tabindex: "0",
            "aria-label": `${box.name} Python script${editable ? "" : ", read-only"}`,
          }),
          EditorView.updateListener.of((u) => {
            if (u.docChanged) report(u.state.doc.toString());
          }),
          history(),
          autocompletion(),
          lineNumbers(),
          highlightActiveLineGutter(),
          syntaxHighlighting(defaultHighlightStyle),
          python(),
          bracketMatching(),
          search({ top: true }),
          highlightSelectionMatches(),
          keymap.of([...completionKeymap, ...searchKeymap, ...historyKeymap, ...defaultKeymap]),
          EditorView.theme({
            "&": {
              height: "100%",
              color: "var(--text)",
              backgroundColor: "var(--surface)",
            },
            ".cm-scroller": {
              overflow: "auto",
              fontFamily: "ui-monospace, SFMono-Regular, Consolas, monospace",
              fontSize: "12px",
            },
            ".cm-gutters": {
              backgroundColor: "var(--surface-raised)",
              color: "var(--muted)",
              border: "none",
            },
            ".cm-activeLineGutter": { backgroundColor: "var(--selection)" },
            ".cm-panels": {
              backgroundColor: "var(--surface-raised)",
              color: "var(--text)",
            },
            "&.cm-focused": {
              outline: "2px solid var(--focus)",
              outlineOffset: "-2px",
            },
            ".cm-selectionBackground, &.cm-focused .cm-selectionBackground, ::selection":
              { backgroundColor: "var(--selection) !important" },
          }),
          preferences.current.of(preferenceExtensions()),
        ],
      }),
    });
    view.current = editor;
    return () => editor.destroy();
    // The editor is recreated for another box, not for every document revision.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [box.node ?? box.name, editable]);
  useEffect(() => {
    savePreference("choren.script-font", fontSize); savePreference("choren.script-indent", tabSize); savePreference("choren.script-wrap", wrap);
    view.current?.dispatch({effects: preferences.current.reconfigure(preferenceExtensions())});
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fontSize, tabSize, wrap]);
  return (
    <section className="script-view">
      <div className="panel-toolbar">
        <span>{box.name}</span>
        <button onClick={() => {
          const state = view.current?.state;
          if (!state) return;
          const tree = ensureSyntaxTree(state, state.doc.length, 1000);
          if (!tree) {setValidation("The script is too large to finish checking. Check smaller sections."); return;}
          const lines = new Set<number>();
          tree.iterate({enter: node => {if (node.type.isError) lines.add(state.doc.lineAt(node.from).number);}});
          setValidation(lines.size ? `Syntax errors near line${lines.size > 1 ? "s" : ""} ${[...lines].join(", ")}.` : "No syntax errors found by the editor grammar.");
        }}>Check syntax</button>
        <label>Font<select aria-label="Script font size" value={fontSize} onChange={e => setFontSize(Number(e.target.value))}>{[10,12,14,16,18,20,24].map(v => <option key={v} value={v}>{v} px</option>)}</select></label>
        <label>Indent<select aria-label="Script indent size" value={tabSize} onChange={e => setTabSize(Number(e.target.value))}>{[2,4,8].map(v => <option key={v} value={v}>{v} spaces</option>)}</select></label>
        <label className="checkbox"><input type="checkbox" checked={wrap} onChange={e => setWrap(e.target.checked)} />Wrap lines</label>
        <span className="spacer" />
        {editable ? (
          <button
            disabled={!changed}
            onClick={() => {
              if (view.current) onSave!(view.current.state.doc.toString());
            }}
          >
            Apply to box
          </button>
        ) : (
          <span className="badge">Python · Read-only</span>
        )}
      </div>
      {validation && <p className="script-validation" role="status">{validation}</p>}
      <div className="code-editor" ref={host} />
    </section>
  );
}
