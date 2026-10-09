//! 本机字体列表：给设置页的字体下拉框用。

use std::{collections::BTreeMap, sync::OnceLock};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontFamily {
    pub name: String,
    /// 中文名（如「微软雅黑」），没有则与 name 相同
    pub local_name: String,
    pub monospaced: bool,
}

/// 扫描系统字体，按家族合并（Regular / Bold / Italic 属于同一家族）。
/// 结果按名称排序并缓存，进程内只扫描一次。
pub fn system_fonts() -> &'static [FontFamily] {
    static FONTS: OnceLock<Vec<FontFamily>> = OnceLock::new();
    FONTS.get_or_init(|| {
        let mut db = fontdb::Database::new();
        db.load_system_fonts();

        // family -> (英文名, 本地名, 是否等宽)
        let mut families: BTreeMap<String, (String, String, bool)> = BTreeMap::new();
        for face in db.faces() {
            // families[0] 是英文名，后面可能有本地化名
            let Some((english, _)) = face.families.first() else {
                continue;
            };
            let local = face
                .families
                .iter()
                .find(|(n, l)| n != english && *l != fontdb::Language::English_UnitedStates)
                .map(|(n, _)| n.clone());
            let entry = families
                .entry(english.clone())
                .or_insert_with(|| (english.clone(), english.clone(), face.monospaced));
            if let Some(l) = local {
                entry.1 = l;
            }
            entry.2 |= face.monospaced;
        }

        families
            .into_iter()
            .map(|(_, (en, local, mono))| FontFamily {
                name: en,
                local_name: local,
                monospaced: mono,
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_some_fonts() {
        let fonts = system_fonts();
        assert!(fonts.len() > 5, "至少能找到几个系统字体");
        // Windows 上必然有微软雅黑 / Segoe UI；其他平台可能没有，所以只在 Windows 断言
        #[cfg(target_os = "windows")]
        {
            assert!(
                fonts.iter().any(|f| f.name.contains("Microsoft YaHei")),
                "找不到微软雅黑"
            );
        }
        // 排序且无重复
        let mut names: Vec<&str> = fonts.iter().map(|f| f.name.as_str()).collect();
        let sorted = names.clone();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), sorted.len());
        assert_eq!(names, sorted);
    }
}
