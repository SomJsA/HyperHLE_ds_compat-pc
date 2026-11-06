# 平板独立包：存档冷切换与剧情点位对照

2026-10-31，Asia/Hong_Kong。分支 `codex/save-structure-20261031`。

本轮实际安装包为 **`org.touchhle.android.savelab`**，显示名称“死亡空间 · 存档实验室”。
设备为小米 `2405CRPFDC`，Android 14。两条 ADB 连接对应同一平板，操作明确绑定
`10.31.0.232:42417`。原 `frontendnav` 包的 15 个 Documents 文件在测试前后哈希完全一致。

## 七组实际画面

每组独立停止实验包、核验并替换 Documents，再用控制信箱操作原生菜单，加载槽 00。
到达 HUD 后截图、静置 8 秒、再次探针并截图；未移动角色，未自动通关。HUD state=0
只能说明检测到玩家状态，不能据此称为正确进度或正确场景。

| 组 | 实际填入方式 | 画面观察 |
| --- | --- | --- |
| A | 平板原状副本，未编辑；`level_id=01_gov_01`，仍含矿区演员/任务 | 黑色空场景，角色和 HUD 仍在 |
| B | 原始 2012 槽 00 配套检查点/章节档，`level_id=02_mines_02` | 可见矿区结构，对照 |
| C | B 的 Checkpoint/Level 仅改 `current_level/level_id=01_gov_02` | 黑色空场景，角色和 HUD 仍在 |
| D | B 的 Checkpoint/Level 仅改 `player/transform/9,10,11` 为各自原 02 槽坐标 | 黑色空场景，角色和 HUD 仍在 |
| E | B 的检查点 `triggers.count:1→0`，其他字段不改 | 可见矿区结构；后续任务/触发未验证 |
| F | 原 02 槽的 `current_level/actors/objectives/triggers/misc/player/transform` 配套移植到 B | 可见政府区门厅，保留原 00 槽装备 |
| G | 原 02 槽的 Checkpoint/Level/Survival 整套改为槽 00 文件名 | 可见政府区门厅，使用原 02 槽装备 |

检查点坐标：B 为 `(-12.7792759,1,5.3406677)`，F/G 为
`(19.7084007,0.99999994,-44.0499573)`。变换是 12 个 float32 元素，XYZ 位于索引 9/10/11；
不能只当一个关卡字符串处理。D 的 Level 文件采用其对应 donor 的实际坐标，完整值在字段 JSON。

拼图每格写明实际填入字段、来源槽和关键值；完整逐字段变化按 Checkpoint/Level/Survival
文件分别记录。差异以 B 为基准，A 相对于 B 有差异并不表示改写过原平板档。

- [拼图 A–D](C:/Users/somjs/Documents/ChatGPT/DEMObile/save-lab-20261031/android-experiments/montages/story-comparison-1.jpg)
- [拼图 E–G](C:/Users/somjs/Documents/ChatGPT/DEMObile/save-lab-20261031/android-experiments/montages/story-comparison-2.jpg)
- [全部字段、拼图与原始截图](C:/Users/somjs/Documents/ChatGPT/DEMObile/save-lab-20261031/android-experiments/story-analysis.zip)

直接结论是：**仅改关卡 ID 或仅移植坐标，会出现本次看到的黑色空场景；配套移植可到达目标
门厅。** 尚不能证明 F/G 在所有任务、脚本和后续检查点上都可继续。E 清空队列后的后续剧情
同样未验证。F 保留了原玩家状态，未移植所有教程、已应用奖励等上下文，不能当成通用无损跳关。

## 本轮新增能力

- `adb-pull/adb-activate`：完整 Documents 双次读取核对、按文件验证、应用专属 UID 写入、
  上传前备份、设备保留 rollback 树、切换后验证。写入严格限制到精确包名 `.savelab`。
- `progress-export/progress-import/progress-transplant`：剧情锚点导出、导入、跨档移植，保留
  目标玩家装备；拒绝不兼容结构和额外模块路径。
- `slot-copy`：三个槽内文件整套复制，或 `--story-only` 配套移植 Checkpoint/Level 锚点；
  保留目标共享设置与成就，输出新的配置副本。
- Android ZIP：SBIN v2 区段长度/FNV-1 校验、重复路径拒绝、唯一导入标识、导入互斥、
  运行中游戏守卫和 ADB 操作锁；保留已知版本/账户依赖。这里只验证容器，不重复实现完整语义编辑器。

原生 ZIP 实测导入 **12 个 SBIN + 2 个已知依赖**，逐文件内容一致；`adtrackingevents`
仍被旧式 ZIP 白名单过滤。ADB 整组快照保留全部 15 个文件，包括这个未知普通依赖。
损坏 SBIN、ADB 切档期间导入、游戏运行中导入均在平板上被拒绝；失败导入后原 Documents 不变。

## Android 存储与构建

Android 14 上 `adb shell` 创建的外部目录不属于游戏 UID，而 `run-as` 在本 ROM 中不能直接
访问外部应用目录。实验包通过 MainActivity 的 SDL 存储路径接入，使用应用内部
`/data/user/0/org.touchhle.android.savelab/files/save-lab`，并由 `run-as` 写入同一 UID。
普通包的路径默认不变。当前适配器面向 Android 用户 0、可调试实验包，不承诺其他 ROM 的一致行为。

复用了已接受 `frontendnav` APK 的四个原生库与游戏资源，只重建 Java/Android 包装层。
源 APK SHA256 为 `7f2c0491f62e747197f9501e3ee6b41cc13b5f971f1dc76c72505b498fa0245f`，
与平板原包一致；最终四个原生库哈希均未改变。未修改 Rust 渲染/输入引擎。

最终 APK：`android/app/build/outputs/apk/release/app-release.apk`。
SHA256：`72068c4f959f5363b637f711d31576eebb3be262147866cdf03e60d9e338c38b`。
设备安装包哈希与本机一致。七组矩阵在后续 ZIP 守卫增强前拍摄，矩阵 APK 完整哈希未单独记录；
原生游戏库与 MainActivity 存储/控制设置未变，最终 APK 另做 B 场景加载通过。

初次联调的外部目录权限错误和 tar 流式读取错误，均作为环境失败单独保留在本地早期运行目录；
它们不计作 A–G 的存档结果。正式七组位于 `android-experiments/story-runs-v4`。

## 验证与当前状态

- Python：24 项通过，覆盖图编辑、剧情保留装备、完整槽复制、tar、原包写入拒绝和冷切换回滚。
- Java：`SaveArchiveCoreProbe` 通过，含依赖、校验损坏、路径重复与失败后原档保留。
- Android release：离线 Gradle 构建通过，实验包独立安装；平板原生导入与进程守卫检查通过。
- 七组实际冷加载与截图完成；其中 A/C/D 视觉异常，不能报成“七组都成功”。
- 最终 APK 已再次加载 B 对照；独立包保留 B 的配套配置，游戏进程已停止。
- 原 `frontendnav` Documents 的 15 文件哈希保持不变，原包未写入。

结构化证据：`ANDROID-SAVE-LAB-VERIFICATION.json`。原始 PNG、完整日志、备份、测试 APK
和源存档仍在本地，未纳入 Git。操作示例见 `tools/save-lab/README.md`。

下一阶段应在用户对比画面后，针对 F/G 做门、任务更新、脚本触发、死亡/重载的配套验证；
继续补齐教程、升级台、已应用模块等上下文。当前不把“进到房间”当成任意剧情点可安全跳转。
