# Dead Space Legacy 接手入口 — 2026-10-25

本文件是本轮工作的当前状态入口；优先级高于旧 WORKLOG/HANDOFF 中按时间保留的历史结论。
Android 端 PC 画面继承、开局黑团、生命周期和返回键的当前实现见
[ANDROID-PC-VISUAL-INHERITANCE.md](ANDROID-PC-VISUAL-INHERITANCE.md)。
其他 AI 接手 Android 时，先读仓库根目录
[`ANDROID-BRANCH-TAKEOVER.md`](../../../ANDROID-BRANCH-TAKEOVER.md)。
最后用户反馈：**“测试灯带问题已消失”**。结局 CG 脸上残留头盔灯带的问题已获用户实机验收。
用户随后补充：**“EF也修好了”**。E/F 交互修复也已获实机验收。即时读档和转场黑场仍未解决。

## 1. 环境与当前运行版本

| 项目 | 位置/状态 |
| --- | --- |
| 外层工程（不是 Git 仓库） | `E:\CodexProjects\DeadSpaceLegacy` |
| 实际 Git 仓库 | `E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat` |
| 分支 | `codex/android-mainline-merge` |
| 用户启动入口 | `E:\CodexProjects\DeadSpaceLegacy\run-host\启动 Dead Space.lnk` |
| 启动器 | `run-host\Runtime-PC\DeadSpace Launcher.exe` |
| 游戏运行文件 | `run-host\Runtime-PC\touchHLE.exe` |
| 当前构建归档 | `builds\ending-head-circle-20261025\touchHLE.exe` |
| 原始 ARM 二进制 | `device-app\extracted\Dead Space.app\Dead Space` |
| 原始资源 | `device-app\extracted\Dead Space.app\published` |

相对路径除特别标注外均相对外层工程。仓库中的诊断脚本/文档副本不是运行目录。

已核对当前游戏 SHA256：
`1d0a9e3b3308f50546e8a9352a7830ec2866e4971991c1cc57dbb579e66b9d80`

启动器 SHA256：
`b145504de089e4b96271859fd55d2abe61973593929f044aefa683e379126a5a`。

当前安装状态见 [ending-head-candidate-manifest.json](ending-head-candidate-manifest.json)。
2026-10-25 02:37 +08:00 核对时，游戏 PID 28128 正在运行，启动时间 02:29:49。
这是瞬时观察，接手时要重新核对；不要沿用旧 PID 63752 或假定当前进程已经退出。
旧 PID 63752 是用户自行退出，日志为 `User requested quit`，代理没有关闭它。

用户已允许反编译；要求保留前台测试现场。不要为验证文档擅自关闭、重启、操作游戏或覆盖正在使用的运行文件。
OBS 曾封存录像并重新录制，但后来新录像停止；不能凭历史记录断言它现在仍在录制。

## 2. 状态与验收边界

| 项目 | 当前结论 | 下一步 |
| --- | --- | --- |
| 开场岩石缺块、错误前景岩石 | 已修复并验证 | 保持正常深度写入和原始剔除 |
| 远处走廊从雾中渐显 | 用户确认正常 | 不再当作待修问题 |
| 无重力跳跃 | 用户确认成功 | V 参数 `6,3,4,z`；瞄到绿色后 Shift+空格 |
| 结局脸部被头盔灯带遮盖 | **用户实测已消失，验收通过** | 保留当前修复；不必仅为本问题重打 Boss |
| F 无法吸起念力箱 | **用户确认 E/F 已修好，验收通过** | 保留当前圆形区域与原生获取路径修复 |
| E 交互问题 | **用户确认 E/F 已修好，验收通过** | 保留启用状态检查和移除隐藏目标兜底的修复 |
| 开场运镜到可操作阶段局部黑场 | **未解决** | 检查旧录像 3508–3556，尤其 3524–3540 |
| 可重复读回的即时/手动存档 | **未实现** | 研究原生序列化与独立归档、完整恢复 |

F5/Shift+F5 仅保存诊断快照。**不要按 F9/Shift+F9 来恢复宝贵现场**：旧恢复缺少其他线程和宿主图形/音频等完整状态，已有输入失效/崩溃记录。
后台检查点备份没有保存本次 Boss 进度；目前核验出的 `.sb` 仍是旧第 1 章 `01_gov_01`。

## 3. 已提交修复与技术依据

| 提交 | 内容 |
| --- | --- |
| `75587095` | 岩石深度、无重力摇晃参数、第一轮 E/F 修复及运行支持文件 |
| `fa784a92` | F/E 圆形交互区域解码、移除错误变换过滤 |
| `2ac477c4` | 换 Carrie 头时隐藏剩余头盔发光层，附证据与部署记录 |

第一轮 E/F 的 `aea05e3d…` 二进制实际验收失败，不能回退它并宣称 F/E 已修复。
`bffb6d57…` 圆形交互单独候选只曾暂存，未单独安装；当前 `1d0a9e3b…` 已包含该修复。

### 灯带：现已验收

详见 [ENDING-FACE-FOLLOWUP.md](ENDING-FACE-FOLLOWUP.md) 和 [head-before.json](head-before.json)、[head-after.json](head-after.json)。

原生 `GameObjectPlayable::SwitchToCarrieHead` 在 `0x3d948`。手机分支只按名称找第一个头盔子网格；实际模型有同名的头盔主体和 `nodim_eyes_level_04` 发光层。
快照证明换头标记已变为 1，主体隐藏而发光层仍可见。这不是脸部贴图、深度排序或整体曝光的问题。

补丁在加载后、CPU/JIT 创建前，将 `0x3d974` 的 `beq` 替换为 ARM NOP，复用原有 iPad2 完整隐藏分支。
该分支按外观与名称找子网格，12 项表 `0xa7a3d0` 使用掩码 `0xfaa` 选择 8 个头盔/发光项，保留身体、血条和静滞条。
仅对 bundle `com.ea.deadspace.bv`、指定导出符号地址及 8 个原始指令字匹配时应用。不会改写磁盘上的原始 IPA，也不改设备身份。

源码：仓库 `src/environment.rs`、`src/environment/ds_carrie_patch.rs`。
启动日志：`[DScarrie] verified 1.3.34: use complete helmet/glow cleanup on head switch`。
仅诊断 A/B 时可在新进程环境设置 `TOUCHHLE_DS_CARRIE_HEAD_FIX=0` 禁用；正常测试保持默认启用。
发布构建 5m08s；3 个补丁测试、4 个圆形区域测试、独立无窗口启动验证通过。最新视觉验收依据是用户反馈，未新增验收录像。

### F/E：已获用户实机验收

最新依据为用户原话“EF也修好了”。这是对本轮 E/F 修复的总体确认；没有额外提供逐场景测试清单或新录像，不虚构逐项操作记录。

详见 [CIRCLE-INTERACTION-FOLLOWUP.md](CIRCLE-INTERACTION-FOLLOWUP.md)。
`InputRegionCircle` 是 24 字节：中心 x/y 在 +0xc/+0x10，半径在 +0x14；旧代码按 28 字节矩形读取 +0x18，读到分配器数据，错误过滤可见目标。
此外重复 `CanInteract` 检查传了玩家变换，而原生 `OnUpdatePostCamera` 用目标交互节点变换；这项错误检查已移除。
保留原生启用状态、屏幕范围、目标主人有效性检查，删除危险的隐藏目标兜底。

F 获取物体必须走目标自己的 code `0x10` → `OnInteract` → `StartTelekinesis`。
玩家 code `0x06` 只有 state `0x12`（已持物）时才是念力分支；之前把“开启瞄准”误判为成功的结论已更正。
源码：`src/frameworks/opengles/ds_input_region.rs`、`gles_guest.rs`。

### 岩石与剩余局部黑场

岩石 atlas `d1feb687:600` 被误套入背景“不写深度”规则；移出后恢复正常深度写入，保留原始剔除即可。
不要重新启用失败的无剔除/深度重置/目标填充组合。启动器已清理相应实验开关。
另修复临时 fog override 的状态恢复；SHORT UV 的旧诊断归一化误差不是实际纹理损坏证据。

剩余黑场中人物、灯光、海报和 HUD 仍可见，因此全帧 `blackdetect` 为零不能证明问题不存在。
定位应按图像内容；录像帧号不等于 guest frame，也不要把不同运行的固定帧号当作统一机位。

## 4. 即时存档：保留的研究线索

完整测试边界见 [SAVE-TEST.md](SAVE-TEST.md)。尚未实现可用的任意时刻存取。

- `Application::GetInstance`：`0xb688`。
- `Application::SaveCheckpoint`：`0x872c`，取 layer +`0x624`。
- `LayerGameWorld::SaveCheckpoint`：`0x8272c` → `GameWorld::SaveCheckpoint`：`0x176d8`（有模式守卫）。
- `SaveGame::SaveCheckpoint`：`0xfef60`；已有笔记中的 `0xff17c` 是调用 SaveDatabase 的位置，不要未经反汇编核对当作函数入口。
- `LayerGameWorld::FadeToRestartCheckpoint`：`0x809b8` 请求状态 9。
- `GameWorld::RestartCheckpoint`：`0x194f8` 恢复内存中的 SaveGame；不能直接等同于跨进程从磁盘恢复。
- Boss 的 SetPhase/OnTrigger 有原生检查点保存调用，后续应核对触发条件、序列化内容及持久化路径。
- `tools/checkpoint-bank.py` 只监视并归档游戏自己写出的稳定 `.sb`，不会强制保存。

建议下一步推进原生手动检查点和剩余局部黑场。手动检查点需要证明同一进程和退出重启后均能恢复位置、血量、弹药、Boss 阶段，且输入/暂停正常，才算交付。
不应要求用户为已验收的灯带问题反复重打 Boss，也不应把三份 F5 文件宣称为能恢复的存档。

## 5. 证据、工具和重现

| 外层路径 | 内容 |
| --- | --- |
| `logs/ending-face-20261025/` | 三次 F5、原始截图、旧异常 OBS 视频、反汇编、离线脚本、完整日志和编译日志 |
| `logs/kinesis-visible-target-20261025/` | 念力箱可见但 F 被过滤的快照与日志 |
| `logs/interaction-lock-20261025/repeat-0107/` | 隐藏交互目标导致 E 瞬移卡住、F 只瞄准的日志 |
| `logs/frame2016-final-20261025/` | 岩石验证及 `review-partial-black-3508-3556.jpg`、原始录像 |
| `savebank/20261025-020809-paused-near-boss-bug-20261025/` | 旧第 1 章检查点备份，并非 Boss 存档 |
| `builds/ending-head-circle-20261025/startup-smoke/` | 独立启动验证结果；不代表画面验收 |
| `logs/handoff-20261025/` | 本次交接时遗留工作树原始文件备份和 Git 状态快照 |

三次 F5：`state-1792865354-f036252.dsstate`（Boss 阶段）、`state-1792865381-f037353.dsstate`（CG 前最后一下）、`state-1792865422-f039261.dsstate`（脸部异常）。
其 SHA256 与旧视频/截图在 [evidence-manifest.json](evidence-manifest.json)。旧异常视频 285.783 秒，脸部在约 90–100 秒。
**这份 manifest 描述旧版异常现场，不能把其中 runtime 哈希更新为新版。**

小型诊断脚本和 ARM 文本已备份到本目录 `diagnostics/`，基础解析器也在其中。
这是原始取证脚本快照，保留了机器绝对路径；优先在外层原位置运行，不是通用恢复工具。
示例（外层工程目录）：

```powershell
python tools/dsstate.py logs/ending-face-20261025/state-1792865422-f039261.dsstate summary
python logs/ending-face-20261025/head-cleanup-targets.py logs/ending-face-20261025/state-1792865422-f039261.dsstate logs/ending-face-20261025/head-recheck.json
python tools/arm-dis.py 'device-app/extracted/Dead Space.app/Dead Space' '@0x3d948' arm 160
```

编译（实际 Git 仓库目录）：

```powershell
$env:RUSTUP_HOME='E:\CodexProjects\DeadSpaceLegacy\tools\rust\.rustup'
$env:CARGO_HOME='E:\CodexProjects\DeadSpaceLegacy\tools\rust\.cargo'
$env:PATH="$env:CARGO_HOME\bin;$env:PATH"
cargo build --release --target-dir target-local-verify
```

安装器 `tools/install_pc_runtime.ps1` 的默认 GameExe 仍指向旧文件；必须显式传入经过核对的构建路径。
只在用户已退出或明确允许替换后部署。现已安装，不需为接手再次部署。
`tools/pc-runtime/` 是启动器、安装器与摇晃窗口脚本的版本化副本。

## 6. Git 与文件保全

本次提交只新增/更新交接记录、验收状态、诊断脚本快照和 manifest，不改已验收运行逻辑。
根目录旧 WORKLOG/HANDOFF 已同步入口和最新状态；完整反编译笔记以 `DECOMPILE-reference.md` 保存本轮快照。
大文件（exe、dsstate、mp4、图片）留在外层工程，不把构建目录加入 Git；[local-artifacts.json](local-artifacts.json) 记录取证目录的文件尺寸和 SHA256。

交接检查发现：主仓库若干 tracked 文件被 `git status` 标为 M，但 `git diff --numstat` 没有文本差异；原始字节已另存，不批量暂存或强制还原。
`vendor/SDL` 存在既有 Android `SDLActivity.java` 修改（SDL 线程栈改 16 MiB）；与本轮 PC 灯带修复无关，没有替它创建子模块提交。
其原始 diff 备份为 [git-state/vendor-SDL-working.patch](git-state/vendor-SDL-working.patch)，基准与文件哈希见 [git-state/pre-handoff.json](git-state/pre-handoff.json)。
工作树中的 `target-local-verify/`、`target-verify/` 是未跟踪构建产物。不要 `git add -A`，不要 `reset --hard` 或清理这些目录。
未进行远端 push。不要误称整个工作树干净；用 `git status` 检查最新状态。

用户要求降低上下文体积：先量图大小，超过 400 KB 先缩小再查看；不要整段导出长日志或反复看同一图片。
新接手者先读本文件及对应问题的专题记录，需要时再按帧号、符号和行号读取原始证据。
