//! 本地缓存：SQLite 存邮件头、原始邮件正文和每个文件夹的同步状态。

use crate::mail::Envelope;
use rusqlite::{params, Connection, OptionalExtension};
use std::{path::Path, sync::Mutex};

/// 列表视图：收件箱（未归类）、某个分类、或全部
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum View {
    Inbox,
    Category(i64),
    All,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Category {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub count: i64,
}

/// #rgb 或 #rrggbb
fn is_valid_color(c: &str) -> bool {
    let hex = c.strip_prefix('#').unwrap_or_default();
    (hex.len() == 3 || hex.len() == 6) && hex.chars().all(|ch| ch.is_ascii_hexdigit())
}

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
"#, r#"
-- 本地分类：每个账号一套，邮件移进分类后不再出现在收件箱里
CREATE TABLE categories (
    account    TEXT NOT NULL,
    id         INTEGER NOT NULL,
    name       TEXT NOT NULL,
    color      TEXT NOT NULL,
    position   INTEGER NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY (account, id)
);
CREATE TABLE sender_rules (
    id          INTEGER NOT NULL,
    account     TEXT NOT NULL,
    pattern     TEXT NOT NULL,
    category_id INTEGER NOT NULL,
    position    INTEGER NOT NULL,
    PRIMARY KEY (account, id),
    FOREIGN KEY (account, category_id) REFERENCES categories (account, id) ON DELETE CASCADE
);
ALTER TABLE messages ADD COLUMN category_id INTEGER;
CREATE INDEX idx_messages_category ON messages (account, category_id);
CREATE INDEX idx_rules_account ON sender_rules (account);
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

    /// 最新的在前。view: None 收件箱（未归类），Some(id) 指定分类
    pub fn list_envelopes(
        &self,
        account: &str,
        folder: &str,
        limit: u32,
        view: View,
    ) -> Result<Vec<Envelope>, String> {
        self.with(|c| {
            let (sql, param): (&str, Vec<rusqlite::types::Value>) = match view {
                View::Inbox => (
                    "SELECT uid, subject, from_name, from_address, date, seen, category_id FROM messages
                     WHERE account = ?1 AND folder = ?2 AND category_id IS NULL
                     ORDER BY uid DESC LIMIT ?3",
                    vec![account.to_owned().into(), folder.to_owned().into(), limit.into()],
                ),
                View::Category(id) => (
                    "SELECT uid, subject, from_name, from_address, date, seen, category_id FROM messages
                     WHERE account = ?1 AND folder = ?2 AND category_id = ?3
                     ORDER BY uid DESC LIMIT ?4",
                    vec![account.to_owned().into(), folder.to_owned().into(), id.into(), limit.into()],
                ),
                View::All => (
                    "SELECT uid, subject, from_name, from_address, date, seen, category_id FROM messages
                     WHERE account = ?1 AND folder = ?2
                     ORDER BY uid DESC LIMIT ?3",
                    vec![account.to_owned().into(), folder.to_owned().into(), limit.into()],
                ),
            };
            let mut stmt = c.prepare(sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(param), |r| {
                Ok(Envelope {
                    uid: r.get(0)?,
                    subject: r.get(1)?,
                    from_name: r.get(2)?,
                    from_address: r.get(3)?,
                    date: r.get(4)?,
                    seen: r.get(5)?,
                    category_id: r.get(6)?,
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

    /// (邮件头数, 已缓存正文数, 正文总字节数)
    pub fn stats(&self, account: &str) -> Result<(u64, u64, u64), String> {
        self.with(|c| {
            let headers: i64 =
                c.query_row("SELECT count(*) FROM messages WHERE account = ?1", params![account], |r| r.get(0))?;
            let (bodies, bytes): (i64, i64) = c.query_row(
                "SELECT count(*), coalesce(sum(length(raw)), 0) FROM bodies WHERE account = ?1",
                params![account],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok((headers as u64, bodies as u64, bytes as u64))
        })
    }

    /// 只删正文，邮件列表保留；再次打开时重新下载
    pub fn clear_bodies(&self) -> Result<(), String> {
        self.with(|c| c.execute("DELETE FROM bodies", []).map(|_| ()))?;
        self.vacuum()
    }

    /// 清空全部缓存，下次同步从头拉取
    pub fn clear_all(&self) -> Result<(), String> {
        self.with(|c| c.execute_batch("DELETE FROM bodies; DELETE FROM messages; DELETE FROM folder_state;"))?;
        self.vacuum()
    }

    fn vacuum(&self) -> Result<(), String> {
        self.with(|c| c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE); VACUUM;"))
    }

    pub fn delete_account(&self, account: &str) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            for table in ["messages", "bodies", "folder_state"] {
                tx.execute(&format!("DELETE FROM {table} WHERE account = ?1"), params![account])?;
            }
            // 分类和规则跟着账号一起删（sender_rules 由外键级联）
            tx.execute("DELETE FROM categories WHERE account = ?1", params![account])?;
            tx.commit()
        })
    }

    // ---------- 分类与规则 ----------

    pub fn default_categories(&self, account: &str) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            for (i, (name, color)) in [
                ("重要", "#d29922"),
                ("通知", "#539bf5"),
                ("不需要在意", "#6e7781"),
                ("垃圾邮件", "#f47067"),
            ]
            .into_iter()
            .enumerate()
            {
                let exists: bool = tx
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM categories WHERE account = ?1 AND name = ?2)",
                        params![account, name],
                        |r| r.get(0),
                    )?;
                if !exists {
                    tx.execute(
                        "INSERT INTO categories (account, id, name, color, position)
                         VALUES (?1, (SELECT coalesce(max(id), 0) + 1 FROM categories WHERE account = ?1), ?2, ?3, ?4)",
                        params![account, name, color, i as i64],
                    )?;
                }
            }
            tx.commit()
        })
    }

    pub fn list_categories(&self, account: &str) -> Result<Vec<Category>, String> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT cat.id, cat.name, cat.color, count(m.uid)
                 FROM categories cat
                 LEFT JOIN messages m ON m.account = cat.account AND m.category_id = cat.id
                 WHERE cat.account = ?1
                 GROUP BY cat.id, cat.name, cat.color
                 ORDER BY cat.position, cat.id",
            )?;
            let rows = stmt.query_map(params![account], |r| {
                Ok(Category { id: r.get(0)?, name: r.get(1)?, color: r.get(2)?, count: r.get(3)? })
            })?;
            rows.collect()
        })
    }

    pub fn create_category(&self, account: &str, name: &str, color: &str) -> Result<Category, String> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 20 {
            return Err("分类名称需为 1–20 个字符".into());
        }
        if !is_valid_color(color) {
            return Err("颜色格式不正确".into());
        }
        self.with(|c| {
            let exists: bool = c
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM categories WHERE account = ?1 AND name = ?2)",
                    params![account, name],
                    |r| r.get(0),
                )
                .unwrap_or(false);
            if exists {
                return Err(rusqlite::Error::InvalidParameterName(format!("已存在名为「{name}」的分类")));
            }
            c.execute(
                "INSERT INTO categories (account, id, name, color, position)
                 VALUES (?1, (SELECT coalesce(max(id), 0) + 1 FROM categories WHERE account = ?1), ?2, ?3,
                         (SELECT coalesce(max(position), 0) + 1 FROM categories WHERE account = ?1))",
                params![account, name, color.to_lowercase()],
            )?;
            Ok(Category {
                id: c.query_row(
                    "SELECT id FROM categories WHERE account = ?1 AND name = ?2",
                    params![account, name],
                    |r| r.get(0),
                )?,
                name: name.into(),
                color: color.to_lowercase(),
                count: 0,
            })
        })
    }

    /// 重命名 / 改颜色；None 表示不修改
    pub fn update_category(&self, account: &str, id: i64, name: Option<&str>, color: Option<&str>) -> Result<(), String> {
        let name = name.map(str::trim);
        if let Some(n) = name {
            if n.is_empty() || n.chars().count() > 20 {
                return Err("分类名称需为 1–20 个字符".into());
            }
        }
        if let Some(c) = color {
            if !is_valid_color(c) {
                return Err("颜色格式不正确".into());
            }
        }
        let n = self.with(|c| {
            c.execute(
                "UPDATE categories SET
                    name = coalesce(?3, name),
                    color = coalesce(?4, color)
                 WHERE account = ?1 AND id = ?2",
                params![account, id, name, color.map(str::to_lowercase)],
            )
        })?;
        (n == 1).then_some(()).ok_or_else(|| "分类不存在".to_string())
    }

    /// 删除分类：里面的邮件回到收件箱，相关规则一并删除。返回是否真的删除了
    pub fn delete_category(&self, account: &str, id: i64) -> Result<bool, String> {
        let n = self.with(|c| {
            let tx = c.transaction()?;
            tx.execute(
                "UPDATE messages SET category_id = NULL WHERE account = ?1 AND category_id = ?2",
                params![account, id],
            )?;
            tx.execute("DELETE FROM sender_rules WHERE account = ?1 AND category_id = ?2", params![account, id])?;
            let n = tx.execute("DELETE FROM categories WHERE account = ?1 AND id = ?2", params![account, id])?;
            tx.commit()?;
            Ok::<_, rusqlite::Error>(n)
        })?;
        Ok(n == 1)
    }

    pub fn reorder_categories(&self, account: &str, ids: &[i64]) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            for (position, id) in ids.iter().enumerate() {
                tx.execute(
                    "UPDATE categories SET position = ?3 WHERE account = ?1 AND id = ?2",
                    params![account, id, position as i64],
                )?;
            }
            tx.commit()
        })
    }

    /// 移动邮件到分类；category_id 为 None 表示移回收件箱
    pub fn move_messages(&self, account: &str, uids: &[u32], category_id: Option<i64>) -> Result<usize, String> {
        self.with(|c| {
            let tx = c.transaction()?;
            let mut n = 0;
            for uid in uids {
                n += tx.execute(
                    "UPDATE messages SET category_id = ?3 WHERE account = ?1 AND folder = 'INBOX' AND uid = ?2",
                    params![account, uid, category_id],
                )?;
            }
            tx.commit()?;
            Ok(n)
        })
    }

    pub fn list_rules(&self, account: &str) -> Result<Vec<crate::rules::Rule>, String> {
        self.with(|c| {
            let mut stmt = c.prepare(
                "SELECT id, pattern, category_id FROM sender_rules WHERE account = ?1 ORDER BY position, id",
            )?;
            let rows = stmt.query_map(params![account], |r| {
                Ok(crate::rules::Rule { id: r.get(0)?, pattern: r.get(1)?, category_id: r.get(2)? })
            })?;
            rows.collect()
        })
    }

    /// 返回规则 id 和实际归类到的邮件数；apply_existing 同时把已缓存的匹配邮件归类
    pub fn add_rule(&self, account: &str, pattern: &str, category_id: i64, apply_existing: bool) -> Result<(i64, usize), String> {
        let pattern = crate::rules::normalize_pattern(pattern).ok_or("规则格式不正确：可用 @域名、完整邮箱或至少两个字的关键词")?;
        self.with(|c| {
            let tx = c.transaction()?;
            let category_exists: bool = tx
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM categories WHERE account = ?1 AND id = ?2)",
                    params![account, category_id],
                    |r| r.get(0),
                )?;
            if !category_exists {
                return Err(rusqlite::Error::InvalidParameterName("分类不存在".into()));
            }
            tx.execute(
                "INSERT INTO sender_rules (account, id, pattern, category_id, position)
                 VALUES (?1, (SELECT coalesce(max(id), 0) + 1 FROM sender_rules WHERE account = ?1), ?2, ?3,
                         (SELECT coalesce(max(position), 0) + 1 FROM sender_rules WHERE account = ?1))",
                params![account, pattern, category_id],
            )?;
            let id = tx.last_insert_rowid();
            let mut moved = 0;
            if apply_existing {
                // 已缓存的邮件按规则重新归类：先清空这些发件人的手动分类，再逐封匹配
                let mut stmt = tx.prepare(
                    "SELECT uid, from_address, from_name, category_id FROM messages
                     WHERE account = ?1 AND folder = 'INBOX'",
                )?;
                let rows: Vec<(u32, String, String, Option<i64>)> = stmt
                    .query_map(params![account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
                    .collect::<Result<_, _>>()?;
                drop(stmt);
                let rules: Vec<crate::rules::Rule> = {
                    let mut stmt = tx.prepare(
                        "SELECT id, pattern, category_id FROM sender_rules WHERE account = ?1 ORDER BY position, id",
                    )?;
                    let rows = stmt.query_map(params![account], |r| {
                        Ok(crate::rules::Rule { id: r.get(0)?, pattern: r.get(1)?, category_id: r.get(2)? })
                    })?;
                    rows.collect::<Result<_, _>>()?
                };
                for (uid, addr, name, _) in rows {
                    // 发件人规则覆盖手动分类：用户明确表达了「这个发件人的邮件都归到某分类」
                    if let Some(cat) = crate::rules::match_sender(&rules, &addr, &name) {
                        tx.execute(
                            "UPDATE messages SET category_id = ?3 WHERE account = ?1 AND folder = 'INBOX' AND uid = ?2",
                            params![account, uid, cat],
                        )?;
                        moved += 1;
                    }
                }
            }
            tx.commit()?;
            Ok((id, moved))
        })
    }

    pub fn delete_rule(&self, account: &str, id: i64) -> Result<(), String> {
        let n = self.with(|c| c.execute("DELETE FROM sender_rules WHERE account = ?1 AND id = ?2", params![account, id]))?;
        (n == 1).then_some(()).ok_or_else(|| "规则不存在".to_string())
    }

    /// 规则的排序（位置越小优先级越高）
    #[allow(dead_code)]
    pub fn reorder_rules(&self, account: &str, ids: &[i64]) -> Result<(), String> {
        self.with(|c| {
            let tx = c.transaction()?;
            for (position, id) in ids.iter().enumerate() {
                tx.execute(
                    "UPDATE sender_rules SET position = ?3 WHERE account = ?1 AND id = ?2",
                    params![account, id, position as i64],
                )?;
            }
            tx.commit()
        })
    }

    /// 默认分类 id：新邮件没有规则命中时，如果存在名为「不需要在意」的分类则自动归入。
    /// 目前只提供查询，是否默认归类由调用方决定
    pub fn default_category(&self, account: &str) -> Result<Option<i64>, String> {
        self.with(|c| {
            c.query_row(
                "SELECT id FROM categories WHERE account = ?1 AND name = '不需要在意'",
                params![account],
                |r| r.get(0),
            )
            .optional()
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
            category_id: None,
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

        let list = db.list_envelopes(A, F, 2, View::Inbox).unwrap();
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
    fn categories_and_rules_lifecycle() {
        let db = Db::open_in_memory().unwrap();
        db.apply_sync(A, F, &[], &[], &[env(1, false), env(2, false), env(3, false)]).unwrap();

        // 默认分类幂等
        db.default_categories(A).unwrap();
        let cats = db.list_categories(A).unwrap();
        db.default_categories(A).unwrap();
        assert_eq!(db.list_categories(A).unwrap().len(), cats.len());
        assert!(cats.iter().any(|c| c.name == "重要"));

        // 移入分类后从收件箱消失，出现在分类里
        let junk = cats.iter().find(|c| c.name == "垃圾邮件").unwrap();
        assert_eq!(db.move_messages(A, &[2], Some(junk.id)).unwrap(), 1);
        assert_eq!(db.list_envelopes(A, F, 100, View::Inbox).unwrap().len(), 2);
        let in_junk = db.list_envelopes(A, F, 100, View::Category(junk.id)).unwrap();
        assert_eq!(in_junk.iter().map(|e| e.uid).collect::<Vec<_>>(), [2]);
        assert_eq!(in_junk[0].category_id, Some(junk.id));

        // 规则：apply_existing 会把发件人 x@y.z 的全部邮件都归到「重要」
        // （包括之前手动移进「垃圾邮件」的 uid 2 —— 规则明确表达了意图）
        let important = cats.iter().find(|c| c.name == "重要").unwrap();
        let (_, moved) = db.add_rule(A, "@y.z", important.id, true).unwrap();
        assert_eq!(moved, 3);
        let rules = db.list_rules(A).unwrap();
        assert_eq!(rules.len(), 1);
        assert_eq!(rules[0].category_id, important.id);
        // 三封邮件的发件人都是 x@y.z，收件箱被清空
        assert!(db.list_envelopes(A, F, 100, View::Inbox).unwrap().is_empty());
        assert_eq!(db.list_envelopes(A, F, 100, View::Category(important.id)).unwrap().len(), 3);

        // 新邮件走同步路径：apply_sync 不会动 category_id，但删除时连带清理
        db.apply_sync(A, F, &[3], &[], &[]).unwrap();
        assert_eq!(db.list_envelopes(A, F, 100, View::All).unwrap().len(), 2);

        // 无效输入被拒绝
        assert!(db.add_rule(A, "x", important.id, false).is_err());
        assert!(db.add_rule(A, "@ok.com", 999, false).is_err());
        assert!(db.create_category(A, "", "#fff").is_err());
        assert!(db.create_category(A, "重要", "#fff").is_err());
        assert!(db.create_category(A, "临时", "red").is_err());

        // 重命名 + 改色
        db.update_category(A, junk.id, Some("垃圾"), Some("#FF0000")).unwrap();
        let cats = db.list_categories(A).unwrap();
        let junk = cats.iter().find(|c| c.id == junk.id).unwrap();
        assert_eq!((junk.name.as_str(), junk.color.as_str()), ("垃圾", "#ff0000"));

        // 删除分类：邮件回收件箱，规则一并删除
        assert!(db.delete_category(A, junk.id).unwrap());
        assert!(!db.delete_category(A, junk.id).unwrap());
        assert_eq!(db.list_rules(A).unwrap().len(), 1);
        // 垃圾分类已删空（邮件都被 @y.z 规则移走了）

        // 账号删除时分类和规则一起清掉
        db.delete_account(A).unwrap();
        assert!(db.list_categories(A).unwrap().is_empty());
        assert!(db.list_rules(A).unwrap().is_empty());
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
    fn stats_and_clearing() {
        let db = Db::open_in_memory().unwrap();
        db.reset_folder(A, F, 1).unwrap();
        db.apply_sync(A, F, &[], &[], &[env(1, false), env(2, false)]).unwrap();
        db.put_body(A, F, 1, b"12345").unwrap();
        assert_eq!(db.stats(A).unwrap(), (2, 1, 5));
        assert_eq!(db.stats("other@qq.com").unwrap(), (0, 0, 0));

        db.clear_bodies().unwrap();
        assert_eq!(db.stats(A).unwrap(), (2, 0, 0));

        db.clear_all().unwrap();
        assert_eq!(db.stats(A).unwrap(), (0, 0, 0));
        assert_eq!(db.uid_validity(A, F).unwrap(), None);
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
