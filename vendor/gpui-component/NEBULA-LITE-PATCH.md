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

## 唯一的功能性改动

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

## 跟上游同步时怎么做

1. 在新的 rev 上重新截取：`Cargo.toml`、`README.md`、`LICENSE-APACHE`、
   `.rustfmt.toml`、`themes/`、`crates/{ui,assets,macros}`。
2. 按上表裁短根 `Cargo.toml` 的 `[workspace] members`（并把
   `[workspace.dependencies] story = …` 那行删掉，它指向没复制过来的 crate）。
3. 在新版 `crates/ui/src/input/mode.rs` 里重新套用上面那段补丁。
4. 同步更新 `nebula-lite/Cargo.toml` 里 `gpui-component` 与
   `gpui-component-assets` 的 `rev`，以及本文件里的 rev。
