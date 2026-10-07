import type { Envelope } from "../api";
import { formatMailDate } from "../format";
import { Avatar } from "./Avatar";

export function MailList({ mails }: { mails: Envelope[] }) {
  return (
    <ul className="mail-list">
      {mails.map((m) => {
        const sender = m.fromName || m.fromAddress || "(未知发件人)";
        return (
          <li key={m.uid} className={m.seen ? "" : "unread"}>
            <Avatar name={sender} seed={m.fromAddress || sender} />
            <div className="mail-body">
              <div className="mail-row">
                <span className="from" title={m.fromAddress}>
                  {sender}
                </span>
                <time className="date" title={m.date ?? undefined}>
                  {formatMailDate(m.date)}
                </time>
              </div>
              <div className="subject" title={m.subject}>
                {!m.seen && <span className="dot" />}
                {m.subject}
              </div>
            </div>
          </li>
        );
      })}
    </ul>
  );
}
