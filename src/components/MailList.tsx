import type { Envelope } from "../api";
import { formatMailDate } from "../format";
import { Avatar } from "./Avatar";

type Props = {
  mails: Envelope[];
  selectedUid: number | null;
  onSelect: (mail: Envelope) => void;
};

export function MailList({ mails, selectedUid, onSelect }: Props) {
  return (
    <ul className="mail-list" role="listbox">
      {mails.map((m) => {
        const sender = m.fromName || m.fromAddress || "(未知发件人)";
        const classes = [m.seen ? "" : "unread", m.uid === selectedUid ? "selected" : ""].join(" ");
        return (
          <li
            key={m.uid}
            className={classes}
            role="option"
            aria-selected={m.uid === selectedUid}
            onClick={() => onSelect(m)}
          >
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
