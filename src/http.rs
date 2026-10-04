//! gpui 图片管线的 HTTP 客户端。
//!
//! gpui 默认装的是 `NullHttpClient`——一切请求直接报错，于是 Markdown 里的网络
//! 图片（`![](https://...)`）全部加载失败。这里注册 zed 自己的 reqwest 客户端
//! （与 gpui 同一 git 源和 rev，`http_client::HttpClient` 因此是同一个类型）。
//!
//! 请求跑在客户端自带的 tokio 运行时上，不阻塞 UI；本地相对路径图片不走这里，
//! 由 `TextViewStyle::image_base` 直接解析成文件路径（见 `app.rs` 的预览面）。

use std::sync::Arc;

use reqwest_client::ReqwestClient;

/// 注册为 gpui 的全局 HTTP 客户端；`main` 调用一次（窗口创建之前）。
pub fn register(cx: &mut gpui::App) {
    cx.set_http_client(Arc::new(ReqwestClient::new()));
}
