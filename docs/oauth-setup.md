# 配置 Gmail / Outlook 浏览器登录（OAuth）

Gmail 和 Outlook 的浏览器登录需要一个「OAuth 应用」来标识 MailBox。
这个应用由你自己在 Google / 微软后台免费创建，拿到 ID 后填进 MailBox 的「设置 → 高级」。

> 只想先用起来 Gmail，也可以跳过本文：开启两步验证后生成「应用专用密码」，
> 添加账号时选 Gmail → 「改用应用专用密码」。Outlook / Hotmail 个人邮箱已经不支持密码登录，只能走 OAuth。

国内网络访问 Google 需要代理：先在「设置 → 代理」填好并测试通过，添加账号时勾选「通过代理连接」。
浏览器授权页本身走的是系统浏览器，跟随浏览器的代理设置。

---

## Google（Gmail）

Google Cloud 控制台现在把 OAuth 配置放在 **Google Auth Platform** 下，分为品牌塑造、目标对象、数据访问、客户端几个页面。

1. 打开 [Google Cloud Console](https://console.cloud.google.com/)，顶部项目下拉框 → **新建项目**（名字随意，如 `MailBox`），创建后切换到这个项目。
2. （推荐）「API 和服务 → 库」搜索 **Gmail API** 并启用，这样下一步的范围列表里能直接找到 Gmail 权限。
3. 左侧菜单进入 **Google Auth Platform**，第一次使用点「开始」：
   - 应用名称：`MailBox`；用户支持邮箱、开发者联系邮箱：填你自己的
   - 目标对象（用户类型）：**外部（External）**
4. **目标对象（Audience）** → 测试用户 → **添加用户**，填入你要登录的 Gmail 地址。
   不加的话登录时会提示「已拒绝访问：未完成 Google 验证流程」。发布状态保持「测试」即可。
5. **数据访问（Data Access）** → 添加或移除范围：
   - 勾选 `https://mail.google.com/`（Gmail 完整权限，IMAP 需要这个）
   - 列表里找不到就在底部「手动添加范围」粘贴进去，点「添加到表格」→「更新」→ 保存
   - `openid`、`email` 属于基础范围，不加也能用
6. **客户端（Clients）** → **创建客户端**：
   - 应用类型：**桌面应用（Desktop app）**，名称随意
   - 不需要填重定向 URI，桌面应用默认允许 `http://127.0.0.1:<任意端口>`
   - 创建后复制 **客户端 ID**（`xxxx.apps.googleusercontent.com`）和 **客户端密钥**（`GOCSPX-...`）
   - ⚠️ 客户端密钥**只在创建时的弹窗里显示一次**，关掉后控制台只显示最后几位。
     错过了就进入这个客户端的详情页，点 **添加密钥（Add secret）** 生成一个新的，并立即复制
7. 回到 MailBox：「⚙ 设置 → 高级 → Google OAuth 应用」填入这两个值，点「保存」。
8. 「设置 → 代理」填好代理并测试 `imap.gmail.com:993` 通过，再「添加账号 → Gmail」，勾选「通过代理连接」，点「使用 Gmail 账号登录」。

注意：
- 桌面应用的「客户端密钥」按 Google 的说明并不保密，但仍然必须提交，所以两个都要填。
- 测试状态下 refresh token **7 天后过期**，届时在「设置 → 账号 → 编辑 → 重新登录」即可。
  想长期免登录，需要在权限请求页面把应用「发布」为正式版（个人使用时 Google 会显示「未经验证」警告，点「继续」即可）。

## Microsoft（Outlook / Hotmail / Live）

应用注册必须放在一个 Microsoft Entra「租户（目录）」里。公司/学校账号自带租户；
**个人账号（outlook.com / hotmail.com）默认没有**，直接打开应用注册会提示没有租户或无权访问，需要先做第 0 步。

0. （仅个人账号）创建一个免费租户：
   - 用你的微软账号登录 [Azure 门户](https://portal.azure.com)；如果提示注册 Azure 免费账户，按提示完成（需要手机验证，可能要求绑卡验证身份，不会扣费）
   - 搜索 **Microsoft Entra ID** → 「管理租户」→「＋ 创建」→ 选 **Microsoft Entra ID（员工）**
   - 组织名称随意，初始域名如 `yourname-dev`（得到 `yourname-dev.onmicrosoft.com`）
   - 创建后点右上角齿轮「目录 + 订阅」切换到这个新租户
1. **Microsoft Entra ID → 应用注册 → ＋ 新注册**：
   - 名称：`MailBox`
   - 受支持的帐户类型：**任何组织目录中的帐户和个人 Microsoft 帐户**
     （只用个人邮箱也可以选「仅个人 Microsoft 帐户」，但**不要选单租户**，否则个人邮箱登录会被拒绝）
   - 重定向 URI：平台选 **公共客户端/本机（移动和桌面）**，填 `http://localhost`
2. 注册后在「概述」页复制 **应用程序(客户端) ID**（形如 `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`）。
3. **身份验证 → 高级设置 → 允许公共客户端流**，设为 **是**，保存。
4. （可选）**API 权限 → 添加权限 → Microsoft Graph → 委托的权限**，勾选
   `IMAP.AccessAsUser.All`、`offline_access`、`openid`、`email`、`profile`。
   MailBox 登录时会动态申请这些权限，不加也能用；加上后授权页列出的权限更清楚。
5. 回到 MailBox：「设置 → 高级 → Microsoft OAuth 应用」填入客户端 ID，保存。

**不需要**客户端密码（client secret），也不要创建：桌面应用无法安全保存密码，MailBox 使用 PKCE。

个人 Outlook 邮箱还需要确认 IMAP 已开启：网页版 Outlook → 设置 → 邮件 → 转发和 IMAP → **允许设备和应用使用 IMAP**。

## 工作原理

1. MailBox 在本机随机端口启动一个临时监听（只接受 127.0.0.1 / ::1）
2. 用系统浏览器打开授权页，登录并同意后浏览器跳回 `http://localhost:<端口>/?code=...`
3. MailBox 用授权码 + PKCE 换取 access token 和 refresh token
4. 令牌保存在 Windows 凭据管理器（`MailBox` / `oauth:<邮箱>`），不写入任何文件
5. IMAP 使用 `AUTHENTICATE XOAUTH2` 登录；access token 过期前自动用 refresh token 续期
