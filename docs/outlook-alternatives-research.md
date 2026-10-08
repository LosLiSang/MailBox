# Outlook 接入替代方案：GitHub 调研

## 目标与验证范围

针对个人 Outlook/Hotmail 邮箱、没有银行卡且当前没有可用 Entra 目录的情况，调查 MailBox 可采用的接入路线。本次核实项目 README、DavMail 官方 FAQ 和 MailBox 源码；未实际登录用户邮箱，不能保证某个第三方应用 ID 当前可用。未发现可用的后台代理工具，调研由当前会话完成。

## 1. Email OAuth 2.0 Proxy

项目：https://github.com/simonrob/email-oauth2-proxy

一手说明：https://github.com/simonrob/email-oauth2-proxy/blob/main/README.md

- 本地代理将客户端传统 IMAP/POP/SMTP 登录转换成上游 OAuth/XOAUTH2；项目明确支持 Windows、Outlook 和 Hotmail。
- 自托管仍需要 OAuth 客户端凭据，不是消除 OAuth。
- README 的 “OAuth 2.0 client credentials” 明确讨论复用已有开源邮件客户端凭据，并警告访问会归属于被复用的应用身份。个人 Outlook 注册受限时，README 指向复用既有 Client ID 的社区示例。
- 这证明存在社区实践，不证明应用所有者授权其他产品使用其注册、不证明微软认可该用途，也不保证当前可登录。不要把别人的 ID 当作 MailBox 自有身份公开发布。
- 默认客户端到代理为未加密的本地连接，上游连接为 TLS；也支持本地加密配置。只应绑定回环地址，不能把明文认证端口暴露到局域网或公网。
- 客户端提交给代理的密码不必是微软账号密码；它用于加密/解密代理缓存的 OAuth 令牌，应该使用独立且稳定的密码。令牌缓存必须受保护。

README 引用的社区示例（本次未成功读取正文，不作为独立验证证据）：
https://github.com/simonrob/email-oauth2-proxy/issues/297#issuecomment-2424200404

### 与 MailBox 的匹配

`src-tauri/src/imap_client.rs` 的 Session 固定为 TLS 流，`open()` 无条件进行 TLS 握手。因此不能直接把默认明文代理端口填入 MailBox 并期望可用。

可选实现：
1. 为代理配置可信的本地 TLS；或
2. 为 MailBox 增加严格限制在回环地址的本地代理传输模式，保留远程连接 TLS。

不要通过全局关闭证书校验解决本地 TLS。

## 2. DavMail

项目：https://github.com/mguessan/davmail

官方 FAQ：https://davmail.sourceforge.net/faq.html

- 仓库将其定位为 Exchange / Office 365 到 IMAP、POP、SMTP 等协议的网关。
- 官方 FAQ 说明 O365Interactive 交互授权以及项目提供的应用 Client ID，也允许替换为自有注册。
- 可以用项目自身的授权入口，而不一定由终端用户注册应用；组织策略仍可能阻止或要求管理员同意。
- 本次证据主要针对 Exchange / Office 365，不能据此保证个人 Outlook.com/Hotmail 可用，不能把它作为个人邮箱的首选已验证方案。
- 本地连接默认未加密，也需要解决与 MailBox 强制 TLS 的兼容。
- FAQ 中有 EWS 路线说明；本次未核实其当前个人邮箱兼容性、Graph 功能覆盖和 EWS 生命周期，不作保证。

## 3. 直接复用其他客户端的注册

Email OAuth 2.0 Proxy README 链接到 Thunderbird 等开源客户端的 OAuth 配置源码，表明该路线有社区先例。但它不是代理独有能力：MailBox 已有 PKCE/XOAUTH2，理论上也可配置现成的公共客户端 ID。

需要逐项确认：应用允许个人账号、允许当前回调 URI、支持当前 OAuth flow 和所需 scope，以及所有者允许的使用方式。项目当前请求 `IMAP.AccessAsUser.All offline_access openid email profile`；不能假设第三方注册与这套请求完全匹配。

本次访问 Thunderbird 配置源码遇到网络错误，因此不提供未经验证的具体 Client ID，也不声明已测试成功。

## 结论

- 个人实验：最值得评估的是现成公共客户端注册或 email-oauth2-proxy，但必须披露身份复用及失效风险；代理不能自动解决没有客户端注册的问题。
- 正式分发：使用 MailBox 自己的应用注册，或由可信协作者为项目注册，比借用其他客户端身份更可靠。
- 公司/学校 Exchange：DavMail 值得单独评估，不能保证适用于个人 Outlook。
- 无 OAuth 直接远程读写 Outlook 的密码方案，未在本次一手证据中找到。

## 本地发现

`docs/oauth-setup.md` 的微软权限章节把 IMAP 权限指向 Microsoft Graph，与当前代码使用的 Exchange/Outlook 资源 scope 不一致。本文仅记录，未修改原说明或实现。
