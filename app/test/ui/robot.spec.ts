import { expect, test, type Page } from "@playwright/test";
import { expectTidyLayout } from "./layout-audit";
import { readFileSync } from "node:fs";
const descriptor = JSON.parse(
  readFileSync(
    new URL("../../../crates/choren-robot/robots/nao6.json", import.meta.url),
    "utf8",
  ),
);

async function backend(page: Page) {
  await page.addInitScript((descriptor) => {
    let rid = 0;
    const calls: { command: string; args: unknown }[] = [];
    const status = {
      url: "tcp://127.0.0.1:9559",
      model: "nao6",
      virtual: !localStorage.getItem("mock-physical"),
    };
    Object.assign(window, {
      calls,
      isTauri: true,
      __TAURI_INTERNALS__: {
        transformCallback: () => ++rid,
        invoke: async (command: string, args: any) => {
          calls.push({ command, args });
          if (command === "plugin:menu|new") return [++rid, String(rid)];
          if (command.startsWith("plugin:")) return;
          if (command === "library_tree") return [];
          if (command === "robot_status")
            return localStorage.getItem("mock-connected") ? status : null;
          if (command === "robot_models") return [{ id: "nao6", name: "NAO 6" }];
          if (command === "robot_discover") {
            if ((window as any).failDiscovery) throw new Error("Discovery unavailable");
            return (window as any).emptyDiscovery ? [] : [{ name: "Nao blue", url: "tcp://192.168.1.20:9559" }, { name: "Nao red", url: "tcp://192.168.1.21:9560" }];
          }
          if (command === "robot_connect") {
            if ((window as any).holdConnect) await new Promise((resolve) => { (window as any).finishConnect = resolve; });
            if ((window as any).failConnect) throw new Error("No answer from the robot");
            return { ...status, virtual: false, url: args.url };
          }
          if (command === "robot_start_virtual") {
            localStorage.setItem("mock-connected", "true");
            return status;
          }
          if (command === "robot_stop") {
            localStorage.removeItem("mock-running");
            return;
          }
          if (command === "robot_disconnect") {
            localStorage.removeItem("mock-connected");
            return;
          }
          if (command === "robot_descriptor") return descriptor;
          if (command === "robot_services") return args.service ? {description: "Test service", methods: [{id: 100, name: "echo", parameters: "(s)", returns: "s"}], signals: []} : ["TestService"];
          if (command === "robot_camera_frame") return {width: 2, height: 1, bytes: [255,0,0,0,255,0]};
          if (command === "robot_call") {
            if (args.method === "echo") return args.args[0];
            if (args.method === "subscribeCamera") return "test-camera";
            if (args.method === "getInstalledBehaviors") return ["demo/dance"];
            if (args.method === "startBehavior") {localStorage.setItem("mock-running", "true"); return;}
            if (args.method === "stopBehavior") {localStorage.removeItem("mock-running"); return;}
            if (args.method === "setMotionSpeed" && (window as any).failSpeed)
              throw new Error("Speed change failed");
            if (args.method === "getRunningBehaviors")
              return localStorage.getItem("mock-running") ? ["demo/dance"] : [];
            if (args.method === "hear" && (window as any).failSpeech)
              throw new Error("Speech input failed");
            return null;
          }
          throw new Error(`Unexpected IPC: ${command}`);
        },
      },
    });
  }, descriptor);
  await page.goto("/");
}

test("connection dialog chooses virtual or real robot, handles failure and disconnects", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Connection…", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Robot connection" });
  await expect(dialog.getByRole("radio", { name: "Virtual robot", exact: true })).toBeChecked();
  await expect(dialog.getByRole("combobox", { name: "Robot model" })).toHaveValue("nao6");
  await dialog.getByRole("button", { name: "Connect to virtual robot", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(".connection-status")).toHaveText("Virtual");
  await page.getByRole("button", { name: "Connection…", exact: true }).click();
  await expect(dialog).toContainText("Connected to virtual robot");
  await dialog.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(dialog).toContainText("Disconnected");
  await dialog.getByRole("radio", { name: "Real robot", exact: true }).check();
  await expect(dialog.getByRole("spinbutton", { name: "Port", exact: true })).toHaveValue("9561");
  await dialog.getByRole("textbox", { name: "Hostname or IP address" }).fill("robot.local");
  await dialog.getByRole("spinbutton", { name: "Port", exact: true }).fill("9560");
  await page.evaluate(() => { (window as any).failConnect = true; (window as any).holdConnect = true; });
  await dialog.getByRole("button", { name: "Connect to real robot", exact: true }).click();
  await expect(dialog.getByRole("button", { name: "Connecting…", exact: true })).toBeDisabled();
  await expect(dialog.getByRole("radio", { name: "Virtual robot", exact: true })).toBeDisabled();
  await page.evaluate(() => (window as any).finishConnect());
  await expect(dialog.getByRole("alert")).toContainText("No answer");
  await expect(dialog.getByRole("textbox", { name: "Hostname or IP address" })).toHaveValue("robot.local");
  await page.evaluate(() => { (window as any).failConnect = false; (window as any).holdConnect = false; });
  await dialog.getByRole("button", { name: "Connect to real robot", exact: true }).click();
  await expect(dialog).toBeHidden();
  await expect(page.locator(".connection-status")).toHaveText("Connected");
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === "robot_connect").map((c: any) => c.args)))
    .toEqual([{ url: "tcp://robot.local:9560" }, { url: "tcp://robot.local:9560" }]);
  await page.getByRole("button", { name: "Connection…", exact: true }).click();
  await expect(dialog).toContainText("Connected to real robot");
  await dialog.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(page.locator(".connection-status")).toHaveText("Offline");
  // The next dialog offers the last robot the app connected to.
  await page.reload();
  await page.getByRole("button", { name: "Connection…", exact: true }).click();
  await dialog.getByRole("radio", { name: "Real robot", exact: true }).check();
  await expect(dialog.getByRole("textbox", { name: "Hostname or IP address" })).toHaveValue("robot.local");
  await expect(dialog.getByRole("spinbutton", { name: "Port", exact: true })).toHaveValue("9560");
});

test("connection dialog fits compact workspaces and keyboard cancel preserves connection", async ({ page }) => {
  await backend(page);
  await page.setViewportSize({ width: 1024, height: 640 });
  await page.getByRole("button", { name: "Connection…", exact: true }).click();
  const dialog = page.getByRole("dialog", { name: "Robot connection" });
  await dialog.getByRole("radio", { name: "Real robot", exact: true }).check();
  const bounds = await dialog.boundingBox();
  expect(bounds!.y).toBeGreaterThanOrEqual(0);
  expect(bounds!.y + bounds!.height).toBeLessThanOrEqual(640);
  await expect(dialog.getByRole("button", { name: "Connect to real robot", exact: true })).toBeInViewport();
  await expectTidyLayout(page, "dialog[open]");
  await page.screenshot({ path: "/tmp/choren-connection.png" });
  await page.keyboard.press("Escape");
  await expect(dialog).toBeHidden();
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => ["robot_connect", "robot_start_virtual", "robot_disconnect"].includes(c.command)))).toEqual([]);
});

test("virtual movement speed can change live and retains its value on failure", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Offline", exact: true }).click();
  await page.getByRole("button", { name: "Start virtual NAO 6", exact: true }).click();
  await expect(page.getByRole("combobox", { name: /Movement speed/i })).toHaveCount(0);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  const settings = page.getByRole("dialog", { name: "Settings", exact: true });
  const speed = settings.getByRole("combobox", { name: "Virtual movement speed" });
  await expect(speed).toHaveValue("1");
  await speed.selectOption("0.5");
  await expect(speed).toHaveValue("0.5");
  await page.screenshot({ path: `/tmp/choren-speed-settings-${test.info().project.name}.png` });
  expect(await page.evaluate(() => (window as any).calls.find((c: any) => c.args?.method === "setMotionSpeed").args))
    .toEqual({ service: "ChorenSim", method: "setMotionSpeed", args: [0.5] });
  await page.evaluate(() => ((window as any).failSpeed = true));
  await speed.selectOption("2");
  await expect(settings.getByRole("alert")).toContainText("Speed change failed");
  await expect(speed).toHaveValue("0.5");
  await page.evaluate(() => ((window as any).failSpeed = false));
  await speed.selectOption("0.75");
  await expect(speed).toHaveValue("0.75");
  await settings.getByRole("button", { name: "Close dialog" }).click();
  await expect(page.getByRole("combobox", { name: /Movement speed/i })).toHaveCount(0);
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(speed).toHaveValue("0.75");
});

test("physical robot connections have no movement speed override", async ({ page }) => {
  await page.addInitScript(() => {
    localStorage.setItem("mock-physical", "true");
    localStorage.setItem("mock-connected", "true");
  });
  await backend(page);
  await page.locator(".connection-status").click();
  await expect(page.getByRole("combobox", { name: "Posture" })).toBeVisible();
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await expect(page.getByRole("combobox", { name: /Movement speed/i })).toHaveCount(0);
});

test("robot view starts the virtual robot and sends posture commands", async ({
  page,
}) => {
  const errors: string[] = [];
  page.on("pageerror", (e) => errors.push(e.message));
  await backend(page);
  await page.getByRole("button", { name: /Search commands/ }).click();
  await page
    .getByRole("textbox", { name: "Search commands" })
    .fill("Start virtual NAO 6");
  await page.getByRole("button", { name: /^Start virtual NAO 6/ }).click();
  await expect(page.locator(".robot-panel > .panel-toolbar")).toContainText(
    "Virtual NAO 6 · tcp://127.0.0.1:9559",
  );
  await expect(page.getByRole("img", { name: "3-D robot view" })).toBeVisible();
  await expect(page.locator(".connection-status")).toHaveText("Virtual");
  await page.getByRole("combobox", { name: "Posture" }).selectOption("Sit");
  await page.getByRole("button", { name: "Go", exact: true }).click();
  await page.locator("summary", { hasText: "Simulated touch sensors" }).click();
  await page.getByRole("button", { name: "Head Front" }).click();
  const robotCalls = await page.evaluate(() =>
    (window as any).calls
      .filter(
        (c: any) =>
          c.command === "robot_call" && c.args.method !== "getRunningBehaviors",
      )
      .map((c: any) => c.args),
  );
  expect(robotCalls).toEqual([
    { service: "ALRobotPosture", method: "goToPosture", args: ["Sit", 0.8] },
    { service: "ChorenSim", method: "press", args: ["HeadFront"] },
  ]);
  expect(errors).toEqual([]);
});

test("robot speech retains failed input and reconnects to a running backend after reload", async ({
  page,
}) => {
  await backend(page);
  await page.getByRole("button", { name: "Offline", exact: true }).click();
  await page
    .getByRole("button", { name: "Start virtual NAO 6", exact: true })
    .click();
  await page
    .getByRole("textbox", { name: "Say to the robot" })
    .fill("hello robot");
  await page.evaluate(() => ((window as any).failSpeech = true));
  await page.getByRole("button", { name: "Send speech" }).click();
  await expect(page.locator(".robot-panel").getByRole("alert")).toContainText(
    "Speech input failed",
  );
  await expect(
    page.getByRole("textbox", { name: "Say to the robot" }),
  ).toHaveValue("hello robot");
  await page.evaluate(() => ((window as any).failSpeech = false));
  await page.getByRole("button", { name: "Send speech" }).click();
  await expect(
    page.getByRole("textbox", { name: "Say to the robot" }),
  ).toHaveValue("");
  await page.evaluate(() => localStorage.setItem("mock-running", "true"));
  await page.reload();
  await expect(page.locator(".connection-status")).toHaveText("Virtual · Running");
  // Reconnection must work even if the panel layout has not been persisted yet.
  await page.locator(".connection-status").click();
  await expect(page.locator(".robot-runtime")).toHaveText(
    "Running: demo/dance",
  );
  await page
    .locator(".robot-panel")
    .getByRole("button", { name: "Stop", exact: true })
    .click();
  await expect(page.locator(".robot-runtime")).toHaveText("Robot ready");
  await page.getByRole("button", { name: "Disconnect", exact: true }).click();
  await expect(page.locator(".connection-status")).toHaveText("Offline");
});

test("robot controls remain reachable in light and compact dark workspaces", async ({
  page,
}, info) => {
  await backend(page);
  await page
    .getByRole("button", { name: "Explore example", exact: true })
    .click();
  await page.getByRole("button", { name: "Offline", exact: true }).click();
  await page
    .getByRole("button", { name: "Start virtual NAO 6", exact: true })
    .click();
  await expect(page.getByRole("img", { name: "3-D robot view" })).toBeVisible();
  await page
    .getByRole("textbox", { name: "Say to the robot" })
    .fill("Hello NAO");
  await page.screenshot({ path: info.outputPath("studio-robot-light.png") });
  await page.setViewportSize({ width: 1024, height: 640 });
  await page.getByRole("button", { name: "Settings", exact: true }).click();
  await page.getByRole("combobox", { name: "Appearance" }).selectOption("dark");
  await page.getByRole("button", { name: "Close dialog", exact: true }).click();
  await page.getByRole("button", { name: "Send speech", exact: true }).click();
  await expect(
    page.getByRole("textbox", { name: "Say to the robot" }),
  ).toHaveValue("");
  expect(
    await page.evaluate(
      () => document.documentElement.scrollWidth <= innerWidth,
    ),
  ).toBe(true);
  await expectTidyLayout(page);
  await page.screenshot({ path: info.outputPath("studio-robot-compact.png") });
});

test("service inspector calls methods and application controls run and stop installed behavior", async ({page}) => {
  await page.addInitScript(() => localStorage.setItem("mock-connected", "true"));
  await backend(page);
  const panel = async (name: string) => {
    await page.getByRole("button", {name: /Search commands/}).click();
    await page.getByRole("textbox", {name: "Search commands"}).fill(name);
    await page.getByRole("button", {name: new RegExp(`^${name}`)}).click();
  };
  await panel("Show Service inspector");
  await expect(page.getByRole("combobox", {name: "Robot service"})).toHaveValue("TestService");
  await page.getByRole("textbox", {name: "Method arguments"}).fill('["hello"]');
  await page.getByRole("button", {name: "Call method", exact: true}).click();
  await expect(page.getByLabel("Service result")).toContainText('"hello"');
  await page.getByRole("textbox", {name: "Method arguments"}).fill('{}');
  await page.getByRole("button", {name: "Call method", exact: true}).click();
  await expect(page.getByRole("form", {name: "Service call", exact: true}).getByRole("alert")).toContainText("JSON array");
  await panel("Show Robot applications");
  await page.getByRole("button", {name: "Run", exact: true}).click();
  await expect(page.getByRole("button", {name: "Stop", exact: true})).toBeEnabled();
  await page.getByRole("button", {name: "Stop", exact: true}).click();
  await expect(page.getByRole("button", {name: "Run", exact: true})).toBeEnabled();
});

test("camera converts RGB frames and unsubscribes when stopped", async ({page}) => {
  await page.addInitScript(() => {localStorage.setItem("mock-connected", "true"); localStorage.setItem("mock-physical", "true");});
  await backend(page);
  await page.getByRole("button", {name: /Search commands/}).click();
  await page.getByRole("textbox", {name: "Search commands"}).fill("Show Camera");
  await page.getByRole("button", {name: /^Show Camera/}).click();
  await page.getByRole("button", {name: "Start camera", exact: true}).click();
  const canvas = page.getByLabel("Robot camera image");
  await expect.poll(() => canvas.evaluate((el: HTMLCanvasElement) => [...el.getContext("2d")!.getImageData(0,0,2,1).data])).toEqual([255,0,0,255,0,255,0,255]);
  await page.getByRole("button", {name: "Stop camera", exact: true}).click();
  await expect.poll(() => page.evaluate(() => (window as any).calls.some((c: any) => c.command === "robot_call" && c.args.method === "unsubscribe" && c.args.args[0] === "test-camera"))).toBe(true);
});

test("active content records stopped behaviors, filters history and exposes timestamps", async ({page}, info) => {
  await page.addInitScript(() => {localStorage.setItem("mock-connected", "true"); localStorage.setItem("mock-running", "true");});
  await backend(page);
  await page.getByRole("button", {name: /Search commands/}).click();
  await page.getByRole("textbox", {name: "Search commands"}).fill("Show Active content");
  await page.getByRole("button", {name: /^Show Active content/}).click();
  await expect(page.getByRole("table")).toContainText("demo/dance");
  await page.getByRole("combobox", {name: "Timestamp format"}).selectOption("milliseconds");
  await page.getByRole("button", {name: "Stop all active content", exact: true}).click();
  await expect(page.getByRole("table")).toContainText("Stopped");
  await page.screenshot({path: info.outputPath("parity-active-content.png")});
  await page.getByRole("checkbox", {name: "Show history", exact: true}).uncheck();
  await expect(page.getByRole("table")).not.toContainText("demo/dance");
  await page.getByRole("checkbox", {name: "Show history", exact: true}).check();
  await page.getByRole("button", {name: "Clear history", exact: true}).click();
  await expect(page.getByRole("table")).not.toContainText("demo/dance");
});


test("backend wiring: discovery refreshes and connects using the advertised endpoint", async ({ page }) => {
  await backend(page);
  await page.getByRole("button", { name: "Connection…", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Robot connection" });
  await modal.getByRole("radio", { name: "Real robot", exact: true }).check();
  await expect(modal.getByRole("button", { name: /Nao blue/ })).toBeVisible();
  await page.evaluate(() => (window as any).emptyDiscovery = true);
  await modal.getByRole("button", { name: "Refresh robots" }).click();
  await expect(modal.getByText("No robots found", { exact: true })).toBeVisible();
  await page.evaluate(() => (window as any).failDiscovery = true);
  await modal.getByRole("button", { name: "Refresh robots" }).click();
  await expect(modal.getByRole("alert")).toContainText("Discovery unavailable");
  await page.evaluate(() => { (window as any).failDiscovery = false; (window as any).emptyDiscovery = false; });
  await modal.getByRole("button", { name: "Refresh robots" }).click();
  await modal.getByRole("button", { name: /Nao red/ }).click();
  await expect(page.locator(".connection-status")).toHaveText("Connected");
  expect(await page.evaluate(() => (window as any).calls.filter((c: any) => c.command === "robot_connect").map((c: any) => c.args))).toEqual([{ url: "tcp://192.168.1.21:9560" }]);
});
