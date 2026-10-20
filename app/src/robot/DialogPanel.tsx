import { useEffect, useRef, useState } from "react";
import { getRobot, robot, useRobot } from "./store";

/** Live conversation; topic-file editing remains in the Dialog / QiChat panel. */
export function RobotDialogPanel() {
  const status = useRobot(s => s.status);
  const transcript = useRobot(s => s.dialog);
  const [text, setText] = useState("");
  const [language, setLanguage] = useState("");
  const [languages, setLanguages] = useState<string[]>([]);
  const [topics, setTopics] = useState<string[]>([]);
  const [error, setError] = useState("");
  const [loadError, setLoadError] = useState("");
  const [busy, setBusy] = useState(false);
  const log = useRef<HTMLDivElement>(null);
  useEffect(() => { log.current?.scrollTo(0, log.current.scrollHeight); }, [transcript]);
  useEffect(() => {
    let live = true;
    let timer: ReturnType<typeof setTimeout>;
    setLanguage(""); setLanguages([]); setTopics([]); setError(""); setLoadError(""); setText("");
    const refresh = async () => {
      const results = await Promise.allSettled([
        robot.call("ALDialog", "getLanguage"), robot.call("ALDialog", "getActivatedTopics"), robot.call("ALTextToSpeech", "getAvailableLanguages"),
      ]);
      if (!live || getRobot().status !== status) return;
      const [lang, active, available] = results;
      if (lang.status === "fulfilled" && typeof lang.value === "string") setLanguage(lang.value);
      if (active.status === "fulfilled" && Array.isArray(active.value)) setTopics(active.value.filter((v): v is string => typeof v === "string"));
      if (available.status === "fulfilled" && Array.isArray(available.value)) setLanguages(available.value.filter((v): v is string => typeof v === "string"));
      setLoadError(results.filter(r => r.status === "rejected").map(r => String(r.reason)).join("\n"));
      timer = setTimeout(() => void refresh(), 2000);
    };
    if (status) void refresh();
    return () => { live = false; clearTimeout(timer); };
  }, [status]);
  const run = async (work: () => Promise<unknown>) => {
    if (!status || busy) return;
    setBusy(true); setError("");
    try { await work(); }
    catch (e) { if (getRobot().status === status) setError(String(e)); }
    finally { setBusy(false); }
  };
  return <div className="panel-column robot-dialog">
    <div className="panel-toolbar">
      <label>Language <select aria-label="Dialog language" value={language} disabled={!status || busy || !language} onChange={e => {
        const next = e.target.value;
        void run(async () => { await robot.call("ALDialog", "setLanguage", next); if (getRobot().status === status) setLanguage(next); });
      }}><option value="" disabled>Choose language</option>{[...new Set([language, ...languages])].filter(Boolean).map(l => <option key={l}>{l}</option>)}</select></label>
      <button onClick={robot.clearDialog} disabled={!status}>Clear conversation</button>
    </div>
    <p className="panel-message">{status ? `Active topics: ${topics.join(", ") || "none"}` : "Connect to a robot to start a conversation."}</p>
    {(error || loadError) && <p className="error" role="alert">{error || loadError}</p>}
    <div className="panel-scroll dialog-transcript" role="log" aria-label="Robot conversation" ref={log}>
      {transcript.map((line, i) => <p key={i}><strong>{line.speaker}:</strong> {line.text}</p>)}
    </div>
    <form className="panel-toolbar" onSubmit={e => {
      e.preventDefault();
      if (!text.trim()) return;
      void run(async () => {
        await robot.call("ALDialog", "forceInput", text.trim());
        if (getRobot().status === status) setText("");
      });
    }}>
      <input aria-label="Say to robot" placeholder="Type a sentence…" value={text} disabled={!status || busy} onChange={e => setText(e.target.value)} />
      <button type="submit" disabled={!status || busy || !text.trim()}>Send</button>
    </form>
  </div>;
}
