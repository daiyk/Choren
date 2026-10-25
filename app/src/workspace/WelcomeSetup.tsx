import { useState } from "react";
import { Boxes, Check, FolderOpen, Image, PersonStanding, Rotate3d } from "lucide-react";
import { Modal } from "./Modal";
import { chooseLibrary } from "../platform";
import { setShareDir, useShareDir } from "../icons";

/** The app icon (icons/icon.svg) without its margin. */
export function AppMark() {
  return (
    <svg className="setup-mark" viewBox="100 100 824 824" aria-hidden="true">
      <defs>
        <linearGradient id="setup-mark-bg" x1="0" y1="0" x2="0" y2="1">
          <stop offset="0" stopColor="#1f2633" />
          <stop offset="1" stopColor="#121720" />
        </linearGradient>
        <linearGradient id="setup-mark-wire" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#79acff" />
          <stop offset="1" stopColor="#3979db" />
        </linearGradient>
      </defs>
      <rect x="100" y="100" width="824" height="824" rx="185" fill="url(#setup-mark-bg)" />
      <g transform="translate(18 0)">
        <path d="M653.4 370.6 A200 200 0 1 0 653.4 653.4" fill="none" stroke="url(#setup-mark-wire)" strokeWidth="72" strokeLinecap="round" />
        <circle cx="653.4" cy="370.6" r="56" fill="#fff" />
        <circle cx="653.4" cy="653.4" r="56" fill="#fff" />
      </g>
    </svg>
  );
}

/** First-run window: what Choren is, and where Choregraphe is installed. */
export function WelcomeSetup({ onClose }: { onClose: () => void }) {
  const shareDir = useShareDir();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const choose = async () => {
    setBusy(true);
    setError("");
    try {
      const dir = await chooseLibrary();
      if (dir) setShareDir(dir);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };
  return (
    <Modal title="Welcome to Choren" wide onClose={onClose}>
      <div className="setup">
        <div className="setup-intro">
          <AppMark />
          <div>
            <h3>Your studio for Choregraphe behaviors</h3>
            <p>
              Open .pml, .crg and .xar projects, edit flow diagrams, timelines and Python boxes, and
              run them on a virtual or real NAO.
            </p>
          </div>
        </div>
        <section className="setup-step" aria-labelledby="setup-choregraphe">
          <h3 id="setup-choregraphe">
            Link your Choregraphe installation <span className="badge">Recommended</span>
          </h3>
          <p className="muted">Choren works on its own. With Choregraphe it also gives you:</p>
          <ul className="setup-gains">
            <li><Boxes size={16} />Choregraphe’s box libraries</li>
            <li><Image size={16} />The original box and port icons</li>
            <li><Rotate3d size={16} />The 3-D NAO model in the Robot view</li>
            <li><PersonStanding size={16} />Standard poses and robot postures</li>
          </ul>
          <div className="setup-folder" data-linked={!!shareDir}>
            <span className="setup-folder-icon">{shareDir ? <Check size={16} /> : <FolderOpen size={16} />}</span>
            <span className="setup-folder-text">
              <strong>{shareDir ? "Choregraphe linked" : "Not linked yet"}</strong>
              <small title={shareDir ?? undefined}>
                {shareDir || "Choose Choregraphe.app, its installation folder or share/choregraphe."}
              </small>
            </span>
            <button className={shareDir ? undefined : "primary"} disabled={busy} onClick={() => void choose()}>
              {busy ? "Choosing…" : shareDir ? "Change…" : "Choose Choregraphe…"}
            </button>
          </div>
          {error && <p className="error setup-error" role="alert">{error}</p>}
        </section>
        <div className="setup-footer">
          <span className="muted">You can change this later in Settings.</span>
          <button className={shareDir ? "primary" : undefined} onClick={onClose}>
            {shareDir ? "Get started" : "Skip for now"}
          </button>
        </div>
      </div>
    </Modal>
  );
}
