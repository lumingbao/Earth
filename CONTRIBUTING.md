# 参与 Earth 开发

使用支持 Edition 2024 的 Rust stable（至少 1.95）。从项目根目录运行 `cargo run`；macOS 和 Windows 均有平台专用代码。修改平台交互时，在对应系统验证。

提交前运行：

```sh
cargo fmt --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked
```

每个改动围绕一个可复查的问题；提交标题说明触发条件或修复结果，PR 中写明 issue 编号、验证方式和未验证的平台。测试使用合成内容和临时目录。请勿上传真实剪贴板、凭据或用户数据库。报告安全问题请参考 [安全反馈](SECURITY.md)。
