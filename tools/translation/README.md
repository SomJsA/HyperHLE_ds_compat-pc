# 六列校订台账：隔离集成说明

本次使用用户提供的 `六列校订台账.xlsx` F 列作为当前校订稿，没有把表内备注当作操作指令，也没有将尚未校订完的稿件标记为终稿。

## 文本与资源

- 共 996 个 ID，844 项与原中文不同；占位符校验无差异。
- 保留原有 995 项字幕索引，只在末尾补入 `UNLOCK_NEWGAME+_NIGHTMARE`。
- 同时更新高清覆盖层和游戏原生 `CHS_CN/deadspace_iphone.bin`。只改覆盖层会漏掉菜单、物品等原生文字。
- 原生 SBIN 已按真实键值索引更新，避免破坏共享字符串；重新计算段哈希并通过回读校验。
- `ledger-audit.json` 保留数量、干扰 ID 和十二章藏头检查结果。

新版台账可直接只读导入：

```text
python tools/translation/Update-Ledger.py --ledger "六列校订台账 (1).xlsx"
```

脚本要求 `openpyxl` 和 `fonttools`。它会保留既有字符串 ID，只追加本轮发生变化的原生中文，
同步 PC/Android 覆盖层文本、原生 SBIN、章节字体子集和两个预览页，并拒绝 ID 集合或占位符变化。

## 显示与操作

章节为三行：`第 X 章`（数字较大）、中文章名、英文章名。两种语言分别计算十二章都不换行的最大统一字号；章名仅第一个字／字母红色加粗，其余白色。中文使用狮尾快剪黑子集，英文使用 Russell Square。按最新反馈，“第／章”增大，数字再增约 5%，中英章名在原字体间距外追加 0.10em 字距；重新适配十二章统一单行字号。旧字体文件的目录、Unicode 字表和度量兼容性已修复，字形轮廓不变；来源与原版权保留在字体说明中，用户负责随分发附带其授权说明。

PC 教程从同一份键鼠、手柄映射获取操作名称和按键图形，保留原生教程触发、进度与完成逻辑。当前键盘输入为向前自动跑动、瞄准时步行，教程照实说明。手柄零重力跳跃使用 LT + 快速转身键（默认 RS）。Android 继续使用原触控文字和手势图形。

带“干扰”标记的文字按 2.56 秒周期呈现：正常 → 抖动 → 乱码 → 恢复 → 80 毫秒消失 → 恢复 → 偏移重影。正常／恢复占多数；只改变显示，不改字幕原文、音轨或时序。

章节默认使用“分段投影”，任务追踪默认使用“扫描展开”。背景先出现，约 220 毫秒后文字接入；原生显示结束时间不变。`RIG-motion-preview.html` 是可离线打开的三版比较，包含十二章、任务、慢放、暂停和时间轴。

## 开关

覆盖层 JSON 的 `layers.chapter`、`layers.objective` 支持：

```json
{"projection":"scan","projectionBackground":true,"projectionAnimation":true}
```

`projection` 可选 `scan`、`segmented`、`minimal`。关闭背景或入场动画分别将相应布尔值设为 `false`。`layers.subtitle.signalAnimation=false` 可关闭干扰动画。PC 使用 `TOUCHHLE_DS_OVERLAY_STYLE` 指定的文件；Android 使用本应用私有目录 `files/overlay-style.json`。现有字号、位置、透明度和逐条文本覆盖继续有效。

## 原生 IPA 补丁模型

原版 IPA 保持不变。R15 在启动时把仓库中的 SBIN 补丁挂载到原有中文资源节点；
PC EXE 与 Android 原生库携带相同补丁，直接选择原版／原已接受基底 IPA 即可。
挂载前核对源资源哈希，未知版本保留原节点。读取、长度、seek、fstat 和只读保护
使用同一份补丁字节；其他资源与存档路径不变。日志以 `[DSresource-overlay]` 报告挂载。
诊断回退可设置 `TOUCHHLE_DS_NATIVE_TEXT_PATCH=0`，它不关闭独立的高清覆盖层。
更新台账后须重建运行库，详见 [PATCH-MODEL.md](PATCH-MODEL.md)。

`Apply-RevisedIpa.py` 只保留为旧运行库的可选工具，不属于 R15 发布打包流程。
生成派生 IPA 必须先取得用户明确确认；以下仅为工具用法：

```text
python tools/translation/Apply-RevisedIpa.py --input 原版.ipa --output 校订版.ipa
python tools/translation/Apply-RevisedIpa.py --input 校订版.ipa --verify-only
```

脚本只接受已知原始／上一版／本次修订的中文资源哈希，拒绝覆盖输入和已有输出。
Android 发布资源中的 `touchHLE_bundled/DeadSpace.ipa` 应保持基底 IPA 的原始字节，
`bundle.id` 记录基底文件 SHA256；新 SBIN 从原生库挂载，不回写这个 IPA。

## 性能与验证

PC 相关测试 130 项通过，Android 时间与恢复条件 6 项独立测试通过。十二章字体均已实际栅格化核对；浏览器确认两种字体成功加载、十二章可选且桌面／窄屏无水平溢出。PC 后台游戏实测移动、视角教程和任务追踪；平板独立包实测新章节、字幕与原触控教程，并完成移动到视角教程的转换。后续关卡教程目前覆盖映射和公共显示路径检查，不等于全流程通关或实体手柄逐项测试。

固定场景 ABBA 对比，PC RTX 4090、内部 640×960、显示 1080×720、解除 FPS 上限，干扰关闭／开启的平均帧时间中位数约 6.31／6.35 毫秒（约 +0.04 毫秒，+0.66%）。各模式两段、每段 18 秒；此结果仅代表该场景。平板 1920×1200、2×、30 FPS 限制下，两组均为 30 FPS，1% low 没有显示额外下降；该测试不能量出被帧率上限隐藏的全部 GPU 余量。

为控制开销，Android 章节测量仅在尺寸或样式变化时重算，覆盖层以 30 Hz 更新；PC 缓存文字和入场背景，稳定后丢弃临时背景帧，文字缓存上限 64 MiB、背景缓存上限 32 MiB。无全屏模糊、粒子或新增场景回读。

## 集成边界

PC 与 Android 各自在独立工作树开发和构建。主会话的工作目录、进程、存档未被修改。PC 基线额外吸收了主会话同期的后台诊断输入修复，三方合并后再验证。本次功能提交应单独挑选，不能把前置的“Snapshot…”基线提交一起合回主分支。

PC 隐藏测试需同时设置 `TOUCHHLE_DS_BACKGROUND_TEST=1` 和专用 `TOUCHHLE_DS_CONTROL_INBOX`；窗口从创建时就隐藏，测试输入只送入该进程。正常启动不启用这个测试模式。Android 实测包名为 `org.touchhle.android.translationlab`，与正式包数据隔离。

## 长句字幕开关与显示研究

`RIG-long-subtitle-preview.html` 使用真实台账文本比较大字两行分段、整段三行、整段压成两行。最长整条对白 Dain_01 为 118 字，最长标点分句为 53 字。网页仍用估算时长演示；游戏已读取原生 SubtitleTrack 的总时长、剩余时间，按各页字符权重在当前片段内分配，暂停时进度冻结，片段结束时收起。这不是按语音波形逐句标注的切点。原生文本与对白总时序不变。

Android 默认“大字分段”，PC 默认“整段三行”。PC 在启动器“兼容与诊断”页的“长句字幕”选择后启动；Android 在“字幕显示”设置中即时切换并保存。配置键为 `layers.subtitle.longTextMode`，值为 `paged` 或 `three`；PC 启动器还可通过 `TOUCHHLE_DS_SUBTITLE_MODE` 设置全局值。分页不可取得有效原生时长时退回整段三行。

大字分段保持两行底板高度，最多缓存 16 份排版；每帧只用原生时间选页。另修正覆盖层忽略原生 finished 标志导致最后一条字幕可能残留的问题。

## 主线交接与本轮经验

- [主线合并说明](MAINLINE-MERGE.md)：提交顺序、隔离基线、资源接入、冲突位置、验收和回退。
- [本轮经验](LESSONS-2026-10-31.md)：数据、字体、原生时钟、暂停恢复、ADB 和后台测试方法。

设置返回时新增的 Android 恢复逻辑，只对进入菜单前正在运行且 HUD / world 所有者未变的关卡派发原生继续事件；保持原有暂停状态与所有者变化的回退条件。

`text-corrections.json` records verified native UI context corrections after ledger import. `CREDITS` is Store currency, while `credits_list` is the separate staff list. The input workbook remains unchanged. Unknown future translations require review before the correction is reapplied.
