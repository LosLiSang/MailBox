/** OAuth 应用配置：教程链接、控制台入口与粘贴值校验（纯函数，无依赖） */

export const OAUTH_GUIDE_URL = "https://github.com/LosLiSang/MailBox/blob/main/docs/oauth-setup.md";
/** GitHub 标题锚点：「### 自定义应用（高级）」去掉全角括号 */
export const MICROSOFT_CUSTOM_GUIDE_URL = `${OAUTH_GUIDE_URL}#自定义应用高级`;

/** 与 docs/oauth-setup.md 中「Google（Gmail）」的步骤顺序、页面名称一一对应 */
export const GOOGLE_SETUP_STEPS: { title: string; detail: string; button: string; url: string }[] = [
  {
    title: "新建项目",
    detail: "项目名称随意（如 MailBox），创建后在顶部切换到这个项目。",
    button: "新建项目",
    url: "https://console.cloud.google.com/projectcreate",
  },
  {
    title: "启用 Gmail API",
    detail: "点「启用」。",
    button: "打开 Gmail API",
    url: "https://console.cloud.google.com/apis/library/gmail.googleapis.com",
  },
  {
    title: "Google Auth Platform → 开始配置",
    detail: "第一次进入点「开始」：应用名称填 MailBox，邮箱都填自己的，目标对象选「外部」，最后点「创建」。",
    button: "打开 Google Auth Platform",
    url: "https://console.cloud.google.com/auth/overview",
  },
  {
    title: "目标对象 → 测试用户 → 添加用户",
    detail: "发布状态保持「测试」，在「测试用户」里添加要登录的 Gmail 地址。不加会提示「已拒绝访问」。",
    button: "打开目标对象",
    url: "https://console.cloud.google.com/auth/audience",
  },
  {
    title: "数据访问 → 添加或移除范围",
    detail: "勾选（或手动添加）https://mail.google.com/，点「更新」，再点页面底部「保存」。",
    button: "打开数据访问",
    url: "https://console.cloud.google.com/auth/scopes",
  },
  {
    title: "客户端 → 创建客户端",
    detail: "应用类型选「桌面应用」。弹窗里的客户端密钥只显示一次，立即复制到下方。",
    button: "打开客户端",
    url: "https://console.cloud.google.com/auth/clients",
  },
];

/** 粘贴的值常带首尾空格或换行 */
export const cleanCredential = (value: string) => value.trim();

const GUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/** 返回提示文字；空值或格式正常返回 null。只提示，不阻止保存 */
export function googleClientIdWarning(id: string): string | null {
  const v = cleanCredential(id);
  if (!v || v.endsWith(".apps.googleusercontent.com")) return null;
  if (v.startsWith("GOCSPX-")) return "这看起来是客户端密钥，请填到下面的 Client Secret";
  return "Google Client ID 通常以 .apps.googleusercontent.com 结尾，请确认复制的是「客户端 ID」";
}

export function googleClientSecretWarning(secret: string): string | null {
  const v = cleanCredential(secret);
  if (!v || v.startsWith("GOCSPX-")) return null;
  if (v.endsWith(".apps.googleusercontent.com")) return "这看起来是客户端 ID，请填到上面的 Client ID";
  return "Google 客户端密钥通常以 GOCSPX- 开头，请确认复制的是「客户端密钥」";
}

export function microsoftClientIdWarning(id: string): string | null {
  const v = cleanCredential(id);
  if (!v || GUID.test(v)) return null;
  return "Microsoft Application (client) ID 是 GUID 格式，如 88600fe5-f9d3-4b15-974d-1c299963a96e";
}

export function googleOAuthConfigured(o: { googleClientId: string; googleClientSecret: string }): boolean {
  return Boolean(cleanCredential(o.googleClientId) && cleanCredential(o.googleClientSecret));
}

/** 后端「还没有配置 xx OAuth 应用」的报错，用于在对话框里给出跳转按钮 */
export const isOAuthNotConfiguredError = (message: string) => message.includes("还没有配置") && message.includes("OAuth 应用");
