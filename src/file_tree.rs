//! 左侧文件树的状态与可见行计算。
//!
//! 树用"展开集合 + 展平"的模型，而不是持久化的节点树：文件系统本身是唯一真源。
//! 展平结果由调用方（`Workspace`）缓存，只在展开集合 / 过滤词 / 外部改动时重算
//! ——每帧重算等于每帧在 UI 线程上同步走盘，大树下就是明显的卡顿。Pebrel 的
//! `SidePanel` 同样把展平行缓存成 `rows` / `tree_rows`
//! （`display/side_panel/mod.rs:401-406`），渲染只读缓存。
//!
//! 下面几个上限与跳过清单都照抄 Pebrel 的同名常量，出处逐条写在各自注释里。

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::text_file::display_name;

/// 行高（逻辑 px）。宽松的间距：Pebrel 的侧栏行距走同一档。
pub const ROW_HEIGHT: f32 = 28.0;
/// 每层缩进。
pub const INDENT: f32 = 14.0;

/// 展平行数上限，同时界定文件系统遍历与渲染。
///
/// 照抄 Pebrel 的 `MAX_ROWS`（`display/side_panel/mod.rs:364`）：它既是渲染护栏
/// （配合 `uniform_list` 只画可视区），也是遍历护栏——行数封顶意味着递归读到的
/// 目录条目也封顶，所以侧栏的开销有一个确定的上界。
const MAX_ROWS: usize = 1000;
/// 单目录条目上限，照抄 Pebrel 的 `MAX_PER_DIR`（`display/side_panel/mod.rs:368`）。
/// **在排序之后**截断，所以砍掉的是字母表末尾，而不是 `read_dir` 随机给出的那一半。
const MAX_PER_DIR: usize = 600;
/// 过滤时允许访问的目录条目总数，照抄 Pebrel 的 `SEARCH_VISIT_BUDGET`
/// （`display/side_panel/mod.rs:373`）。
///
/// 它限制的是**遍历本身**，不是保留下来的命中数：`node_modules` / `target` 这类
/// 目录动辄几十万条，每次击键都走一遍会把界面冻住——Pebrel 的注释原话是
/// "walking it per keystroke froze the UI"（`display/side_panel/mod.rs:371`）。
const SEARCH_VISIT_BUDGET: usize = 20_000;
/// 过滤时整个跳过（不列出、不递归）的目录名，照抄 Pebrel 的 `SEARCH_SKIP_DIRS`
/// （`display/side_panel/mod.rs:377`）：全是体量、没有信号。
///
/// 只用于**过滤**：普通展平仍照常显示它们。本编辑器的文件树是主要导航面，
/// "任何文件都能打开"是既有契约，所以不在树里悄悄吞掉目录；遍历开销交给
/// `MAX_ROWS` 兜。大小写不敏感——Windows 上目录名的大小写不可靠。
const SEARCH_SKIP_DIRS: &[&str] =
    &["target", "node_modules", ".git", ".cache", ".gradle", "build", "trellis"];
/// 递归深度上限，防御符号链接成环。
const MAX_DEPTH: usize = 40;

#[derive(Clone, Debug)]
pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

pub struct FileTree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    pub selected: Option<PathBuf>,
}

impl FileTree {
    pub fn new(root: PathBuf) -> Self {
        let mut expanded = HashSet::new();
        // 根目录默认展开：打开就看见内容，不用先点一下。
        expanded.insert(root.clone());
        Self { root, expanded, selected: None }
    }

    pub fn toggle(&mut self, path: &Path) {
        if !self.expanded.remove(path) {
            self.expanded.insert(path.to_path_buf());
        }
    }

    /// 树根目录。标题栏用它说明"当前打开的是哪个目录"。
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// 换一个树根（打开工作区目录）。
    ///
    /// 展开集合是相对**旧根**累积的——留着会让新根下同名路径的目录意外展开，所以
    /// 整份清掉、只把新根设为展开（与 [`FileTree::new`] 的初始态一致）。选中项也
    /// 清掉：它记的是旧树里的某个路径。
    pub fn set_root(&mut self, root: PathBuf) {
        self.expanded.clear();
        self.expanded.insert(root.clone());
        self.selected = None;
        self.root = root;
    }

    /// 展开到某个路径（含其所有祖先），用于把当前文件在树里显出来。
    pub fn reveal(&mut self, path: &Path) {
        let mut current = path.parent();
        while let Some(dir) = current {
            self.expanded.insert(dir.to_path_buf());
            if dir == self.root {
                break;
            }
            current = dir.parent();
        }
    }

    /// 按当前展开状态展平成渲染用的行序列（深度优先，目录在前）。
    pub fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        self.push_children(&self.root, 0, &mut rows);
        rows
    }

    /// 按名称过滤后的行序列。
    ///
    /// 与 [`Self::rows`] 的差别，以及为什么这样定：
    /// - **忽略展开集合**：过滤时若还认收缩状态，匹配项藏在收起目录里就一条都
    ///   看不到，过滤等于失效。所以过滤态一律看整棵树。
    /// - **保留匹配项的祖先**：只列命中行会把层级信息丢掉（`main.rs` 是从哪来的
    ///   看不出来），所以自顶向下走一遍、保留"自己命中或有子孙命中"的目录。
    /// - 空查询回落 [`Self::rows`]，缩进与展开标记照旧。
    ///
    /// 匹配不区分大小写（过滤器都是"记不住大小写、只想找那个名字"的用法）。
    pub fn rows_filtered(&self, query: &str) -> Vec<Row> {
        self.rows_filtered_with_budget(query, SEARCH_VISIT_BUDGET)
    }

    /// [`Self::rows_filtered`] 的预算可注入版本，便于测试访问预算真的生效
    /// （真预算 2 万条，测不起）。
    fn rows_filtered_with_budget(&self, query: &str, budget: usize) -> Vec<Row> {
        let needle = query.trim().to_lowercase();
        if needle.is_empty() {
            return self.rows();
        }
        let mut rows = Vec::new();
        let mut visited = 0usize;
        self.push_matching(&self.root, 0, &needle, budget, &mut visited, &mut rows);
        rows
    }

    /// 过滤态的递归：目录若自身或其子孙命中就保留，命中行的 `expanded` 语义
    /// 在过滤态没有意义（树整体展开），一律给 `true` 让箭头显示为"展开"。
    ///
    /// `budget` 是允许访问的条目总数，`visited` 是当前已访问计数：这两样是照抄
    /// Pebrel 的 `SEARCH_VISIT_BUDGET`——限定的是遍历本身，否则一次过滤就能把
    /// 整棵 `node_modules` 走穿。
    fn push_matching(
        &self,
        dir: &Path,
        depth: usize,
        needle: &str,
        budget: usize,
        visited: &mut usize,
        rows: &mut Vec<Row>,
    ) {
        if depth >= MAX_DEPTH || rows.len() >= MAX_ROWS || *visited >= budget {
            return;
        }
        for (path, is_dir) in read_dir_sorted(dir) {
            if rows.len() >= MAX_ROWS || *visited >= budget {
                return;
            }
            *visited += 1;
            let name = display_name(&path);
            // 体量目录整个跳过：既不列进结果、也不递归进去（Pebrel 同款）。
            if is_dir
                && SEARCH_SKIP_DIRS.iter().any(|skip| skip.eq_ignore_ascii_case(&name))
            {
                continue;
            }
            let self_matches = name.to_lowercase().contains(needle);
            if is_dir {
                // 先递归看子孙里有没有命中，再决定这一行去留。命中的目录连同
                // 整棵子树展开，让用户看得到"匹配所在的那片区域"。
                let before = rows.len();
                self.push_matching(&path, depth + 1, needle, budget, visited, rows);
                let descendant_matches = rows.len() > before;
                if self_matches || descendant_matches {
                    rows.insert(
                        before,
                        Row {
                            name,
                            path,
                            depth,
                            is_dir: true,
                            expanded: true,
                        },
                    );
                }
            } else if self_matches {
                rows.push(Row {
                    name,
                    path,
                    depth,
                    is_dir: false,
                    expanded: false,
                });
            }
        }
    }

    fn push_children(&self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        if depth >= MAX_DEPTH || rows.len() >= MAX_ROWS {
            return;
        }
        // 单目录在这里截断（Pebrel 的 `MAX_PER_DIR`）：树是概览面，一个目录列
        // 600 条够用，剩下的靠过滤框找。过滤路径**不**截（见 `read_dir_sorted`）。
        for (path, is_dir) in read_dir_sorted(dir).into_iter().take(MAX_PER_DIR) {
            if rows.len() >= MAX_ROWS {
                return;
            }
            let expanded = is_dir && self.expanded.contains(&path);
            rows.push(Row {
                name: display_name(&path),
                path: path.clone(),
                depth,
                is_dir,
                expanded,
            });
            if expanded {
                self.push_children(&path, depth + 1, rows);
            }
        }
    }
}

/// 读一层目录，目录在前、再按名字不区分大小写排序。
///
/// 读不了（权限等）就当空目录：文件树不该因为一个目录读不动而整棵树消失。
///
/// **不做每目录截断**：截断的时机由两条消费路径各自决定——树展平按
/// `MAX_PER_DIR` 截（Pebrel 的 `flatten_dir_into` 就是这么做的），过滤**不截**
/// （过滤存在的意义正是在大目录里找出第 600 个之后的那个文件；Pebrel 的过滤走
/// 单独的索引遍历，不受 `MAX_PER_DIR` 限制）。
fn read_dir_sorted(dir: &Path) -> Vec<(PathBuf, bool)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<(PathBuf, bool)> = entries
        .filter_map(Result::ok)
        .map(|entry| {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            (entry.path(), is_dir)
        })
        .collect();
    crate::text_file::sort_entries(&mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 每次用独立的临时目录，避免并行测试互相看见对方的文件。
    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("nebula-lite-tree-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("zeta")).unwrap();
        std::fs::create_dir_all(dir.join("alpha")).unwrap();
        std::fs::write(dir.join("readme.md"), b"x").unwrap();
        std::fs::write(dir.join("main.rs"), b"x").unwrap();
        std::fs::write(dir.join("alpha").join("nested.rs"), b"x").unwrap();
        dir
    }

    /// 根默认展开（打开就看得见内容），子目录默认收起；目录排在文件前面。
    #[test]
    fn root_is_expanded_and_directories_sort_first() {
        let dir = scratch("sort");
        let tree = FileTree::new(dir.clone());
        let rows = tree.rows();
        let names: Vec<String> = rows.iter().map(|row| row.name.clone()).collect();
        assert_eq!(names, vec!["alpha", "zeta", "main.rs", "readme.md"]);
        assert!(rows.iter().all(|row| row.depth == 0), "根的子项都在第 0 层");
        assert!(
            rows.iter().filter(|row| row.is_dir).all(|row| !row.expanded),
            "子目录默认收起"
        );
    }

    #[test]
    fn toggling_a_directory_reveals_then_hides_its_children() {
        let dir = scratch("toggle");
        let mut tree = FileTree::new(dir.clone());
        let alpha = dir.join("alpha");

        tree.toggle(&alpha);
        let nested = tree
            .rows()
            .into_iter()
            .find(|row| row.name == "nested.rs")
            .expect("展开后子项应出现在展平结果里");
        assert!(!nested.is_dir);
        assert_eq!(nested.depth, 1, "子项比父目录深一层");

        tree.toggle(&alpha);
        assert!(!tree.rows().iter().any(|row| row.name == "nested.rs"), "再点一次收起");
    }

    /// 打开文件时要把它在树里显出来，即使它所在的目录原本是收起的。
    #[test]
    fn reveal_expands_the_ancestors_of_a_file() {
        let dir = scratch("reveal");
        let mut tree = FileTree::new(dir.clone());
        tree.reveal(&dir.join("alpha").join("nested.rs"));
        assert!(tree.rows().iter().any(|row| row.name == "nested.rs"));
    }

    /// 不存在的目录当空目录处理：文件树不因为一个读不动的目录整棵消失。
    #[test]
    fn unreadable_directory_yields_no_rows() {
        let tree = FileTree::new(PathBuf::from("Z:/nebula-lite-definitely-missing"));
        assert!(tree.rows().is_empty());
    }

    /// 换根（打开工作区目录）后：新根默认展开、列出新内容，旧根的展开状态与选中项
    /// 都不该残留（它们是相对旧根累积的）。
    #[test]
    fn set_root_replaces_contents_and_resets_state() {
        let first = scratch("set-root-a");
        let second = scratch("set-root-b");
        let mut tree = FileTree::new(first.clone());
        // 在旧根下展开一个子目录、选中一个文件，制造"需要被清掉"的状态。
        tree.toggle(&first.join("alpha"));
        tree.selected = Some(first.join("readme.md"));

        tree.set_root(second.clone());

        let names: Vec<String> = tree.rows().into_iter().map(|row| row.name).collect();
        // 新根默认展开，子项都在第 0 层；旧根独有的选中项被清空。
        assert_eq!(names, vec!["alpha", "zeta", "main.rs", "readme.md"]);
        assert_eq!(tree.root(), second.as_path());
        assert_eq!(tree.selected, None, "换根要清掉旧树里的选中项");
        assert!(
            tree.rows().iter().all(|row| row.depth == 0),
            "旧根的展开集合不该让新根的目录意外展开"
        );
    }

    /// 空查询回落普通展平：过滤框没输入时行为必须与没有它时完全一致。
    #[test]
    fn empty_filter_falls_back_to_the_normal_rows() {
        let dir = scratch("filter-empty");
        let tree = FileTree::new(dir);
        let normal: Vec<PathBuf> = tree.rows().into_iter().map(|row| row.path).collect();
        let filtered: Vec<PathBuf> =
            tree.rows_filtered("   ").into_iter().map(|row| row.path).collect();
        assert_eq!(normal, filtered);
    }

    /// 过滤要忽略展开集合：命中的文件即使在收起的目录里也要列出来，
    /// 否则过滤等于失效（这是它与 `rows` 最关键的差别）。
    #[test]
    fn filter_sees_into_collapsed_directories() {
        let dir = scratch("filter-collapsed");
        let tree = FileTree::new(dir);
        // `alpha` 默认是收起的，nested.rs 在普通展平里不出现。
        assert!(!tree.rows().iter().any(|row| row.name == "nested.rs"));
        let filtered = tree.rows_filtered("nested");
        let hit = filtered.iter().find(|row| row.name == "nested.rs");
        assert!(hit.is_some(), "过滤要穿透收起的目录");
    }

    /// 保留匹配项的祖先目录，层级才看得出来；同层不匹配的邻居不能带进来。
    #[test]
    fn filter_keeps_ancestors_of_matches() {
        let dir = scratch("filter-ancestors");
        let tree = FileTree::new(dir);
        let names: Vec<String> =
            tree.rows_filtered("nested").into_iter().map(|row| row.name).collect();
        assert_eq!(names, vec!["alpha", "nested.rs"], "父目录 alpha 要留着，无关的 zeta 不出现");
    }

    /// 匹配不区分大小写：`README` 与 `readme` 该命中同一批。
    #[test]
    fn filter_is_case_insensitive() {
        let dir = scratch("filter-case");
        let tree = FileTree::new(dir);
        let lower: Vec<String> =
            tree.rows_filtered("readme").into_iter().map(|row| row.name).collect();
        let upper: Vec<String> =
            tree.rows_filtered("README").into_iter().map(|row| row.name).collect();
        assert_eq!(lower, vec!["readme.md"]);
        assert_eq!(lower, upper);
    }

    /// 一条都不命中时返回空，而不是回落成整棵树。
    #[test]
    fn filter_without_matches_is_empty() {
        let dir = scratch("filter-none");
        let tree = FileTree::new(dir);
        assert!(tree.rows_filtered("zzz-no-such-file").is_empty());
    }

    /// 过滤要跳过 `node_modules` / `target` 这类体量目录：既不列进来、也不递归
    /// 进去（Pebrel `SEARCH_SKIP_DIRS` 的语义）。大小写不敏感——Windows 上目录名
    /// 的大小写不可靠。
    #[test]
    fn filter_skips_bulk_directories() {
        let dir = scratch("filter-skip");
        // 命中词藏在 node_modules 里，另一份放在普通目录里做对照。
        std::fs::create_dir_all(dir.join("node_modules").join("pkg")).unwrap();
        std::fs::write(dir.join("node_modules").join("pkg").join("grid.rs"), b"x").unwrap();
        std::fs::write(dir.join("alpha").join("grid.rs"), b"x").unwrap();

        let names: Vec<String> =
            tree_names(&FileTree::new(dir), "grid");
        assert_eq!(names, vec!["alpha", "grid.rs"], "node_modules 整个跳过，普通目录照常命中");

        // 大小写不敏感：`Node_Modules` 同样跳过。
        let dir = scratch("filter-skip-case");
        std::fs::create_dir_all(dir.join("Node_Modules").join("pkg")).unwrap();
        std::fs::write(dir.join("Node_Modules").join("pkg").join("grid.rs"), b"x").unwrap();
        assert!(tree_names(&FileTree::new(dir), "grid").is_empty());
    }

    /// 访问预算真的封顶遍历：预算耗尽后不再往里走，结果里不会带出更深的命中。
    /// 真预算是 2 万条（测不起），所以走可注入版本。
    #[test]
    fn filter_visit_budget_bounds_the_walk() {
        let dir = scratch("filter-budget");
        // 顶层铺满条目，把预算全吃掉；深处藏一个命中。
        for i in 0..50 {
            std::fs::write(dir.join(format!("filler{i:03}.txt")), b"x").unwrap();
        }
        std::fs::create_dir_all(dir.join("deep")).unwrap();
        std::fs::write(dir.join("deep").join("treasure.rs"), b"x").unwrap();

        // 预算够：命中找得到。
        let found = tree_names_budget(&FileTree::new(dir.clone()), "treasure", 10_000);
        assert_eq!(found, vec!["deep", "treasure.rs"], "预算充足时应命中深层文件");

        // 预算只有几条：遍历在最外层就被截断，命中的深层文件不再出现。
        let starved = tree_names_budget(&FileTree::new(dir), "treasure", 3);
        assert!(starved.is_empty(), "预算耗尽后不应继续遍历，实际 {starved:?}");
    }

    /// 两层上限都真的生效：
    /// - 树展平里单目录截断在 `MAX_PER_DIR`（平铺目录先撞到它）；
    /// - 总行数截断在 `MAX_ROWS`（多个目录各摊满 `MAX_PER_DIR`，合计超过它）。
    #[test]
    fn rows_respect_per_dir_and_total_caps() {
        // 平铺一层：撞单目录上限。
        let flat = scratch("cap-per-dir");
        for i in 0..(MAX_PER_DIR + 200) {
            std::fs::write(flat.join(format!("f{i:05}.txt")), b"x").unwrap();
        }
        assert_eq!(
            FileTree::new(flat).rows().len(),
            MAX_PER_DIR,
            "树展平的单目录条目应截断在 MAX_PER_DIR"
        );

        // 多个目录各摊满：合计越过总上限。
        let wide = scratch("cap-total");
        let mut tree = FileTree::new(wide.clone());
        for dir in 0..3 {
            let sub = wide.join(format!("d{dir}"));
            std::fs::create_dir_all(&sub).unwrap();
            for i in 0..MAX_PER_DIR {
                std::fs::write(sub.join(format!("f{i:05}.txt")), b"x").unwrap();
            }
            tree.toggle(&sub); // 展开，让子孙进入展平
        }
        assert_eq!(tree.rows().len(), MAX_ROWS, "总行数应截断在 MAX_ROWS");
    }

    /// 过滤**不**受单目录上限约束：过滤存在的意义就是在大目录里找出排在很后面
    /// 的那个文件（Pebrel 的过滤走独立索引，不受 `MAX_PER_DIR` 限制）。
    #[test]
    fn filter_ignores_the_per_directory_cap() {
        let dir = scratch("filter-beyond-cap");
        for i in 0..(MAX_PER_DIR + 100) {
            std::fs::write(dir.join(format!("filler{i:05}.txt")), b"x").unwrap();
        }
        // 名字排在字母表末尾，必然落在那 600 条之外。
        std::fs::write(dir.join("zzz_treasure.rs"), b"x").unwrap();

        assert_eq!(tree_names(&FileTree::new(dir), "treasure"), vec!["zzz_treasure.rs"]);
    }

    fn tree_names(tree: &FileTree, query: &str) -> Vec<String> {
        tree.rows_filtered(query).into_iter().map(|row| row.name).collect()
    }

    fn tree_names_budget(tree: &FileTree, query: &str, budget: usize) -> Vec<String> {
        tree.rows_filtered_with_budget(query, budget)
            .into_iter()
            .map(|row| row.name)
            .collect()
    }
}
