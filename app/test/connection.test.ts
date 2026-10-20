import { expect, test } from "vitest";
import { robotAddress } from "../src/robot/Connection";

test("connection address has an explicit validated port and supports IPv6", () => {
  expect(robotAddress(" nao.local ", "9559")).toBe("tcp://nao.local:9559");
  expect(robotAddress("192.168.1.10", "1234")).toBe("tcp://192.168.1.10:1234");
  expect(robotAddress("::1", "9559")).toBe("tcp://[::1]:9559");
  expect(robotAddress("[fe80::1]", "9559")).toBe("tcp://[fe80::1]:9559");
  for (const host of ["", "a b", "tcp://robot", "host:9559", "user@robot", "../robot", "[oops]"])
    expect(() => robotAddress(host, "9559")).toThrow();
  for (const port of ["", "0", "65536", "9.5", "abc", "1e3"])
    expect(() => robotAddress("nao.local", port)).toThrow();
});
