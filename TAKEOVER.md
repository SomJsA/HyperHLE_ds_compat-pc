# Dead Space Legacy / HyperHLE 接手入口

> **最新 PC 交接总入口：[PC-HANDOFF-NOW.md](dev-docs/session-2026-10-25/PC-HANDOFF-NOW.md)**。用户要求立即整理交接，已停止新增实现。游戏仍在运行，禁止关游戏或抢前台。本文链接的最新交接优先于以下历史记录。

> 2026-10-25 PC current work: opening blackout **user accepted** (bd949378). User is testing nearby fog with copied production saves; do not interrupt the game. F dynamic drops and reported E QTEs accepted. New staged launcher/input/save work: [implementation and critical DLC schema correction](dev-docs/session-2026-10-25/PC-LAUNCHER-SAVE-IMPLEMENTATION.md). [Full plan and evidence](dev-docs/session-2026-10-25/PC-NEXT-PLAN.md). No computer-use. Never use old DLC bitmask mutation tools.


更新时间：2026-10-25（Asia/Hong_Kong）

用户已澄清：任务重点是让安卓继承 PC 内部纹理/画面修复，不是备份安卓。
安卓侧请先读 [PC 画面修复移植说明](dev-docs/session-2026-10-25/PC-VISUAL-FIXES-FOR-ANDROID.md)。
实际 native 移植、开局黑团根因、后台生命周期、BACK 与 FSR3 强开见
[Android PC 画面继承记录](dev-docs/session-2026-10-25/ANDROID-PC-VISUAL-INHERITANCE.md)。
其他 AI 接手 Android 时，优先读根目录
[`ANDROID-BRANCH-TAKEOVER.md`](ANDROID-BRANCH-TAKEOVER.md)，其中记录最终
包含式 APK、设备验收、构建/恢复流程和当前未完成项。
此前额外源码归档：安卓集成主线已备份并提取纹理/画面修复，源提交 `ef5ed439`。
详见 [安卓画面修复提取与备份](dev-docs/session-2026-10-25/ANDROID-VISUAL-EXPORT.md)，
包含源码、完整 Git 历史、子模块及未提交 SDL 差异；用户已确认灯带和 E/F 修复有效。

这是仓库级入口。其他 AI 或新会话应先读本文件，再按问题进入 `dev-docs/`
专题记录。旧日志中的“待验收”“候选未安装”等表述可能是历史状态，以本文件、
Git HEAD 和当前运行文件哈希为准。

## 1. 第一分钟检查

```powershell
cd E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat
git status --short --branch
git log -12 --oneline --decorate
Get-Process touchHLE,'DeadSpace Launcher' -ErrorAction SilentlyContinue |
  Select-Object Id,ProcessName,StartTime
Get-FileHash `
  E:\CodexProjects\DeadSpaceLegacy\run-host\Runtime-PC\touchHLE.exe,`
  E:\CodexProjects\DeadSpaceLegacy\run-host\Runtime-PC\DeadSpace Launcher.exe `
  -Algorithm SHA256
```

不要因为旧 PID 存在就操作或关闭进程。用户可能正在前台复现问题；先确认当前
进程、游戏是否暂停、是否正在录像，再决定是否替换运行文件。

## 2. 当前权威状态

### PC 游戏内容与兼容修复

| 项目 | 当前值 |
| --- | --- |
| 当前分支 | `codex/android-mainline-merge` |
| 最新 PC 修复提交 | `2ac477c47e87aac5d5bd83d8a985cbc0080e8729` |
| Android 集成合并 | `024f313b` |
| 合并修复 | `37f55960` |
| 运行入口 | `E:\CodexProjects\DeadSpaceLegacy\run-host\启动 Dead Space.lnk` |
| 启动器 | `run-host\Runtime-PC\DeadSpace Launcher.exe` |
| 游戏运行文件 | `run-host\Runtime-PC\touchHLE.exe` |
| 安装版游戏 SHA-256 | `1d0a9e3b3308f50546e8a9352a7830ec2866e4971991c1cc57dbb579e66b9d80` |
| 安装版启动器 SHA-256 | `b145504de089e4b96271859fd55d2abe61973593929f044aefa683e379126a5a` |

用户已实测验收：

- 结局 CG 脸上的残留头盔灯带消失；
- F 念力箱可以抓起；
- E 商店、工作台和门交互正常；
- 无重力跳跃成功，默认参数为 `6,3,4,z`，按键是 `Shift+空格`；
- 远处走廊从雾中逐渐显现正常；
- FRAME 2016 岩石缺块和错误前景岩石已修复。

仍未解决：

- 运镜进入可操作阶段时的局部环境黑场，录像帧约 `3508–3556`，核心区间
  `3524–3540`；人物、灯光、海报和 HUD 仍可见，因此全帧黑屏检测会漏报。
- 可重复读回的即时/手动存档尚未实现。F5 只是诊断快照，F9 有已知恢复缺陷。

### Android FSR3 探索

Android FSR3 是独立探索线，不能把探针结果当成 PC 内容修复，也不能把
“FSR3.1 超分器已 dispatch”宣传成完整 FSR3。

| 项目 | 当前值 |
| --- | --- |
| Android 分支 | `codex/android-fsr3-prototype-20261023` |
| 接手文档 | `TAKEOVER-ANDROID-FSR3.md` |
| 最近代码提交 | `0a527845d8cf317e85fbb2113cbf213c5c97c6af` |
| Android 接手记录 | 设备离线、第 8 章 CSV、构建前置均已提交；用 `git log` 查最新文档提交 |
| 推荐设置 | 原生 3× + FXAA + CAS 0.5，FSR3 关闭 |
| FSR3.1 超分 | PARTIAL-GO；真实 D16，但无 motion/jitter/history |
| FSR3.1 帧生成 | NO-GO |

四机实测已完成：ACE3、Xiaomi 手机、Sony 手机和 Xiaomi 平板均可安装并启动；
Android 9 的 `HardwareBuffer.isSupported` 崩溃已修复。当前
Android 启动器名为“死亡空间IOS移植版”，使用 IPA 内原游戏图标，带 ZIP 存档
导入和存档编辑器高风险提示。包含式 APK 已通过干净安装验证，内置 IPA 可自行
复制并启动；详细记录见下方专题。

完整数据、构建命令、代码地图和未提交文件说明见根目录
[`TAKEOVER-ANDROID-FSR3.md`](TAKEOVER-ANDROID-FSR3.md)，它是 Android 分支的
同一份接手入口，随分支合并进入本主线。

## 3. 权威边界

### 内容修复

PC 主线当前构建是游戏内容、渲染、输入和 PC 兼容行为的唯一权威。Android
分支基于旧基线 `d79eb59a`，其 Android 原生渲染实验可能与 PC 后来的修复
冲突。发生冲突时：

1. 以 PC 主线当前源码、提交和黄金帧为准；
2. Android 只移植 PC 已通过验收的改动；
3. 新旧行为必须用同一存档、同一相机和同一配置 A/B；
4. 不允许把“Android 能启动”当成 PC 画面修复已验证。

### FSR3

FSR3 必须继续独立命名和门控：

- 原生 present：最低回退；
- FXAA/CAS/FSR1：空间处理，不是 FSR3；
- FSR3.1 超分探针：真实 D16 已通，但无完整时间输入；
- FSR3.1 FG：默认关闭，当前不可交付。

FSR3 实验不得成为 Dead Space 内容修复的前置条件，也不得改变 PC 默认构建。

## 4. 本轮完成的关键修复

| 提交 | 内容 |
| --- | --- |
| `75587095` | 岩石深度写入、无重力跳跃、第一轮 E/F 和运行支持 |
| `fa784a92` | 解码圆形交互区域，修复 F 念力、E 商店/工作台 |
| `2ac477c4` | 换 Carrie 头时隐藏全部头盔发光层 |

### 岩石/开场

`d1feb687:600` 被误套用背景层“不写深度”规则。移出后恢复正常深度写入，
保留原始面剔除即可同时消除缺块和异常前景岩石。不要重新启用整批 no-cull、
全局深度重置、`DST_FILL` 或全局 texture-matrix identity。

### E/F

`InputRegionCircle` 是 24 字节结构，中心在 `+0xc/+0x10`，半径在 `+0x14`；
旧代码按 28 字节矩形读取 `+0x18`，把分配器数据当高度。另一个错误是
`CanInteract` 复查传入玩家变换，而原生使用目标交互节点变换。修复保留原生
启用状态和屏幕范围检查，删除隐藏目标兜底。

F 必须走目标实体自己的 action `0x10`，再进入 `OnInteract` /
`StartTelekinesis`；玩家动作 `0x06` 只有已持物时才是念力分支。

### 结局脸部灯带

原生 `GameObjectPlayable::SwitchToCarrieHead` 位于 `0x3d948`。手机分支按
名称只隐藏第一个头盔子网格，导致 `nodim_eyes_level_04` 发光层残留。补丁在
加载后、CPU/JIT 创建前，把 `0x3d974` 的条件分支改为 ARM NOP，复用原游戏
iPad2 完整隐藏路径。补丁校验 bundle、地址和 8 个指令字；不修改 IPA。

设置 `TOUCHHLE_DS_CARRIE_HEAD_FIX=0` 只用于 A/B 诊断，正常运行保持启用。

## 5. 专题文档

| 主题 | 入口 |
| --- | --- |
| 当前 PC 全量接手 | [`dev-docs/session-2026-10-25/TAKEOVER.md`](dev-docs/session-2026-10-25/TAKEOVER.md) |
| 会话审计 | [`dev-docs/session-2026-10-25/SESSION-AUDIT.md`](dev-docs/session-2026-10-25/SESSION-AUDIT.md) |
| Android 合并审核 | [`dev-docs/session-2026-10-25/MERGE-AUDIT.md`](dev-docs/session-2026-10-25/MERGE-AUDIT.md) |
| 结局脸部灯带 | [`dev-docs/session-2026-10-25/ENDING-FACE-FOLLOWUP.md`](dev-docs/session-2026-10-25/ENDING-FACE-FOLLOWUP.md) |
| E/F 交互 | [`dev-docs/session-2026-10-25/CIRCLE-INTERACTION-FOLLOWUP.md`](dev-docs/session-2026-10-25/CIRCLE-INTERACTION-FOLLOWUP.md) |
| 存档边界 | [`dev-docs/session-2026-10-25/SAVE-TEST.md`](dev-docs/session-2026-10-25/SAVE-TEST.md) |
| 开场渲染工作记录 | [`dev-docs/session-2026-10-25/WORKLOG-2026-10-25-FRAME2016.md`](dev-docs/session-2026-10-25/WORKLOG-2026-10-25-FRAME2016.md) |
| 反编译参考 | [`dev-docs/session-2026-10-25/DECOMPILE-reference.md`](dev-docs/session-2026-10-25/DECOMPILE-reference.md) |
| 本地证据清单 | [`dev-docs/session-2026-10-25/local-artifacts.json`](dev-docs/session-2026-10-25/local-artifacts.json) |
| Android FSR3 | [`TAKEOVER-ANDROID-FSR3.md`](TAKEOVER-ANDROID-FSR3.md) |
| Android 设备矩阵 | [`FSR3-ANDROID-DEVICE-TEST-MATRIX.md`](FSR3-ANDROID-DEVICE-TEST-MATRIX.md) |
| Android 移植原则 | [`PC-PARITY-PORTING-CHECKLIST.md`](PC-PARITY-PORTING-CHECKLIST.md) |
| Android 四机验证 | [`dev-docs/session-2026-10-25/ANDROID-DEVICE-VALIDATION.md`](dev-docs/session-2026-10-25/ANDROID-DEVICE-VALIDATION.md) |
| Android 启动器改版与存档导入 | [`dev-docs/session-2026-10-25/ANDROID-LAUNCHER-REDESIGN.md`](dev-docs/session-2026-10-25/ANDROID-LAUNCHER-REDESIGN.md) |
| Android 包含式 APK | `E:\CodexProjects\DeadSpaceLegacy\report\deliverables-e1306dcc\DeadSpace-IOS-Port-e1306dcc-bundled.apk` |

专题文档保留历史时间点。遇到相互矛盾的状态，以根入口、最新提交和当前文件
哈希为准。

## 6. 构建 PC 运行版

```powershell
cd E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat
$env:RUSTUP_HOME='E:\CodexProjects\DeadSpaceLegacy\tools\rust\.rustup'
$env:CARGO_HOME='E:\CodexProjects\DeadSpaceLegacy\tools\rust\.cargo'
$env:PATH="$env:CARGO_HOME\bin;$env:PATH"
cargo build --release --target-dir target-local-verify
```

启动器必须用 Windows PowerShell 5 构建，不能使用 `pwsh`：

```powershell
C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe `
  -File E:\CodexProjects\DeadSpaceLegacy\tools\build_pc_bootstrap.ps1
```

安装时必须显式传入核对过的游戏构建，不能依赖安装器的旧默认值：

```powershell
& E:\CodexProjects\DeadSpaceLegacy\tools\install_pc_runtime.ps1 `
  -GameExe E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat\target-local-verify\release\touchHLE.exe
```

替换运行文件前必须确认用户已退出游戏，或明确允许替换。

## 7. Git、证据和禁区

- 本仓库不收录 IPA、用户存档、`.dsstate`、MP4 和完整日志。
- 大证据保留在外层工程；`local-artifacts.json` 记录路径、字节数和 SHA-256。
- `target-local-verify/`、`target-verify/` 是构建产物，不提交。
- 多个 tracked 文件可能被 `git status` 标为 M，但 `git diff --numstat` 为
  零内容差异；不要批量暂存或强制还原。
- `vendor/SDL` 有既有 Android 线程栈修改，与 PC 修复无关；原 diff 存在
  `dev-docs/session-2026-10-25/git-state/vendor-SDL-working.patch`。
- 不执行 `reset --hard`、`checkout --`、`clean -fd` 或 `git add -A`。
- 未经用户明确要求不推送远端。

## 8. 下一步顺序

1. 定位并修复开场局部黑场，继续使用用户验收标准，不以全帧 blackdetect
   为零作为结论。
2. 实现原生手动检查点和独立恢复入口；F5/F9 不能冒充即时存档。
3. 设备恢复后完成 Android 启动器布局、预设持久化和横屏分辨率矩阵。
4. Android motion/jitter/history/HUD 输入完成前，FSR3 保持实验档。
