//! 应用设置的持久化：两个字体槽位 + 两张字体列表的顶置项。
//!
//! 落盘位置 `%APPDATA%\nebula-lite\settings.json`。为什么要有这个模块：字体是
//! **用户偏好**，从工作区 / 磁盘上推导不出来，不落盘就只能每次启动回默认值
//! （用户报的"界面字体改了、一重启又变回默认"）。
//!
//! 三条约定：
//! - 文件缺失、内容坏掉、字段缺失都**不报错**，一律回落默认值——设置文件坏掉
//!   不该让编辑器打不开。字段全带 `#[serde(default)]`，所以旧版本写的文件也能读。
//! - 写入失败只写一行 stderr（`%APPDATA%` 不可写这种事，用户改不了字体也没意义
//!   去打断他）。
//! - 这里只存"不能从别处推导出来的东西"。窗口位置、标签集合这类没做（会话恢复
//!   是另一件事，见 README「未做」）。
//!
//! 顶置项存的是**字体名**而不是下标：字体目录来自系统枚举，装/卸一个字体就会
//! 让下标错位。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 设置文件名（`%APPDATA%\nebula-lite\settings.json`）。
pub fn settings_path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("nebula-lite").join("settings.json"))
}

/// 持久化的设置。全部可缺省——`Settings::default()` 就是"什么都没设"。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// **界面 / 标题字体**：标题栏、标签条、侧栏、状态栏。`None` = 用内置默认。
    pub ui_font: Option<String>,
    /// **编辑器 / 预览字体**。`None` = 用内置的 Maple Mono。
    pub editor_font: Option<String>,
    /// 被顶置的界面字体，顺序即列表里的先后。
    pub pinned_ui_fonts: Vec<String>,
    /// 被顶置的编辑器字体。
    pub pinned_editor_fonts: Vec<String>,
}

impl Settings {
    /// 读设置。任何一步失败都回落默认值（见模块注释）。
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        let Ok(text) = std::fs::read_to_string(&path) else {
            // 首次运行没有这个文件，属于正常情况，不是错误。
            return Self::default();
        };
        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            Err(error) => {
                eprintln!(
                    "[nebula-lite] 设置文件无法解析，本次按默认值启动：{}（{error}）",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// 写设置。每次改动立即落盘——设置面板里的改动本来就是低频的，攒着一起写
    /// 只会多出"什么时候写"这个状态。
    pub fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        if let Some(dir) = path.parent()
            && let Err(error) = std::fs::create_dir_all(dir)
        {
            eprintln!("[nebula-lite] 无法创建设置目录 {}：{error}", dir.display());
            return;
        }
        let json = match serde_json::to_string_pretty(self) {
            Ok(json) => json,
            Err(error) => {
                eprintln!("[nebula-lite] 设置无法序列化：{error}");
                return;
            }
        };
        if let Err(error) = std::fs::write(&path, json) {
            eprintln!("[nebula-lite] 设置写盘失败 {}：{error}", path.display());
        }
    }
}

/// 字体列表的显示顺序：被顶置的排在最前（按顶置先后），其余保持系统给的顺序。
///
/// 顶置项里出现但目录里没有的名字（字体被卸载了）直接跳过——不报错，也不在
/// 列表里留一个选不了的幽灵项。纯函数，好测。
pub fn ordered<'a>(catalog: &'a [String], pins: &[String]) -> Vec<&'a str> {
    let mut order: Vec<&str> = Vec::with_capacity(catalog.len());
    for pin in pins {
        if let Some(name) = catalog.iter().find(|name| *name == pin)
            && !order.contains(&name.as_str())
        {
            order.push(name);
        }
    }
    order.extend(
        catalog
            .iter()
            .filter(|name| !pins.iter().any(|pin| pin == *name))
            .map(String::as_str),
    );
    debug_assert_eq!(order.len(), catalog.len(), "每个候选必须恰好出现一次");
    order
}

#[cfg(test)]
mod tests {
    use super::*;

    fn catalog(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| (*name).to_owned()).collect()
    }

    /// 没有顶置项时顺序原样保留——"什么都没设"不能把列表打乱。
    #[test]
    fn no_pins_keeps_the_catalog_order() {
        let names = catalog(&["A", "B", "C"]);
        assert_eq!(ordered(&names, &[]), vec!["A", "B", "C"]);
    }

    /// 顶置项排到最前，且**按顶置先后**——用户后顶置的那个在最上面。
    #[test]
    fn pins_come_first_in_pin_order() {
        let names = catalog(&["A", "B", "C"]);
        let pins = vec!["C".to_owned(), "A".to_owned()];
        assert_eq!(ordered(&names, &pins), vec!["C", "A", "B"]);
    }

    /// 名字已不在目录里（字体被卸载）的顶置项被跳过，列表长度不变、也不重复。
    #[test]
    fn unknown_pins_are_skipped_without_duplicating() {
        let names = catalog(&["A", "B"]);
        let pins = vec!["不存在".to_owned(), "B".to_owned()];
        assert_eq!(ordered(&names, &pins), vec!["B", "A"]);
    }

    /// 同一个名字被顶置两次只出现一次（防手改设置文件造出两行一样的）。
    #[test]
    fn duplicated_pins_do_not_duplicate_rows() {
        let names = catalog(&["A", "B"]);
        let pins = vec!["A".to_owned(), "A".to_owned()];
        assert_eq!(ordered(&names, &pins), vec!["A", "B"]);
    }

    /// 缺字段、多字段、空对象都要能读。旧版本写的文件不该让编辑器起不来。
    #[test]
    fn missing_and_unknown_fields_are_tolerated() {
        let settings: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(settings, Settings::default());
        let settings: Settings =
            serde_json::from_str(r#"{"ui_font":"X","将来才有的字段":1}"#).unwrap();
        assert_eq!(settings.ui_font.as_deref(), Some("X"));
        assert!(settings.pinned_ui_fonts.is_empty());
    }

    /// 存-读一轮必须逐字段相等（否则"改了没记住"会以另一种形式回来）。
    #[test]
    fn round_trip_keeps_every_field() {
        let settings = Settings {
            ui_font: Some("Microsoft YaHei UI".to_owned()),
            editor_font: Some("Maple Mono Normal NF CN".to_owned()),
            pinned_ui_fonts: vec!["A".to_owned(), "B".to_owned()],
            pinned_editor_fonts: vec!["C".to_owned()],
        };
        let json = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(settings, restored);
    }
}
