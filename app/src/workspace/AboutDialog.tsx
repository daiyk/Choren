import { bundle, version } from "../../src-tauri/tauri.conf.json";
import { Modal } from "./Modal";
import { AppMark } from "./WelcomeSetup";

/** Help › About Choren (on macOS, Choren › About Choren): the same window on every platform. */
export function AboutDialog({ onClose }: { onClose: () => void }) {
  return (
    <Modal title="About Choren" onClose={onClose}>
      <div className="settings-content">
        <div className="setup-intro">
          <AppMark />
          <div>
            <h3>Choren</h3>
            <p>Version {version}</p>
          </div>
        </div>
        <p>Studio compatible with Choregraphe projects. Not affiliated with Aldebaran or SoftBank Robotics.</p>
        <p className="muted">{bundle.copyright}. Released under the {bundle.license} license.</p>
      </div>
    </Modal>
  );
}
