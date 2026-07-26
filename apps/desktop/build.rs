// T19 构建脚本：编译 ui/main.slint 生成 Rust 绑定（lib.rs 经
// slint::include_modules! 引入）。rerun-if-changed 由 slint-build 自动登记。
// 开发版快捷方式自动化不在此处：build.rs 运行于链接之前拿不到 exe，
// 该职责由 `cargo xtask dev-shortcut` 承担（ADR-0014）。

fn main() {
    slint_build::compile("ui/main.slint").expect("编译 ui/main.slint 失败");
}
