# B6 国际化 - Slint 集成指南

本文档说明如何在 Slint UI 文件中使用国际化文本键。

## 快速开始

### 1. 在 Rust 代码中初始化

```rust
use localization::{Localization, Language, init_global};

fn init_ui_localization() {
    let l10n = Localization::new(Language::ZhCn).expect("Failed to load zh-CN.json");
    init_global(l10n);
}
```

### 2. 在 ViewModel 绑定层把文本键解析好后传入 Slint

壳（S1）零业务逻辑：功能模块把文本键交给 B6 解析，壳只把解析好的
字符串设进 Slint 属性（缝 1：壳 ↔ 功能模块）。

```rust
use localization::{t, t_with};
use serde_json::json;

// 初始化后，在绑定层把文本设进 Slint 导出的属性
let ui = MainWindow::new()?;
ui.set_review_keep_text(t("review.keep").into());
ui.set_pending_notice(t_with("export.pending_notice", json!({ "count": 3 })).into());
```

### 3. 在 .slint 文件中绑定文本键

**禁止（硬编码，违反 ADR-0005）：**
```slint
Text { text: "保留"; }
```

**正确（属性由 Rust 侧用文本键解析后注入）：**
```slint
export component MainWindow inherits Window {
    in property <string> review-keep-text;  // Rust 侧 set_review_keep_text(t("review.keep"))

    Text { text: root.review-keep-text; }
}
```

## 占位符语法

所有带变量的文案都必须使用花括号占位符（ADR-0005：禁止字符串拼接组句）：

| JSON 参数 | 模板字符串 | 结果 |
|----------|-----------|------|
| `{"count": 3}` | `"尚有 {count} 项待定，它们不会被导出。"` | `"尚有 3 项待定，它们不会被导出。"` |
| `{"state": "保留"}` | `"状态：{state}"` | `"状态：保留"` |
| `{"source": "数据采集"}` | `"来源：{source}"` | `"来源：数据采集"` |

## 覆盖范围

当前 zh-CN.json 已覆盖以下类别的文本键：

- ✅ **domain** - 共同语言章名词（校区、方案、候选等）
- ✅ **app** - 应用级文本（设置页、首屏）
- ✅ **plan** - 方案列表与卡片
- ✅ **review** - 评审工作台
- ✅ **export** - 导出与生成
- ✅ **collection** - 数据采集
- ✅ **dialog** - 弹窗与通知
- ✅ **error** - 错误消息

## 扩展新语种

添加英文只需：

1. 复制 `resources/zh-CN.json` → `resources/en-US.json`
2. 翻译所有 value 字段
3. 在 Language 枚举中添加 `EnUs` variant
4. 在 ADR-0004 语言下拉菜单中添加选项

不需要修改任何界面代码！
