import { expect, test, vi } from "vitest";

vi.mock("../src/platform", () => ({ native: false }));
vi.mock("../src/icons", () => ({ getShareDir: () => null }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));

import { invoke } from "@tauri-apps/api/core";
import { getRobot, robot } from "../src/robot/store";

test("connection lifecycle blocks duplicate requests and preserves status on failed disconnect", async () => {
  const status = { virtual: false, model: "nao6", url: "tcp://robot:9559" };
  let finish!: (value: typeof status) => void;
  vi.mocked(invoke).mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
  const connecting = robot.connect(status.url);
  expect(getRobot().busy).toBe(true);
  const calls = vi.mocked(invoke).mock.calls.length;
  await robot.startVirtual();
  await robot.disconnect();
  expect(vi.mocked(invoke).mock.calls.length).toBe(calls);
  finish(status);
  await connecting;
  expect(getRobot().status).toEqual(status);
  expect(getRobot().busy).toBe(false);
  vi.mocked(invoke).mockRejectedValueOnce(new Error("Disconnect failed"));
  await expect(robot.disconnect()).rejects.toThrow("Disconnect failed");
  expect(getRobot().status).toEqual(status);
  expect(getRobot().busy).toBe(false);
  vi.mocked(invoke).mockResolvedValue(undefined);
  await robot.disconnect();
  expect(getRobot().status).toBeUndefined();
  vi.mocked(invoke).mockRejectedValueOnce(new Error("No answer"));
  await robot.connect(status.url);
  expect(getRobot().status).toBeUndefined();
  expect(getRobot().busy).toBe(false);
  expect(getRobot().error).toContain("No answer");
  robot.clearError();
});

test("a new play clears a prior runtime error before execution", async () => {
  robot.reportError("Previous run failed");
  vi.mocked(invoke).mockImplementation(async () => {
    expect(getRobot().error).toBeUndefined();
    // An error reported during this run must not be cleared when invoke resolves.
    robot.reportError("Current run failed");
    return "demo/behavior_1";
  });
  await robot.play(7, "behavior_1");
  expect(invoke).toHaveBeenCalledWith("robot_play", {
    session: 7,
    behavior: "behavior_1",
  });
  expect(getRobot().error).toBe("Current run failed");
  robot.clearError();
});

test("play shows it is starting and ignores repeat clicks until the upload ends", async () => {
  let fail!: (e: Error) => void;
  vi.mocked(invoke).mockReset().mockImplementation(() => new Promise((_, reject) => { fail = reject; }));
  const first = robot.play(7, "behavior_1");
  expect(getRobot().starting).toBe(true);
  await robot.play(7, "behavior_1");
  expect(invoke).toHaveBeenCalledTimes(1);
  fail(new Error("The robot refused the package."));
  await expect(first).rejects.toThrow("refused");
  expect(getRobot().starting).toBe(false);
});

test("input activation rejects another session, behavior, revision, and a stopped run", async () => {
  vi.mocked(invoke).mockImplementation(async (command) => {
    if (command === "robot_start_virtual") return { virtual: true, model: "nao6", url: "local" };
    if (command === "robot_play") return "demo/behavior_1";
    if (command === "robot_call") return ["demo/behavior_1"];
  });
  await robot.startVirtual();
  await robot.play(7, "b1", 4);
  await robot.refreshRunning();
  await expect(robot.activateInput(8, "b1", 4, [], 2, null)).rejects.toThrow("not running");
  await expect(robot.activateInput(7, "b2", 4, [], 2, null)).rejects.toThrow("not running");
  await expect(robot.activateInput(7, "b1", 5, [], 2, null)).rejects.toThrow("changed since Play");
  await robot.activateInput(7, "b1", 4, [[0, 0, 5]], 2, null);
  expect(invoke).toHaveBeenLastCalledWith("robot_activate_input", { session: 7, behavior: "b1", path: [[0, 0, 5]], input: 2, value: null });
  vi.mocked(invoke).mockResolvedValue([]);
  await robot.refreshRunning();
  await expect(robot.activateInput(7, "b1", 4, [], 2, null)).rejects.toThrow("not running");
  await robot.disconnect();
  await expect(robot.activateInput(7, "b1", 4, [], 2, null)).rejects.toThrow("Connect");
});

import { inputActivationError, parameterValue } from "../src/robot/store";
test("live parameter types preserve false, zero, fractions and text and reject invalid numbers", () => {
  expect(parameterValue(0, "0")).toBe(false);
  expect(parameterValue(0, "true")).toBe(true);
  expect(parameterValue(1, "0")).toBe(0);
  expect(parameterValue(2, "0.25")).toBe(0.25);
  expect(parameterValue(3, "42")).toBe("42");
  expect(parameterValue(4, "sound.wav")).toBe("sound.wav");
  for (const text of ["", "not a number", "Infinity", "1.5"]) expect(() => parameterValue(1, text)).toThrow();
});

test("only consecutive successful parameter writes advance the played revision", async () => {
  vi.mocked(invoke).mockImplementation(async command => {
    if (command === "robot_start_virtual") return { virtual: true, model: "nao6", url: "local" };
    if (command === "robot_play") return "demo/b1";
    if (command === "robot_call") return ["demo/b1"];
  });
  await robot.startVirtual(); await robot.play(7, "b1", 4); await robot.refreshRunning();
  await robot.setParameters(7, "b1", [[0, 0, 5]], [["Speed", 0]], 4, 5);
  expect(inputActivationError(7, "b1", 5)).toBeUndefined();
  vi.mocked(invoke).mockRejectedValueOnce(new Error("No connection"));
  await expect(robot.setParameters(7, "b1", [], [["Enabled", false]], 5, 6)).rejects.toThrow("No connection");
  expect(getRobot().played?.revision).toBe(5);
  await robot.setParameters(7, "b1", [], [["Speed", 1]], 5, 8);
  expect(getRobot().played?.revision).toBe(5);
  await expect(robot.setParameters(7, "b1", [], [["Speed", 1]], 8, 9)).rejects.toThrow("changed since Play");
  await robot.disconnect();
});
