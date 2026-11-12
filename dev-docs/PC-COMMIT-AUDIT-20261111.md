# PC 提交整理与交付来源审计

审计日期：2026-11-11，Asia/Hong_Kong。
工作树：`work/HyperHLE_ds_compat-r15-integration-20261031`。
分支：`codex/r15-translation-save-integration`。
审计开始时 HEAD：`aeb57764c79fbdd04b9102f3e42a183e204970f5`。

## 暂存与工作区

- 暂存区为空，stash 为空，没有冲突项。
- 运行时代码、启动器、安装器及本轮测试均已提交。
- 原先未提交的两份文档是 `AGENTS.md` 与 `dev-docs/R15-RELIABILITY.md`：
  修正 Ctrl 为零重力跳跃、G/B 为定位；补存 2026-11-07 的历史性能采样边界。
  本次将其独立提交，并明确历史采样不能充当当前默认策略或待办。
- 不重排、不 squash、不 reset 已有实现提交；保留交付件对应的原始提交身份。

## 最近实现提交

| 提交 | 内容 |
| --- | --- |
| `aeb57764` | 中英文编辑器帮助文字布局 |
| `937a54d9` | 中英文编辑器说明留白 |
| `39352357` | 护眼灰度默认 10/255、默认关闭；启动前闪烁提醒；双语编辑器分类和待保存修改保护 |
| `9b72d700` | 可调灰度的 GPU 护眼层 |
| `57e660de` / `e40c0103` | 默认关闭的暗降限幅候选及开场保护 |
| `cb8b727d` | 显式开启的原生闪烁诊断 |
| `e29ed994` / `5f224984` / `cb5f53a3` / `68cac417` | 统一存档编辑器与界面整理 |
| `fd5b2904` / `092cdb96` / `b5ef48d8` / `76139012` / `81959bb5` | 暂停/继续、输入释放、启动器与原生 HUD 行为整理 |
| `bf57de3d` | 综合优化默认策略 |

已额外核验下列实现提交均为当前 HEAD 的祖先：
`f56f2b2b`、`079ce08f`、`eecd2e54`、`4802fcc7`、`f1a603aa`、
`21132fd8`、`757151cf`、`b5d1ef78`、`796ebc6f`、`8287ae93`、`90a19b1c`。
这表示代码没有从提交链丢失；候选代码存在不代表全部默认开启。
已知有回归的低命中 Keyframe/Matrix inverse 候选仍按交付策略关闭。

## PC 旁支核对

`git cherry HEAD codex/cold-first-shot-20261107` 的提交均有等价补丁，
唯一例外为 `f525322c`（旧固定零重力改键）。当前 `81959bb5` 已实现
`zero_gravity_jump=CTRL` 的可重绑定版本，保留 G/B 定位别名，
不得为了让补丁 ID 相同而重新覆盖当前输入实现。

`codex/resolution-gpu-20261107` 的旁支提交均有等价补丁。
Android 和 iOS 分支是独立平台实现，不纳入 PC 自动合并。

## 历史备份和拒绝补丁

9 个未跟踪文件均与既有冻结清单逐文件 SHA256 相同：

- `src/ds_control_inbox.rs.orig`
- `src/ds_monitor.rs.orig`
- `src/ds_perf_control.rs.orig`
- `src/frameworks/opengles/eagl.rs.rej`
- `src/gles/fsr_es2.rs.orig`
- `src/window.rs.orig`
- `tools/pc-runtime/Start-QuietPerfRun.py.orig`
- `tools/pc-runtime/Summarize-PairedRun.py.orig`
- `tools/pc-runtime/Summarize-PerfRun.py.orig`

冻结依据：项目报告 `report/pc-installer-delivery-20261106/PC-ARTIFACT-EXCLUDES.json`。
这些文件原地保留，不加入实现提交，不加入安装包。
`eagl.rs.rej` 的两处修改已在当前代码落实：
`read_renderbuffer` 与 GPU present 均调用 `ds_perf_control::readback_finish_enabled()`。

## 最新安装包

- 启动器/完整实现来源：`aeb57764`。
- 原生运行时来源：`39352357`；后两次提交只改 C# 布局和测试，不改原生代码。
- 安装包清单声明的 592 项源码输入与本次审计时文件哈希全部相同。
- 包内无 `.orig` 或 `.rej` 文件。
- ZIP 只有安装 EXE 与原只读 `vandal.marker`，原发布件没有覆盖。
- 交付报告：项目 `report/pc-eye-installer-20261111/DELIVERY.json`。
- 本次整理仅提交文档，不重新构建或改变已交付安装包的二进制身份。

源代码 guard 的既有三项基线失败与旧几何测试限制仍按交付报告保留；
本次审计不将其改成通过，也不代替低配远端、真实手柄和全章节人工验收。
