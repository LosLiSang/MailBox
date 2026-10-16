import { describe, expect, it } from "vitest";
import {
  cleanCredential,
  googleClientIdWarning,
  googleClientSecretWarning,
  googleOAuthConfigured,
  isOAuthNotConfiguredError,
  microsoftClientIdWarning,
} from "./oauthConfig";

const GID = "123-abc.apps.googleusercontent.com";
const GSECRET = "GOCSPX-abcdef";
const MSID = "88600fe5-f9d3-4b15-974d-1c299963a96e";

describe("cleanCredential", () => {
  it.each([
    [`  ${GID}\n`, GID],
    ["\tGOCSPX-x ", "GOCSPX-x"],
    ["", ""],
  ])("%j -> %j", (input, expected) => expect(cleanCredential(input)).toBe(expected));
});

describe("format warnings", () => {
  it.each([
    [googleClientIdWarning, "", null],
    [googleClientIdWarning, ` ${GID} `, null],
    [googleClientIdWarning, GSECRET, "Client Secret"],
    [googleClientIdWarning, "123456", "googleusercontent"],
    [googleClientSecretWarning, "", null],
    [googleClientSecretWarning, `${GSECRET}\n`, null],
    [googleClientSecretWarning, GID, "Client ID"],
    [googleClientSecretWarning, "abc", "GOCSPX-"],
    [microsoftClientIdWarning, "", null],
    [microsoftClientIdWarning, ` ${MSID.toUpperCase()} `, null],
    [microsoftClientIdWarning, "not-a-guid", "GUID"],
  ])("%o(%j)", (fn, input, expected) => {
    const r = fn(input);
    if (expected === null) expect(r).toBeNull();
    else expect(r).toContain(expected);
  });
});

describe("googleOAuthConfigured", () => {
  it.each([
    [{ googleClientId: GID, googleClientSecret: GSECRET }, true],
    [{ googleClientId: GID, googleClientSecret: "  " }, false],
    [{ googleClientId: "", googleClientSecret: GSECRET }, false],
  ])("%j -> %s", (o, expected) => expect(googleOAuthConfigured(o)).toBe(expected));
});

describe("isOAuthNotConfiguredError", () => {
  it.each([
    ["还没有配置 Google OAuth 应用。请先到「设置 → 高级」填写 Client ID 和 Client Secret", true],
    ["登录被拒绝: access_denied", false],
  ])("%s -> %s", (msg, expected) => expect(isOAuthNotConfiguredError(msg)).toBe(expected));
});
