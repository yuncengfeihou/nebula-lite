# nebula-lite 对 gpui-component 的本地副本与补丁

这个目录**不是**上游仓库的完整副本，而是从

```
https://github.com/Kuddev/gpui-component
rev f46ecbb06de5907af9700f3f6e2b5b0845459188   （= Pebrel 1.9.1 锁定的那个 rev）
version 0.5.2
```

截取出来的一份，只保留真正参与构建的部分：

| 保留 | 说明 |
| --- | --- |
| `crates/ui` | 组件库本体（`gpui-component`） |
| `crates/assets` | 默认图标集（`gpui-component-assets`，被 `ui` 的路径依赖引用） |
| `crates/macros` | `gpui-component-macros`（被 `ui` 的路径依赖引用） |
| `themes/` | `crates/ui/src/theme/schema.rs` 用 `include_str!` 读它 |

丢掉的是 `examples/`、`crates/story`、`crates/story-web`、`crates/webview`、
`docs/`、`.github/`，以及根 `Cargo.lock`。根 `Cargo.toml` 的 `[workspace] members`
因此被裁短（这是第二处改动，纯裁剪、无功能含义）。

`nebula-lite/Cargo.toml` 末尾的 `[patch."https://github.com/Kuddev/gpui-component"]`
把这两个包指向本目录；`gpui-component-assets` 必须一起换，它带 `links`，
同名的两份会让 cargo 报重复链接。

## 功能性改动（八处）

### 1. 编辑器首次解析改同步

**文件**：`crates/ui/src/input/mode.rs`，`InputMode::update_highlighter` 里那段
`SYNC_PARSE_TIMEOUT` / `SYNC_PARSE_MAX_BYTES`。改动块上下有
`───── nebula-lite 本地补丁` 的标记注释，全仓库搜这个标记即可定位。

**上游的行为**：首次解析也只用 2ms 的前台预算。一旦超时（任何大于约 2KB 的
文件都会超时），活儿被交给后台任务，而后台任务在真正 parse 之前还会
`await` 一个 150ms 的防抖定时器（`state.rs::dispatch_background_parse` 的
`PARSE_DEBOUNCE`）。表现出来就是"打开文件后几百毫秒正文没有颜色，
然后颜色一下子全出来"。

**补丁后的行为**：`highlighter.tree().is_none()`（= 这个 highlighter 第一次
接触这份文本，即刚打开文件 / 刚换语言）走**同步**全量解析，预算 250ms、
体积上限 2 MiB；超限或超时才退回原来那条后台路径。之后的增量解析
（`force = true`，也就是用户打字）原样保留上游的 2ms + 150ms 防抖——
对连续输入来说那个防抖是有用的。

这与 Notepad3 的做法一致：Scintilla 在载入时同步做词法分析，只对需要显示的
区间做样式化，同步路径上没有异步等待。

### 2. Markdown 表格列宽按实测像素分配

**文件**：`crates/ui/src/text/node.rs`，`NodeRenderer::render_wrap_table`。
同样有 `───── nebula-lite 本地补丁` 标记。

**上游的行为**：`render_table` 按 `style.table` 的横向 overflow 分流。默认
（无 overflow）走 `render_wrap_table`，列宽用 **字符数**
（`cell.children.text_len()`）当权重，写成 `Length::Definite(relative(len))`，
也就是百分比宽。

**问题**：字符数不是显示宽度。一个 CJK 字占两列、一个拉丁字母占一列，所以
"中文为主的列"拿到的权重只有它实际需要的一半——列被压得极窄，格子里的话
被挤成一列一两个字；长标识符（`ctx.tools.register`）还会被从词中间掰断。
用户报的"表格里的字跑出表格外"就是这一条（配合 app 侧开了
`overflow_x_scroll` 走滚动分支时表现为整表向右溢出）。

**补丁后的行为**：与 `render_scroll_table` 用同一套测宽（
`text_system().layout_line`）算出每列的 max-content 像素宽，拿它当
`flex_basis`；`flex_shrink(1)` + 行 `w_full` 保证总宽永远收进容器，
文字在格子内换行。

**关键取舍：不要给列加 `min_w` 下限。** 试过两种下限都会坏事：

- `min-content`（最长不可断片段）→ 中文列被压成"每行两个字"的竖条；
- 半个 max-content → 各列下限之和超过容器宽，flex 缩不下去，整表向右溢出、
  最后一列被切在视口外。

去掉下限、只留 shrink 之后，中文与英文都能正常排（英文长标识符在
`white_space: normal` 下会在超宽处强制断开）。容器侧还需要
`min_w_0`（见 `app.rs::render_preview` 的注释），否则这一列会被表格的
最小宽度撑宽。

### 3. 树状图代码块的行距

**文件**：`crates/ui/src/text/node.rs`，`CodeBlock::render`。同样有
`───── nebula-lite 本地补丁` 标记。

**上游的行为**：代码块里的文本和其它正文一样吃默认行高（`phi()`，
即 1.618 倍字号）。

**问题**：用户反馈 Markdown 预览里的文件树"行与行太挤"。更早的一版曾把
含制表符的代码块压到 1.45 倍行距，比正文还紧，反而更难受。

**补丁后的行为**：渲染时检查代码块文本是否含 U+2500–U+257F；含有时把
这个代码块的行高设为 **1.75 倍**字号，让树状图的每一行都有明显的呼吸
空间；普通代码块保持正文行距。竖线（`│`）在等宽字体里只画满一个 em，
行距拉大后行与行之间会留一点缺口——这是等宽字体 + 大行距的固有取舍，
用户明确选择了"行距优先"。只在渲染期判断，因为 `TextView` 的代码块节点
没有"树状图行距"这样的样式位。

### 4. 查找替换面板的版式

**文件**：`crates/ui/src/input/search.rs`，`SearchPanel::render`（以及为它服务的
`input_width` 字段与 `on_prepaint` 采样）。同样有 `───── nebula-lite 本地补丁` 标记。

**上游的行为**：一行里平铺 搜索框 / 替换开关 / 上一下一 / 计数 / 一个固定 28px
的空白占位 / 关闭按钮；计数用 `Label::min_w_16`（64px）撑开；替换开关是
`selected` 的实心选中样式；替换行的输入框宽度按搜索框的**实测**宽度（`on_prepaint`
采样）写死。

**问题**：右侧那一堆控件之间没有分组关系——一颗显眼的蓝色实心圆点（替换开关）
夹在两个灰色箭头前面，计数占掉 64px，关闭按钮被固定空白推到不知所谓的位置，
替换行又因为按实测宽度排版而与第一行的右缘对不齐。用户报"搜索/替换区域右侧
布局很不协调"。

**补丁后的行为**：两行收进同一套栅格，右缘都对齐到面板内边距。

- 第一行：`[搜索框(flex_1) 替换开关 上一个 下一个 计数]` + 一条 1px 竖发丝线 + 关闭；
- 第二行：`[替换框(flex_1) 替换 全部替换]`——不再采样宽度，`flex_1` 自己填满；
- 计数固定 48px 宽、居中，命中时吃强调色、无命中时吃灰（数字从 `0/0` 跳到 `12/34`
  不再推动旁边的控件）；
- 替换开关与大小写开关改用**淡蓝水洗底 + 强调色图标**表示"开着"，不再是实心圆点；
- 面板底色改吃 `theme.muted`（本应用里是外壳灰），与编辑器纸面分出一档层次。

### 5. 代码块头部与按围栏语言高亮

**文件**：`crates/ui/src/text/style.rs`（`TextViewStyle::code_block_theme`）与
`crates/ui/src/text/node.rs`（`CodeBlock::render` / `CodeBlock::key`）。同样有
`───── nebula-lite 本地补丁` 标记。

**上游的行为**：

- 代码块只渲染正文，没有语言名；调用方的 `code_block_actions` 会被放在一个
  绝对定位的右上角小容器里，没有头部，也无法自然地和语言名排在一起；
- `CodeBlock::styles` 固定读**全局** `cx.theme().highlight_theme` 上色，所以
  ```` ```python ```` 的块套的是当前 Markdown 文档那份配色，而不是 Python 的。

**补丁后的行为**：

- `TextViewStyle` 新增 `code_block_theme: Option<Arc<dyn Fn(Option<&str>) -> Arc<HighlightTheme>>>>`。
  设置时 `CodeBlock::render` 用围栏语言取对应主题；未设置时保持上游行为（全局主题）；
- 围栏有语言时在代码块顶部画一条头部：左侧显示语言名，右侧放
  `code_block_actions` 返回的元素；没有语言但有 actions 时左侧显示 `text`，
  避免一条空白的头；
- 头部与正文同底色、**不画分隔横线**：只靠内边距分层，代码块保持一整块；
- 正文的内边距从整块 12px 改成"有头部 12/8/12、无头部 12"，头部横向铺满、
  贴着圆角；
- 新增 `CodeBlock::key()`（源码位置 `span.start`），给按代码块记录 UI 状态
  （本应用的"刚复制过"对勾）提供跨帧稳定的键。

本应用在 `app.rs::render_preview` 里提供主题闭包（`theme::code_block_highlight_theme`
按围栏语言取 Notepad3 配色并缓存）与复制按钮；围栏语言别名（`py` / `js` /
`c++` / `yml` …）在 `theme::canonical_code_language` 里归一。

### 6. Markdown 列表项之间的纵向空隙

**文件**：`crates/ui/src/text/node.rs`，`BlockNode::render_block` 的 `List` 分支。
同样有 `───── nebula-lite 本地补丁` 标记。

**上游的行为**：列表容器只是 `v_flex`，列表项之间没有间距，嵌套的"文件树式"
列表看起来挤成一片。

**补丁后的行为**：列表容器加 `gap(rems(0.3))`，项与项之间多一条约 4px 的窄空带；
行内文字行高不变。

### 7. 查找面板关着时不再重扫全文（性能）

**文件**：`crates/ui/src/input/search.rs`，`InputState::update_search`。

**上游的行为**：缓冲每次变更都调 `SearchMatcher::update`——整条 rope → String
拷贝一次，再用 Aho-Corasick 扫一遍。面板关着时 `matcher()` 返回 `None`（匹配区间
既不绘制也不参与交互），这次扫描纯属白烧：4.8 MB 的 `.txt` 上实测每键 **3 ms →
21 ms**，打开过查找面板（哪怕已经 Esc 关掉）之后打字一直背着它。

**补丁后的行为**：`matcher().is_none()` 时直接返回。重新打开面板时
`open_search_panel` 本来就会 `matcher.update(&text)`，结果不会变旧。

### 8. 大文档的折行宽度变更推迟合并（性能）

**文件**：`crates/ui/src/input/display_map/display_map.rs`、`wrap_map.rs`、
`input/element.rs`、`input/state.rs`、`display_map/text_wrapper.rs`。

**上游的行为**：`InputElement::prepaint` 发现编辑器宽度变了就同步
`set_wrap_width` → 整篇重折行。4.8 MB 的 `.txt`（89k 行、240 万字符）一次约
**180～200 ms**，而拖窗口 / 拖侧栏时宽度每帧都在变——拖动过程里每帧冻一次，
表现为"拖不动"。

**补丁后的行为**：

- `DisplayMap::on_layout_changed` 在「文档 ≥ 256 KiB、且新旧宽度都是 `Some`」时
  只记下目标宽度（`pending_wrap_width` + 代数），不立即重排；`prepaint` 这一帧
  仍按 `applied_wrap_width()`（旧宽度）排版——旧折行区间对当前文本仍然有效，
  只是断行位置是旧宽度的，拖动期间右缘可能被裁掉。
- `InputState::schedule_pending_wrap` 排一个 150 ms 的**尾沿防抖**：等待期间代数
  一变就接着等，停手之后才 `apply_pending_wrap_width` 整篇重排并 `notify`。
- 首次折行（已生效宽度 `None`）与关掉折行（目标 `None`）不走延迟，行为不变；
  小文档（< 256 KiB）也仍然即时重排。
- 同一文件里顺带：`TextWrapper::_update` 在不折行时不再把每行拷成 `String`
  （那个拷贝只为测量宽度服务），4.8 MB 纯文本打开时省掉 89k 次分配
  （`set_value` 实测 64 ms → 48 ms）。

实测（同机 release，4.8 MB `.txt`、自动换行开）：拖动窗口时每次 resize 的
CPU 从 **64～72 ms** 降到 **5～6 ms**，停手后合并重排一次 74 ms；画面在停手后
回到正确的折行（截图核对过）。

## 跟上游同步时怎么做

1. 在新的 rev 上重新截取：`Cargo.toml`、`README.md`、`LICENSE-APACHE`、
   `.rustfmt.toml`、`themes/`、`crates/{ui,assets,macros}`。
2. 按上表裁短根 `Cargo.toml` 的 `[workspace] members`（并把
   `[workspace.dependencies] story = …` 那行删掉，它指向没复制过来的 crate）。
3. 按上面八节，在新版 `crates/ui/src/input/mode.rs`、`crates/ui/src/text/node.rs`、
   `crates/ui/src/text/style.rs`、`crates/ui/src/input/search.rs`、
   `crates/ui/src/input/display_map/{display_map,wrap_map,text_wrapper}.rs`、
   `crates/ui/src/input/{element,state}.rs` 里重放这几段补丁
   （搜 `───── nebula-lite 本地补丁` 能找到上一版改过的确切位置）。
4. 同步更新 `nebula-lite/Cargo.toml` 里 `gpui-component` 与
   `gpui-component-assets` 的 `rev`，以及本文件里的 rev。
