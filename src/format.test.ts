import { describe, expect, it } from "vitest";
import {
  domainOf,
  extractAddress,
  formatFullDate,
  formatMailDate,
  formatSize,
  hue,
  initials,
  move,
  parseSenderList,
  syncSummary,
} from "./format";
import { detectProvider, providerById, unsupportedReason } from "./providers";

describe("detectProvider", () => {
  it.each([
    ["123@qq.com", "qq"],
    [" ABC@QQ.COM ", "qq"],
    ["a@foxmail.com", "qq"],
    ["a@gmail.com", "gmail"],
    ["a@hotmail.com", "outlook"],
    ["a@live.com", "outlook"],
    ["a@icloud.com", "icloud"],
    ["a@company.com", undefined],
    ["no-at-sign", undefined],
    ["", undefined],
  ])("%s -> %s", (email, id) => {
    expect(detectProvider(email)?.id).toBe(id);
  });

  it("未知 id 回退到自定义", () => {
    expect(providerById("nope").id).toBe("custom");
    expect(providerById("gmail").oauth).toBe("google");
  });

  it.each([
    ["a@163.com", true],
    ["a@126.com", true],
    ["a@qq.com", false],
    ["", false],
  ])("unsupported %s -> %s", (email, blocked) => {
    expect(Boolean(unsupportedReason(email))).toBe(blocked);
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

describe("formatFullDate", () => {
  it.each([
    [null, ""],
    ["bad", ""],
    [new Date(2025, 3, 1, 9, 5).toISOString(), "2025年4月1日 09:05"],
  ])("%s -> %s", (iso, want) => {
    expect(formatFullDate(iso)).toBe(want);
  });
});

describe("formatSize", () => {
  it.each([
    [0, "0 B"],
    [1023, "1023 B"],
    [1024, "1.0 KB"],
    [1536, "1.5 KB"],
    [20 * 1024, "20 KB"],
    [5 * 1024 * 1024, "5.0 MB"],
    [3 * 1024 ** 4, "3072 GB"],
  ])("%d -> %s", (bytes, want) => {
    expect(formatSize(bytes)).toBe(want);
  });
});

describe("syncSummary", () => {
  it.each([
    [{ added: 0, deleted: 0, updated: 0 }, "已是最新"],
    [{ added: 3, deleted: 0, updated: 0 }, "3 封新邮件"],
    [{ added: 1, deleted: 2, updated: 4 }, "1 封新邮件，2 封已删除，4 封状态更新"],
  ])("%o -> %s", (stats, want) => {
    expect(syncSummary(stats)).toBe(want);
  });
});

describe("parseSenderList", () => {
  it.each([
    ["", []],
    ["a@qq.com", ["a@qq.com"]],
    ["A@QQ.com\n@github.com, b@x.com；a@qq.com", ["a@qq.com", "@github.com", "b@x.com"]],
    ["  \n , ", []],
  ])("%j -> %j", (text, want) => {
    expect(parseSenderList(text)).toEqual(want);
  });
});

describe("extractAddress / domainOf", () => {
  it.each([
    ["张三 <ZS@QQ.com>", "zs@qq.com", "@qq.com"],
    ["noreply@github.com", "noreply@github.com", "@github.com"],
    ["  x@y.z  ", "x@y.z", "@y.z"],
    ["no address", "no address", ""],
  ])("%s", (from, address, domain) => {
    expect(extractAddress(from)).toBe(address);
    expect(domainOf(extractAddress(from))).toBe(domain);
  });
});

describe("move", () => {
  it.each([
    [[1, 2, 3], 0, 2, [2, 3, 1]],
    [[1, 2, 3], 2, 0, [3, 1, 2]],
    [[1, 2, 3], 1, 1, [1, 2, 3]],
    [[1, 2, 3], 0, 5, [1, 2, 3]],
    [[1, 2, 3], -1, 0, [1, 2, 3]],
  ])("%j %d->%d", (list, from, to, want) => {
    expect(move(list, from, to)).toEqual(want);
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

describe("theme", async () => {
  const { resolveDark, frameBackground } = await import("./theme");

  it.each([
    ["system", false, false],
    ["system", true, true],
    ["light", true, false],
    ["dark", false, true],
  ] as const)("resolveDark(%s, system=%s) -> %s", (theme, system, want) => {
    expect(resolveDark(theme, system)).toBe(want);
  });

  it.each([
    ["dark", false, "#181b20"],
    ["adaptive", false, "#181b20"],
    ["invert", false, "#1a1a1a"],
    ["light", true, "#ffffff"],
    [undefined, true, "#181b20"],
    [undefined, false, "#ffffff"],
  ] as const)("frameBackground(%s, appDark=%s) -> %s", (mode, dark, want) => {
    expect(frameBackground(mode, dark)).toBe(want);
  });
});
