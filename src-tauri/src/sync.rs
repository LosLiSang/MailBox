//! 增量同步：对比本地缓存与服务器，只拉取新邮件头。
//!
//! 流程：
//! 1. SELECT 文件夹，拿到 UIDVALIDITY；与本地不一致就清空本地缓存
//! 2. 本地为空：按序号拉取最近 `window` 封的 UID+FLAGS
//!    本地非空：`UID FETCH <最小已缓存UID>:* (UID FLAGS)`，只拿标记，很轻量
//! 3. 用 [`plan`] 算出：被删除的、已读状态变化的、新到的
//! 4. 只对新到的邮件拉取邮件头，一次事务写入 SQLite

use crate::{db::Db, imap_client, mail::recent_range};
use serde::Serialize;
use std::collections::{HashMap, HashSet};

pub const INBOX: &str = "INBOX";

#[derive(Debug, Default, PartialEq)]
pub struct SyncPlan {
    pub deleted: Vec<u32>,
    pub flag_changes: Vec<(u32, bool)>,
    pub new_uids: Vec<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStats {
    pub added: usize,
    pub deleted: usize,
    pub updated: usize,
}

/// `cached`：本地 (uid, seen)；`server`：服务器上 >= 最小已缓存 UID 的 (uid, seen)
pub fn plan(cached: &[(u32, bool)], server: &[(u32, bool)], max_new: usize) -> SyncPlan {
    let server_map: HashMap<u32, bool> = server.iter().copied().collect();
    let cached_set: HashSet<u32> = cached.iter().map(|(uid, _)| *uid).collect();

    let mut deleted = Vec::new();
    let mut flag_changes = Vec::new();
    for &(uid, seen) in cached {
        match server_map.get(&uid) {
            None => deleted.push(uid),
            Some(&s) if s != seen => flag_changes.push((uid, s)),
            _ => {}
        }
    }

    let mut new_uids: Vec<u32> = server_map
        .keys()
        .copied()
        .filter(|uid| !cached_set.contains(uid))
        .collect();
    new_uids.sort_unstable();
    // 新邮件太多时只要最新的
    let skip = new_uids.len().saturating_sub(max_new);
    new_uids.drain(..skip);

    SyncPlan {
        deleted,
        flag_changes,
        new_uids,
    }
}

/// 把 UID 列表压缩成 IMAP 序列集，如 [1,2,3,5,7,8] -> "1:3,5,7:8"
pub fn uid_set(uids: &[u32]) -> String {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();

    let mut parts = Vec::new();
    let mut iter = sorted.into_iter().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&(end + 1)) {
            end = iter.next().unwrap();
        }
        parts.push(if start == end {
            start.to_string()
        } else {
            format!("{start}:{end}")
        });
    }
    parts.join(",")
}

/// 从服务器 UID 中挑出比 `before` 更早的最新 `count` 封（升序），用于「加载更多」
pub fn older_page(server: impl IntoIterator<Item = u32>, before: u32, count: usize) -> Vec<u32> {
    let mut uids: Vec<u32> = server.into_iter().filter(|&uid| uid < before).collect();
    uids.sort_unstable();
    uids.dedup();
    let skip = uids.len().saturating_sub(count);
    uids.drain(..skip);
    uids
}

/// 加载比本地最早缓存更早的 `count` 封邮件头，返回新增数量；0 表示服务器上已经没有更早的了
pub fn load_older(
    db: &Db,
    creds: &imap_client::Credentials,
    account: &str,
    folder: &str,
    count: u32,
) -> Result<usize, String> {
    let mut session = imap_client::connect(creds).map_err(|e| e.to_string())?;
    let result = load_older_with_session(&mut session, db, account, folder, count);
    session.logout().ok();
    result
}

fn load_older_with_session(
    session: &mut imap_client::Session,
    db: &Db,
    account: &str,
    folder: &str,
    count: u32,
) -> Result<usize, String> {
    let e = |e: imap::Error| e.to_string();
    let mailbox = session.select(folder).map_err(e)?;
    if db.uid_validity(account, folder)? != mailbox.uid_validity {
        return Err("邮箱已变化，请先同步再加载更多".into());
    }
    let Some(&(min_uid, _)) = db.cached_flags(account, folder)?.first() else {
        return Ok(0);
    };
    if min_uid <= 1 {
        return Ok(0);
    }
    let older = session
        .uid_search(format!("UID 1:{}", min_uid - 1))
        .map_err(e)?;
    let uids = older_page(older, min_uid, count as usize);
    if uids.is_empty() {
        return Ok(0);
    }

    let rules = db.list_rules(account).ok();
    let default_category = db.default_category(account).ok().flatten();
    let envelopes: Vec<_> = imap_client::fetch_headers(session, &uid_set(&uids))
        .map_err(e)?
        .into_iter()
        .map(|mut env| {
            env.category_id = rules
                .as_ref()
                .and_then(|r| crate::rules::match_sender(r, &env.from_address, &env.from_name))
                .or(default_category);
            env
        })
        .collect();
    db.apply_sync(account, folder, &[], &[], &envelopes)?;
    Ok(envelopes.len())
}

pub fn sync_folder(
    db: &Db,
    creds: &imap_client::Credentials,
    account: &str,
    folder: &str,
    window: u32,
) -> Result<SyncStats, String> {
    let mut session = imap_client::connect(creds).map_err(|e| e.to_string())?;
    let result = sync_with_session(&mut session, db, account, folder, window);
    session.logout().ok();
    result
}

fn sync_with_session(
    session: &mut imap_client::Session,
    db: &Db,
    account: &str,
    folder: &str,
    window: u32,
) -> Result<SyncStats, String> {
    let e = |e: imap::Error| e.to_string();
    let rules = std::cell::RefCell::new(db.list_rules(account).ok());
    let default_category = db.default_category(account).ok().flatten();

    let mailbox = session.select(folder).map_err(e)?;
    let server_validity = mailbox.uid_validity.unwrap_or(0);
    if db.uid_validity(account, folder)? != Some(server_validity) {
        db.reset_folder(account, folder, server_validity)?;
    }

    let cached = db.cached_flags(account, folder)?;
    let server = if mailbox.exists == 0 {
        vec![]
    } else if let Some(&(min_uid, _)) = cached.first() {
        imap_client::fetch_flags_by_uid(session, &format!("{min_uid}:*"))
            .map_err(e)?
            .into_iter()
            // 所有邮件 UID 都小于 min_uid 时，服务器会把 * 解释成最大的那封返回
            .filter(|(uid, _)| *uid >= min_uid)
            .collect()
    } else {
        let range = recent_range(mailbox.exists, window).unwrap_or_default();
        imap_client::fetch_flags_by_seq(session, &range).map_err(e)?
    };

    let plan = plan(&cached, &server, window as usize);
    let new_envelopes = if plan.new_uids.is_empty() {
        vec![]
    } else {
        imap_client::fetch_headers(session, &uid_set(&plan.new_uids))
            .map_err(e)?
            .into_iter()
            .map(|mut env| {
                // 新邮件按发件人规则自动归类
                env.category_id = rules
                    .borrow()
                    .as_ref()
                    .and_then(|r| crate::rules::match_sender(r, &env.from_address, &env.from_name))
                    .or(default_category);
                env
            })
            .collect()
    };

    db.apply_sync(
        account,
        folder,
        &plan.deleted,
        &plan.flag_changes,
        &new_envelopes,
    )?;

    Ok(SyncStats {
        added: new_envelopes.len(),
        deleted: plan.deleted.len(),
        updated: plan.flag_changes.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_sync() {
        struct Case {
            name: &'static str,
            cached: &'static [(u32, bool)],
            server: &'static [(u32, bool)],
            max_new: usize,
            want: SyncPlan,
        }
        let cases = [
            Case {
                name: "首次同步",
                cached: &[],
                server: &[(5, true), (3, false), (4, false)],
                max_new: 10,
                want: SyncPlan {
                    new_uids: vec![3, 4, 5],
                    ..Default::default()
                },
            },
            Case {
                name: "无变化",
                cached: &[(1, true), (2, false)],
                server: &[(1, true), (2, false)],
                max_new: 10,
                want: SyncPlan::default(),
            },
            Case {
                name: "删除 + 已读变化 + 新邮件",
                cached: &[(1, true), (2, false), (3, false)],
                server: &[(1, false), (3, true), (4, false), (6, true)],
                max_new: 10,
                want: SyncPlan {
                    deleted: vec![2],
                    flag_changes: vec![(1, false), (3, true)],
                    new_uids: vec![4, 6],
                },
            },
            Case {
                name: "新邮件超过上限只保留最新",
                cached: &[(1, true)],
                server: &[(1, true), (2, false), (3, false), (4, false), (5, false)],
                max_new: 2,
                want: SyncPlan {
                    new_uids: vec![4, 5],
                    ..Default::default()
                },
            },
            Case {
                name: "服务器清空",
                cached: &[(1, true), (2, false)],
                server: &[],
                max_new: 10,
                want: SyncPlan {
                    deleted: vec![1, 2],
                    ..Default::default()
                },
            },
        ];
        for c in cases {
            assert_eq!(plan(c.cached, c.server, c.max_new), c.want, "{}", c.name);
        }
    }

    #[test]
    fn picks_older_page() {
        let cases: &[(&[u32], u32, usize, &[u32])] = &[
            (&[], 10, 5, &[]),
            (&[1, 2, 3], 1, 5, &[]),
            (&[1, 2, 3, 4], 10, 5, &[1, 2, 3, 4]),
            (&[9, 1, 5, 3, 7], 8, 2, &[5, 7]),
            // 服务器把 * 解释成最大 UID 时可能带回 >= before 的结果
            (&[3, 10, 12], 10, 5, &[3]),
        ];
        for (server, before, count, want) in cases {
            assert_eq!(
                older_page(server.iter().copied(), *before, *count),
                *want,
                "{server:?} before {before}"
            );
        }
    }

    #[test]
    fn compresses_uid_set() {
        let cases: &[(&[u32], &str)] = &[
            (&[], ""),
            (&[7], "7"),
            (&[1, 2, 3], "1:3"),
            (&[8, 1, 3, 2, 5, 7], "1:3,5,7:8"),
            (&[4, 4, 5], "4:5"),
        ];
        for (uids, want) in cases {
            assert_eq!(uid_set(uids), *want, "{uids:?}");
        }
    }
}
