import { useEffect, useState } from "react";
import { api, type EmailDarkMode, type Envelope, type MessageView } from "../api";
import { domainOf, formatFullDate, formatSize } from "../format";
import { frameBackground } from "../theme";
import { Avatar } from "./Avatar";

type Props = {
  email: string;
  mail: Envelope;
  onToggleSeen: (mail: Envelope) => void;
  onNotice: (text: string) => void;
  /** 把发件人（地址或 @域名）加入信任列表 */
  onTrustSender: (entry: string) => Promise<void>;
  /** 应用当前实际是否深色 */
  appDark: boolean;
  /** 设置变化时重新渲染（深色策略、信任列表等） */
  settingsVersion: string;
  /** 切换正文深浅色：写入设置，对之后打开的所有邮件生效 */
  onDarkModeChange: (mode: EmailDarkMode) => Promise<void>;
};

type ViewState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ok"; view: MessageView };

export function Reader(props: Props) {
  const { email, mail, onToggleSeen, onNotice, onTrustSender, appDark, settingsVersion, onDarkModeChange } = props;
  const [state, setState] = useState<ViewState>({ status: "loading" });
  const [allowRemote, setAllowRemote] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);
  const sender = mail.fromAddress.toLowerCase();

  useEffect(() => {
    let cancelled = false;
    setState((s) => (s.status === "ok" ? s : { status: "loading" }));
    api
      .getMessage(email, mail.uid, sender, { allowRemote, appDark })
      .then((view) => !cancelled && setState({ status: "ok", view }))
      .catch((err) => !cancelled && setState({ status: "error", message: String(err) }));
    return () => {
      cancelled = true;
    };
  }, [email, mail.uid, sender, allowRemote, appDark, reloadKey, settingsVersion]);

  async function attachmentAction(part: number, action: "open" | "save") {
    try {
      const path =
        action === "open"
          ? await api.openAttachment(email, mail.uid, part)
          : await api.saveAttachment(email, mail.uid, part);
      onNotice(action === "save" ? `已保存到 ${path}` : `已打开 ${path}`);
    } catch (err) {
      onNotice(`附件操作失败: ${err}`);
    }
  }

  async function trust(entry: string) {
    await onTrustSender(entry);
    setReloadKey((k) => k + 1);
  }

  if (state.status === "loading") {
    return (
      <div className="reader">
        <div className="placeholder">正在加载邮件…</div>
      </div>
    );
  }
  if (state.status === "error") {
    return (
      <div className="reader">
        <div className="placeholder error">
          <p>{state.message}</p>
          <button className="primary" onClick={() => setReloadKey((k) => k + 1)}>
            重试
          </button>
        </div>
      </div>
    );
  }

  const { view } = state;
  const senderName = mail.fromName || mail.fromAddress;
  const domain = domainOf(sender);
  // 邮件本身是深色设计时原样就是深色，不提供切换
  const nativeDark = view.contentKind === "dark";
  const bodyIsDark = nativeDark || view.renderMode !== "light";

  return (
    <div className="reader">
      <header className="reader-header">
        <div className="reader-title">
          <h2>{view.subject}</h2>
          <div className="reader-title-actions">
            {appDark && !nativeDark && (
              <button
                className="ghost small"
                onClick={() => onDarkModeChange(bodyIsDark ? "never" : "always")}
                title={
                  bodyIsDark
                    ? "之后所有邮件都用白底原样显示（可在设置中改回智能）"
                    : "之后所有邮件都用深色显示（可在设置中改回智能）"
                }
              >
                {bodyIsDark ? "☀ 原样" : "🌙 深色"}
              </button>
            )}
            <button className="ghost small" onClick={() => onToggleSeen(mail)}>
              {mail.seen ? "标为未读" : "标为已读"}
            </button>
          </div>
        </div>
        <div className="reader-meta">
          <Avatar name={senderName} seed={mail.fromAddress || senderName} size={40} />
          <div className="reader-addresses">
            <div className="reader-from">{view.from}</div>
            {view.to && <div className="muted">收件人：{view.to}</div>}
            {view.cc && <div className="muted">抄送：{view.cc}</div>}
          </div>
          <time className="muted">{formatFullDate(view.date)}</time>
        </div>
      </header>

      {view.hasRemoteContent && !view.remoteAllowed && (
        <div className="remote-banner">
          <span>🛡️ 已阻止远程图片，防止发件人追踪你是否打开了邮件</span>
          <span className="banner-actions">
            <button className="ghost small" onClick={() => setAllowRemote(true)}>
              显示图片
            </button>
            {sender && (
              <button className="ghost small" onClick={() => trust(sender)} title="以后自动显示这个发件人的图片">
                总是信任此发件人
              </button>
            )}
            {domain && (
              <button className="ghost small" onClick={() => trust(domain)} title={`信任所有 ${domain} 的邮件`}>
                信任 {domain}
              </button>
            )}
          </span>
        </div>
      )}

      {view.attachments.length > 0 && (
        <ul className="attachments">
          {view.attachments.map((a) => (
            <li key={a.part} title={a.mime}>
              <button className="attachment-name" onClick={() => attachmentAction(a.part, "open")}>
                📎 {a.name}
              </button>
              <span className="muted">{formatSize(a.size)}</span>
              <button className="link" onClick={() => attachmentAction(a.part, "save")}>
                保存
              </button>
            </li>
          ))}
        </ul>
      )}

      {/* 不加 allow-scripts / allow-same-origin：邮件 HTML 无法执行脚本、无法访问应用。
          allow-popups 让链接能发出新窗口请求，由 Rust 拦截后交给系统浏览器打开 */}
      <iframe
        key={`${mail.uid}-${view.remoteAllowed}-${view.renderMode}`}
        className="reader-frame"
        title="邮件正文"
        sandbox="allow-popups allow-popups-to-escape-sandbox"
        style={{
          background: frameBackground(view.renderMode, appDark),
          // iframe 的 color-scheme 决定邮件里 prefers-color-scheme 的结果。
          // 反色模式必须保持 light，否则邮件自己的深色样式生效后再被反色，又变回浅色
          colorScheme: view.renderMode === "dark" || view.renderMode === "adaptive" ? "dark" : "light",
        }}
        srcDoc={view.html}
      />
    </div>
  );
}
