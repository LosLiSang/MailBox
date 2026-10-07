import { describe, expect, it } from "vitest";
import { detectProvider, formatMailDate, hue, initials } from "./format";

describe("detectProvider", () => {
  it.each([
    ["123@qq.com", "imap.qq.com"],
    [" ABC@QQ.COM ", "imap.qq.com"],
    ["a@foxmail.com", "imap.qq.com"],
    ["a@163.com", "imap.163.com"],
    ["a@126.com", "imap.126.com"],
    ["a@gmail.com", undefined],
    ["no-at-sign", undefined],
    ["", undefined],
  ])("%s -> %s", (email, host) => {
    expect(detectProvider(email)?.host).toBe(host);
  });
});

describe("formatMailDate", () => {
  const now = new Date(2026, 9, 7, 15, 0); // 2026-10-07 15:00 本地时间

  it.each([
    [null, ""],
    ["not-a-date", ""],
    [new Date(2026, 9, 6, 9, 0).toISOString(), "昨天"],
    [new Date(2026, 2, 3, 9, 0).toISOString(), "3月3日"],
  ])("%s -> %s", (iso, want) => {
    expect(formatMailDate(iso, now)).toBe(want);
  });

  it("今天只显示时间", () => {
    expect(formatMailDate(new Date(2026, 9, 7, 9, 5).toISOString(), now)).toMatch(/9.*05/);
  });

  it("往年显示完整日期", () => {
    expect(formatMailDate(new Date(2024, 0, 2).toISOString(), now)).toMatch(/2024/);
  });
});

describe("initials", () => {
  it.each([
    ["张三", "张"],
    ["alice", "A"],
    ["  bob ", "B"],
    ["😀 Smile", "😀"],
    ["", "?"],
  ])("%s -> %s", (name, want) => {
    expect(initials(name)).toBe(want);
  });
});

describe("hue", () => {
  it("同一输入稳定且在 0-359 之间", () => {
    expect(hue("a@qq.com")).toBe(hue("a@qq.com"));
    for (const s of ["", "a", "张三", "x@y.z"]) {
      expect(hue(s)).toBeGreaterThanOrEqual(0);
      expect(hue(s)).toBeLessThan(360);
    }
  });
});
