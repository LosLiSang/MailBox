import { describe, expect, it } from "vitest";
import { formatNewMailNotification } from "./notification";

describe("formatNewMailNotification", () => {
  it("addedCount <= 0 时返回空", () => {
    expect(formatNewMailNotification("Work", 0)).toEqual({ title: "", body: "" });
    expect(formatNewMailNotification("Work", -1)).toEqual({ title: "", body: "" });
  });

  it("单封邮件时以发件人和主题作为通知", () => {
    const res = formatNewMailNotification("Work", 1, {
      fromName: "Alice",
      fromAddress: "alice@example.com",
      subject: "明天会议安排",
    });
    expect(res).toEqual({
      title: "Alice - 新邮件",
      body: "明天会议安排",
    });
  });

  it("单封邮件无主题时显示（无主题）", () => {
    const res = formatNewMailNotification("Work", 1, {
      fromName: "Bob",
      fromAddress: "bob@example.com",
      subject: "",
    });
    expect(res).toEqual({
      title: "Bob - 新邮件",
      body: "（无主题）",
    });
  });

  it("多封邮件时显示汇总和最新一封主题", () => {
    const res = formatNewMailNotification("my@mail.com", 3, {
      fromName: "Charlie",
      subject: "周报总结",
    });
    expect(res).toEqual({
      title: "my@mail.com 收到 3 封新邮件",
      body: "最新：周报总结",
    });
  });

  it("多封邮件无最新邮件详情时提供默认提示", () => {
    const res = formatNewMailNotification("my@mail.com", 5);
    expect(res).toEqual({
      title: "my@mail.com 收到 5 封新邮件",
      body: "点击查看邮件详情",
    });
  });
});
