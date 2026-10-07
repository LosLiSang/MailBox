import { useEffect, useState } from "react";
import { api, type Envelope, type MessageView } from "../api";
import { formatFullDate, formatSize } from "../format";
import { Avatar } from "./Avatar";

type Props = {
  email: string;
  mail: Envelope;
  onToggleSeen: (mail: Envelope) => void;
  onNotice: (text: string) => void;
};

type ViewState =
  | { status: "loading" }
  | { status: "error"; message: string }
  | { status: "ok"; view: MessageView };

export function Reader({ email, mail, onToggleSeen, onNotice }: Props) {
  const [state, setState] = useState<ViewState>({ status: "loading" });
  const [allowRemote, setAllowRemote] = useState(false);
  const [reloadKey, setReloadKey] = useState(0);

  // 切换邮件时重置远程图片开关
  useEffect(() => setAllowRemote(false), [email, mail.uid]);

  useEffect(() => {
    let cancelled = false;
    setState((s) => (s.status === "ok" && allowRemote ? s : { status: "loading" }));
    api
      .getMessage(email, mail.uid, allowRemote)
      .then((view) => !cancelled && setState({ status: "ok", view }))
      .catch((err) => !cancelled && setState({ status: "error", message: String(err) }));
    return () => {
      cancelled = true;
    };
  }, [email, mail.uid, allowRemote, reloadKey]);

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
  const sender = mail.fromName || mail.fromAddress;

  return (
    <div className="reader">
      <header className="reader-header">
        <div className="reader-title">
          <h2>{view.subject}</h2>
          <button className="ghost small" onClick={() => onToggleSeen(mail)}>
            {mail.seen ? "标为未读" : "标为已读"}
          </button>
        </div>
        <div className="reader-meta">
          <Avatar name={sender} seed={mail.fromAddress || sender} size={40} />
          <div className="reader-addresses">
            <div className="reader-from">{view.from}</div>
            {view.to && <div className="muted">收件人：{view.to}</div>}
            {view.cc && <div className="muted">抄送：{view.cc}</div>}
          </div>
          <time className="muted">{formatFullDate(view.date)}</time>
        </div>
      </header>

      {view.hasRemoteContent && !allowRemote && (
        <div className="remote-banner">
          <span>🛡️ 已阻止远程图片，防止发件人追踪你是否打开了邮件</span>
          <button className="ghost small" onClick={() => setAllowRemote(true)}>
            显示图片
          </button>
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
        key={`${mail.uid}-${allowRemote}`}
        className="reader-frame"
        title="邮件正文"
        sandbox="allow-popups allow-popups-to-escape-sandbox"
        srcDoc={view.html}
      />
    </div>
  );
}
