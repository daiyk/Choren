import { expect, test, vi } from "vitest";
const { handlers } = vi.hoisted(() => ({ handlers: new Map<string, (event: {payload: unknown}) => void>() }));
vi.mock("../src/platform", () => ({ native: true }));
vi.mock("../src/icons", () => ({ getShareDir: () => null }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async (name, handler) => { handlers.set(name, handler); return () => {}; }) }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
import { invoke } from "@tauri-apps/api/core";
import { getRobot, robot, startRobotEvents } from "../src/robot/store";
const emit = (name: string, payload: unknown) => handlers.get(name)!({payload});

test("frame batches clear on Play, Stop and disconnect; conversation is bounded to 100 lines", async () => {
  vi.useFakeTimers();
  try {
    vi.mocked(invoke).mockImplementation(async command => {
      if (command === "robot_connect") return { url: "tcp://nao:9559", virtual: false, model: "nao6" };
      if (command === "robot_play") return "demo/b1";
    });
    startRobotEvents(); await robot.connect("tcp://nao:9559");
    emit("robot://frame", ["demo/b1", [3, 4], 20]);
    emit("robot://frame", ["demo/b1", [3, 4], 21]);
    expect(getRobot().frames).toEqual({});
    vi.advanceTimersByTime(100);
    expect(getRobot().frames).toEqual({"demo/b1|3/4": 21});
    emit("robot://frame", ["demo/b1", [3, 4], 22]);
    await robot.play(1, "b1", 1); vi.advanceTimersByTime(100);
    expect(getRobot().frames).toEqual({});
    emit("robot://frame", ["demo/b1", [3], -1]);
    vi.advanceTimersByTime(100);
    expect(getRobot().frames["demo/b1|3"]).toBe(-1);
    await robot.stop(); expect(getRobot().frames).toEqual({});
    for (let i = 0; i < 101; i++) emit("robot://dialog-input", [String(i)]);
    emit("robot://dialog-answer", ["Answer"]);
    expect(getRobot().dialog).toHaveLength(100);
    expect(getRobot().dialog[0]).toEqual({speaker: "Human", text: "2"});
    expect(getRobot().dialog.at(-1)).toEqual({speaker: "Robot", text: "Answer"});
    emit("robot://frame", ["demo/b1", [3], 23]);
    emit("robot://disconnected", null); vi.advanceTimersByTime(100);
    expect(getRobot().frames).toEqual({}); expect(getRobot().dialog).toEqual([]);
    emit("robot://dialog-answer", ["Late answer"]);
    expect(getRobot().dialog).toEqual([]);
  } finally { vi.useRealTimers(); }
});
