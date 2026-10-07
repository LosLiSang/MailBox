import { useState } from "react";
import type { Category, Envelope } from "../api";
import { formatMailDate } from "../format";
import { Avatar } from "./Avatar";

type Props = {
  mails: Envelope[];
  selectedUid: number | null;
  onSelect: (mail: Envelope) => void;
  categories: Category[];
  /** 当前视图：null 收件箱，-1 全部，其他为分类 id */
  currentView: number | null;
  onMove: (mail: Envelope, view: number | null) => void;
};

/** 右键 / 悬停菜单里可去的目标：收件箱（当前不在时）+ 其他分类 */
function moveTargets(mail: Envelope, categories: Category[], currentView: number | null) {
  const targets: { label: string; view: number | null; color?: string }[] = [];
  if (mail.categoryId !== null || currentView !== null) {
    targets.push({ label: "移回收件箱", view: null });
  }
  for (const c of categories) {
    if (c.id !== mail.categoryId && c.id !== currentView) {
      targets.push({ label: c.name, view: c.id, color: c.color });
    }
  }
  return targets;
}

export function MailList({ mails, selectedUid, onSelect, categories, currentView, onMove }: Props) {
  const [menuFor, setMenuFor] = useState<number | null>(null);

  function closeMenu() {
    setMenuFor(null);
  }

  return (
    <ul className="mail-list" role="listbox" onClick={closeMenu} onContextMenu={closeMenu}>
      {mails.map((m) => {
        const sender = m.fromName || m.fromAddress || "(未知发件人)";
        const category = categories.find((c) => c.id === m.categoryId);
        const classes = [m.seen ? "" : "unread", m.uid === selectedUid ? "selected" : ""].join(" ");
        const targets = moveTargets(m, categories, currentView);
        return (
          <li
            key={m.uid}
            className={classes}
            role="option"
            aria-selected={m.uid === selectedUid}
            onClick={(e) => {
              e.stopPropagation();
              closeMenu();
              onSelect(m);
            }}
            onContextMenu={(e) => {
              e.preventDefault();
              e.stopPropagation();
              setMenuFor(menuFor === m.uid ? null : m.uid);
            }}
            draggable
            onDragStart={(e) => e.dataTransfer.setData("text/mail-uid", String(m.uid))}
          >
            <Avatar name={sender} seed={m.fromAddress || sender} />
            <div className="mail-body">
              <div className="mail-row">
                <span className="from" title={m.fromAddress}>
                  {sender}
                </span>
                <span className="mail-row-right">
                  {category && (
                    <span className="cat-chip" style={{ background: category.color }}>
                      {category.name}
                    </span>
                  )}
                  <time className="date" title={m.date ?? undefined}>
                    {formatMailDate(m.date)}
                  </time>
                </span>
              </div>
              <div className="subject" title={m.subject}>
                {!m.seen && <span className="dot" />}
                {m.subject}
              </div>
            </div>

            {menuFor === m.uid && targets.length > 0 && (
              <div className="move-menu" onClick={(e) => e.stopPropagation()}>
                <div className="move-menu-title">移动到</div>
                {targets.map((t) => (
                  <button
                    key={t.label}
                    className="move-menu-item"
                    onClick={() => {
                      onMove(m, t.view);
                      closeMenu();
                    }}
                  >
                    {t.color && <span className="view-dot" style={{ background: t.color }} />}
                    {t.label}
                  </button>
                ))}
              </div>
            )}
          </li>
        );
      })}
    </ul>
  );
}

/** 侧栏 / 分类标题上的拖放目标行为由调用方处理；这里只导出判断函数 */
export function mailUidFromDrop(e: React.DragEvent): number | null {
  const uid = e.dataTransfer.getData("text/mail-uid");
  return uid ? Number(uid) : null;
}
