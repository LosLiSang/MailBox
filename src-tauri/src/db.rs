//! 本地缓存：SQLite 存邮件头、原始邮件正文和每个文件夹的同步状态。

use crate::mail::Envelope;
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::Mutex};

/// 每次升级表结构时在末尾追加一条，按 user_version 依次执行
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE folder_state (
    account       TEXT NOT NULL,
    folder        TEXT NOT NULL,
    uid_validity  INTEGER NOT NULL,
    synced_at     TEXT,
    PRIMARY KEY (account, folder)
);
CREATE TABLE messages (
    account       TEXT NOT NULL,
    folder        TEXT NOT NULL,
    uid           INTEGER NOT NULL,
    subject       TEXT NOT NULL,
    from_name     TEXT NOT NULL,
    from_address  TEXT NOT NULL,
    date          TEXT,
    seen          INTEGER NOT NULL,
    PRIMARY KEY (account, folder, uid)
);
CREATE TABLE bodies (
    account       TEXT NOT NULL,
    folder        TEXT NOT NULL,
    uid           INTEGER NOT NULL,
    raw           BLOB NOT NULL,
    fetched_at    TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (account, folder, uid)
);
"#];

pub struct Db {
    conn: Mutex<Connection>,
}

fn err(e: impl std::fmt::Display) -> String {
    format!("数据库错误: {e}")
}

impl Db {
    pub fn open(path: &Path) -> Result<Self, String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        Self::init(Connection::open(path).map_err(err)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self, String> {
        Self::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> Result<Self, String> {
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")
            .map_err(err)?;
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(err)?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(version as usize) {
            conn.execute_batch(&format!("BEGIN; {sql} PRAGMA user_version = {}; COMMIT;", i + 1))
                .map_err(err)?;
        }
        Ok(Self { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&mut Connection) -> rusqlite::Result<T>) -> Result<T, String> {
        let mut conn = self.conn.lock().map_err(err)?;
        f(&mut conn).map_err(err)
    }

    pub fn uid_validity(&self, account: &str, folder: &str) -> Result<Option<u32>, String> {
        self.with(|c| {
            c.query_row(
                "SELECT uid_validity FROM folder_state WHERE account = ?1 AND folder = ?2",
                params![account, folder],
                |r| r.get(0),
            )
            .optional()
        })
    }

    /// UIDVALIDITY 变化说明服务器重建了文件夹，旧 UID 全部作废
    pub fn reset_folder(&self, account: &str, folder: &str, uid_validity: u32) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            tx.execute("DELETE FROM messages WHERE account = ?1 AND folder = ?2", params![account, folder])?;
            tx.execute("DELETE FROM bodies WHERE account = ?1 AND folder = ?2", params![account, folder])?;
            tx.execute(
                "INSERT INTO folder_state (account, folder, uid_validity) VALUES (?1, ?2, ?3)
                 ON CONFLICT (account, folder) DO UPDATE SET uid_validity = excluded.uid_validity, synced_at = NULL",
                params![account, folder, uid_validity],
            )?;
            tx.commit()
        })
    }

    /// 已缓存邮件的 (uid, seen)，按 uid 升序
    pub fn cached_flags(&self, account: &str, folder: &str) -> Result<Vec<(u32, bool)>, String> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT uid, seen FROM messages WHERE account = ?1 AND folder = ?2 ORDER BY uid",
            )?;
            let rows = stmt.query_map(params![account, folder], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect()
        })
    }

    /// 在一个事务里写入一次同步的全部结果
    pub fn apply_sync(
        &self,
        account: &str,
        folder: &str,
        deleted: &[u32],
        flag_changes: &[(u32, bool)],
        new_envelopes: &[Envelope],
    ) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            for uid in deleted {
                tx.execute(
                    "DELETE FROM messages WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                    params![account, folder, uid],
                )?;
                tx.execute(
                    "DELETE FROM bodies WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                    params![account, folder, uid],
                )?;
            }
            for (uid, seen) in flag_changes {
                tx.execute(
                    "UPDATE messages SET seen = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                    params![account, folder, uid, seen],
                )?;
            }
            for e in new_envelopes {
                tx.execute(
                    "INSERT OR REPLACE INTO messages
                     (account, folder, uid, subject, from_name, from_address, date, seen)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![account, folder, e.uid, e.subject, e.from_name, e.from_address, e.date, e.seen],
                )?;
            }
            tx.execute(
                "UPDATE folder_state SET synced_at = datetime('now') WHERE account = ?1 AND folder = ?2",
                params![account, folder],
            )?;
            tx.commit()
        })
    }

    /// 最新的在前
    pub fn list_envelopes(&self, account: &str, folder: &str, limit: u32) -> Result<Vec<Envelope>, String> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT uid, subject, from_name, from_address, date, seen FROM messages
                 WHERE account = ?1 AND folder = ?2 ORDER BY uid DESC LIMIT ?3",
            )?;
            let rows = stmt.query_map(params![account, folder, limit], |r| {
                Ok(Envelope {
                    uid: r.get(0)?,
                    subject: r.get(1)?,
                    from_name: r.get(2)?,
                    from_address: r.get(3)?,
                    date: r.get(4)?,
                    seen: r.get(5)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn set_seen(&self, account: &str, folder: &str, uid: u32, seen: bool) -> Result<(), String> {
        self.with(|c| {
            c.execute(
                "UPDATE messages SET seen = ?4 WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid, seen],
            )
            .map(|_| ())
        })
    }

    pub fn get_body(&self, account: &str, folder: &str, uid: u32) -> Result<Option<Vec<u8>>, String> {
        self.with(|c| {
            c.query_row(
                "SELECT raw FROM bodies WHERE account = ?1 AND folder = ?2 AND uid = ?3",
                params![account, folder, uid],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn put_body(&self, account: &str, folder: &str, uid: u32, raw: &[u8]) -> Result<(), String> {
        self.with(|c| {
            c.execute(
                "INSERT OR REPLACE INTO bodies (account, folder, uid, raw) VALUES (?1, ?2, ?3, ?4)",
                params![account, folder, uid, raw],
            )
            .map(|_| ())
        })
    }

    pub fn delete_account(&self, account: &str) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            for table in ["messages", "bodies", "folder_state"] {
                tx.execute(&format!("DELETE FROM {table} WHERE account = ?1"), params![account])?;
            }
            tx.commit()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "a@qq.com";
    const F: &str = "INBOX";

    fn env(uid: u32, seen: bool) -> Envelope {
        Envelope {
            uid,
            subject: format!("s{uid}"),
            from_name: "n".into(),
            from_address: "x@y.z".into(),
            date: Some("2025-01-01T00:00:00+08:00".into()),
            seen,
        }
    }

    #[test]
    fn sync_lifecycle() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(db.uid_validity(A, F).unwrap(), None);

        db.reset_folder(A, F, 100).unwrap();
        assert_eq!(db.uid_validity(A, F).unwrap(), Some(100));

        db.apply_sync(A, F, &[], &[], &[env(1, true), env(2, false), env(3, false)]).unwrap();
        db.put_body(A, F, 2, b"raw2").unwrap();
        assert_eq!(db.cached_flags(A, F).unwrap(), [(1, true), (2, false), (3, false)]);

        // 删除 2（连带正文）、3 标记已读、新增 4
        db.apply_sync(A, F, &[2], &[(3, true)], &[env(4, false)]).unwrap();
        assert_eq!(db.cached_flags(A, F).unwrap(), [(1, true), (3, true), (4, false)]);
        assert_eq!(db.get_body(A, F, 2).unwrap(), None);

        let list = db.list_envelopes(A, F, 2).unwrap();
        assert_eq!(list.iter().map(|e| e.uid).collect::<Vec<_>>(), [4, 3]);
        assert_eq!(list[0], env(4, false));

        db.set_seen(A, F, 4, true).unwrap();
        assert_eq!(db.cached_flags(A, F).unwrap().last(), Some(&(4, true)));

        // UIDVALIDITY 变化 -> 清空
        db.put_body(A, F, 1, b"raw1").unwrap();
        db.reset_folder(A, F, 200).unwrap();
        assert!(db.cached_flags(A, F).unwrap().is_empty());
        assert_eq!(db.get_body(A, F, 1).unwrap(), None);
        assert_eq!(db.uid_validity(A, F).unwrap(), Some(200));
    }

    #[test]
    fn accounts_are_isolated_and_deletable() {
        let db = Db::open_in_memory().unwrap();
        for acc in [A, "b@qq.com"] {
            db.reset_folder(acc, F, 1).unwrap();
            db.apply_sync(acc, F, &[], &[], &[env(1, false)]).unwrap();
            db.put_body(acc, F, 1, b"raw").unwrap();
        }
        db.delete_account(A).unwrap();
        assert!(db.cached_flags(A, F).unwrap().is_empty());
        assert_eq!(db.uid_validity(A, F).unwrap(), None);
        assert_eq!(db.cached_flags("b@qq.com", F).unwrap(), [(1, false)]);
        assert!(db.get_body("b@qq.com", F, 1).unwrap().is_some());
    }

    #[test]
    fn reopening_file_keeps_data_and_skips_migrations() {
        let dir = std::env::temp_dir().join(format!("mailbox-test-{}", std::process::id()));
        let path = dir.join("t.db");
        {
            let db = Db::open(&path).unwrap();
            db.reset_folder(A, F, 1).unwrap();
            db.apply_sync(A, F, &[], &[], &[env(9, true)]).unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.cached_flags(A, F).unwrap(), [(9, true)]);
        drop(db);
        std::fs::remove_dir_all(dir).ok();
    }
}
