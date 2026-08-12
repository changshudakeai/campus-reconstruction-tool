# Cargo 缓存治理

Windows 桌面集成测试会重复链接 Slint/Wry 依赖图；每个顶层测试文件都会生成
独立 EXE，启用调试信息时还会生成大型 PDB。不同 worktree 共用一个 target 又会
保留不同绝对路径对应的哈希代际。因此，小范围源码修改也可能让缓存快速增长。

本仓库采用“源头减量 + 自动回收”，30 GiB 是容量预算，不是开发报错门禁。

## 日常 Cargo 入口

在 Windows 本地开发时使用：

```powershell
.\scripts\cargo-managed.ps1 -- test --workspace
.\scripts\cargo-managed.ps1 -- clippy --workspace --all-targets -- -D warnings
.\scripts\cargo-managed.ps1 -- xtask ci
```

脚本会：

1. 固定 `CARGO_BUILD_JOBS=2` 和 `SLINT_BACKEND=software`；
2. 把当前 worktree 的构建产物放在当前 worktree 自己的 `target`；
3. 用 Git 公共目录中的互斥锁串行化本机受管 Cargo 命令；
4. 在命令前后统计所有 Git worktree 的 target；
5. 总量达到 24 GiB 时，优先整体回收最旧、非当前 worktree 的可重建 target，
   直到回落到 16 GiB；必要时才回收当前 worktree 的 target；
6. 默认两小时超时后终止该命令自己的进程树；独立隐藏 watchdog 还会在包装器
   被外层强制终止时停止它启动的同一 Cargo 进程实例，避免孤儿进程继续增长。

回收失败只给出警告，不会把一个成功的 Cargo 命令改判为失败。单独维护缓存可运行：

```powershell
.\scripts\cargo-managed.ps1 -MaintenanceOnly
```

## 测试体积控制

默认 `[profile.test]` 使用 `debug = 0` 和 `incremental = false`，显著压缩日常测试
PDB（MSVC 链接仍可能留下小型 PDB）。`--` 用于阻止 PowerShell 把 Cargo 的
`-p` 等短参数当作脚本参数。
确实需要源码行号时才显式运行：

```powershell
.\scripts\cargo-managed.ps1 -- test --profile test-debug <其他参数>
```

Slint/Winit 的事件循环要求每个桌面验收场景保持进程隔离，因此仍保留原有独立
测试入口。精准运行某个场景时使用原测试目标，例如：

```powershell
.\scripts\cargo-managed.ps1 -- test -p desktop-shell --test s1_13_leave_workspace_confirmation
```

`cargo xtask cache-report` 继续只读报告：20 GiB 起提示自动回收水位，30 GiB 标为
严重，但报告本身不阻断开发。
