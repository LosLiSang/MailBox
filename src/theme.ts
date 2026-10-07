import { useEffect, useState } from "react";

export type ThemeSetting = "system" | "light" | "dark";

/** 设置里的主题 + 系统当前是否深色 -> 实际是否深色 */
export function resolveDark(theme: ThemeSetting, systemDark: boolean): boolean {
  return theme === "dark" || (theme === "system" && systemDark);
}

const query = "(prefers-color-scheme: dark)";

/** 实际生效的深色状态，系统主题切换时自动更新 */
export function useEffectiveDark(theme: ThemeSetting): boolean {
  const [systemDark, setSystemDark] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const onChange = () => setSystemDark(mq.matches);
    mq.addEventListener("change", onChange);
    return () => mq.removeEventListener("change", onChange);
  }, []);
  return resolveDark(theme, systemDark);
}

export type RenderMode = "light" | "dark" | "adaptive" | "invert";

/** iframe 加载前的底色，避免深色界面里闪一下白屏 */
export function frameBackground(mode: RenderMode | undefined, appDark: boolean): string {
  if (mode === "dark" || mode === "adaptive") return "#181b20";
  if (mode === "invert") return "#1a1a1a";
  if (mode === "light") return "#ffffff";
  return appDark ? "#181b20" : "#ffffff";
}
