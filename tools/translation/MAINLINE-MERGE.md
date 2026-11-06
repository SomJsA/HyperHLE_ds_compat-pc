# 主线合并说明：校订翻译、教程与 RIG 显示

交接日期：2026-10-31（Asia/Hong_Kong）。本文供主会话完成当前 BUG 修复后执行合并。

## 1. 交付状态与合并边界

本轮已在独立 PC / Android 工作树完成集成、构建和测试。正式主工作树、正式包存档没有被写入；目前尚未把功能挑选进主线。

测试二进制对应以下代码提交：

| 平台 | 经验证的代码提交 | 功能分支 | 隔离基线，勿合入 |
|---|---|---|---|
| PC | `dec32d9ecb153e0e1e22ab5899ee44c020a0c8f4` | `codex/translation-tutorial-20261030` | `c8722f34d7fdf95b6e79213ad7ee4ea72a534b81` |
| Android | `a94331bcf35906b4c3e08d9b60060dacc8376a9d` | `codex/translation-android-20261030` | `f2d74200aa0364ce63279ddbdc210a8ae3b90522` |

前置 `Snapshot…` 提交保存了当时主工作树的未提交状态。**仅挑选下面的功能提交及其后的文档提交，不直接 merge 整条实验分支。** 后续文档提交不改变上述二进制对应的代码。

2026-10-31 07:20（香港时间）只读检查时，PC 主工作树 HEAD 为 `f5a742cfb29443ce39812f66336e9ae6999210cd`，Android 为 `04a93ce54079c6b2103efb3078eb0d6bc456840d`；两者均有未提交项。当前三方文本合并检查无冲突，但整合后的主线仍须重新构建、验证。

## 2. 用户已经确认的最终行为

| 项目 | 最终选择 |
|---|---|
| 翻译 | 台账 F 列当前校订稿，996 个 ID，845 项不同于旧中文；仍标识为校订中 |
| 教程 | PC 接管说明和按键图形，读取实际键鼠 / 手柄映射；Android 保留触控说明和手势图 |
| 章节 | 第 X 章 / 中文章名 / 英文章名；第、章放大，数字约增 5% |
| 章节字体 | 中文狮尾快剪黑子集，英文 Russell Square；章名首字红粗，其余白色 |
| 章节字距 | 原字体间距外追加 `0.10em`；十二章各语言保持统一的最大单行字号 |
| 投影 | 章节 `segmented`，任务 `scan`；背景先出现，文字约 220 ms 后接入 |
| 干扰 | 正常 → 抖动 → 乱码 → 恢复 → 短暂消失 → 恢复 → 偏移重影 |
| 长句默认 | Android `paged` 大字分段；PC `three` 整段三行；两种均可选择并保存 |

分页使用原生片段总时长与剩余时间，并按页面文字权重分配切换位置。暂停冻结，片段结束收起；未知时间回退三行。它没有进行逐句波形对齐，HTML 演示时长也不等于实际音轨切点。

## 3. 提交顺序

### PC：依次挑选 3 条

1. `a2af13e2918b0abe7220bd048f8730306447ce30`：翻译、原生文本资源、章节字体 / 干扰 / 投影、PC 教程接管及后台测试入口。
2. `cf3df10509e685e698518f9bc4eeff770f9b1d28`：确认后的字距和首行比例，章节 / 任务各自默认投影，长句 HTML 比较。
3. `dec32d9ecb153e0e1e22ab5899ee44c020a0c8f4`：原生时间驱动分页、三行模式、启动器选项，字幕结束标志修复。

### Android：依次挑选 4 条

1. `28d1222c17854da554c795bfc1305160ecbf0be5`：翻译、字体、章节 / 投影 / 干扰及测试包显示名。
2. `ba332371052d6c5a2ae5579f42698b56b0f31b26`：确认后的字距、首行比例与默认投影。
3. `a14e6a29353b386a623808cfbadfa4e5db20e8bd`：两种长句模式、原生时间 JNI、设置保存和结束标志修复。
4. `a94331bcf35906b4c3e08d9b60060dacc8376a9d`：设置返回时按原生事件继续同一关卡；设置面板从顶部显示模式选项。

本文件与 [本轮经验](LESSONS-2026-10-31.md) 是后续文档提交。交付目录的 `mainline-handoff.json` 记录了两个文档提交的准确 ID；也可通过 `git log --oneline -- tools/translation/MAINLINE-MERGE.md` 查询。若功能已合入，仅补入文档提交即可。

## 4. 建议的执行步骤

1. 主会话先完成并提交当前 BUG 修复，记录目标 HEAD；保留其现有构建和资源备份。
2. 从该 HEAD 建新的整合分支 / 工作树，给它独立的构建输出目录，再挑选本轮功能提交。
3. 处理冲突时保留主会话新增修复，只合入本轮必要增量；随后挑选文档提交。
4. 生成修订 IPA，配置两端默认项，在整合分支重新构建并执行第 7 节验收。
5. 验收后再按主项目惯例把整合分支合入主线，并记录结果。

以下变量应指向**新建的、干净的整合工作树**：

```powershell
git -C $pcIntegration cherry-pick a2af13e2918b0abe7220bd048f8730306447ce30 cf3df10509e685e698518f9bc4eeff770f9b1d28 dec32d9ecb153e0e1e22ab5899ee44c020a0c8f4
git -C $androidIntegration cherry-pick 28d1222c17854da554c795bfc1305160ecbf0be5 ba332371052d6c5a2ae5579f42698b56b0f31b26 a14e6a29353b386a623808cfbadfa4e5db20e8bd a94331bcf35906b4c3e08d9b60060dacc8376a9d
```

若提交对象不在当前仓库，可使用交付目录中的累计 mbox 补丁，通过 `git am --3way` 应用；挑选与补丁二选一。累计补丁包含后续文档提交，具体提交清单以 `mainline-handoff.json` 为准。

正在处理冲突时使用 `git cherry-pick --continue`；放弃尚未完成的一次挑选使用 `git cherry-pick --abort`。保留主会话未提交工作，避免用整个实验文件覆盖主工作树。

## 5. 重点审阅的交叉位置

| 文件 / 范围 | 合并时保留的约束 |
|---|---|
| `src/ds_host_overlay.rs` | 保留主线 CPU / GPU 呈现和回读修复，再合入字幕排版 / 背景缓存；当前主工作树在此已有新改动 |
| `src/frameworks/opengles/gles_guest.rs` | 保留主线渲染和输入时序，追加必要 observer；当前主工作树在此已有新改动 |
| `src/window.rs` | 保留主会话延迟诊断输入修复；提示文字与输入共用映射；隐藏测试仅在显式环境开关下启用 |
| `src/ds_subtitle_overlay.rs` | 两平台基线不同，保留各自原有 track / 空白片段策略，再合入时间、finished 和 JNI 增量 |
| Android `MainActivity.java` | 保留 SDL 生命周期、退出确认与退出看门狗；新恢复只针对进入菜单前正在运行且所有者未变的关卡 |
| Android `SubtitleOverlay.java` / `CaptionPresentation.java` | 保留字号、位置、颜色、透明度和外部样式；设置中途切换沿原生进度选页 |
| 两端字幕样式 JSON | 平台默认有意不同：PC `three`，Android `paged`；章节 `segmented`，任务 `scan` |
| PC 启动器 | 保存 `subtitle_mode`；本地 `overlay-style.json` 优先于旧工作台路径，避免加载其他工作区的样式 |

## 6. 资源与构建接入

### 原生文字资源必须同步

**2026-11-01 更新：R15 默认在 VFS 挂载新版 SBIN，直接使用不变的原运行基底 IPA。**
PC 与 Android 的挂载实现及回退见 [PATCH-MODEL.md](PATCH-MODEL.md)。
以下派生 IPA 命令和哈希是早期可选打包记录，不是当前发布步骤；未经用户明确确认不生成派生 IPA。

只更新覆盖层 JSON 会漏掉原生菜单、物品等文本。源码含 `res/overlay/deadspace_iphone.bin` 和可移植补丁器：

```powershell
python tools/translation/Apply-RevisedIpa.py --input $originalIpa --output $revisedIpa
python tools/translation/Apply-RevisedIpa.py --input $revisedIpa --verify-only
```

`$originalIpa` 取主线当前使用的原版，`$revisedIpa` 指向新的输出文件。验证输出应报告 `revision: revised`；原版也可通过 verify-only，但会报告 `original`，不能仅凭命令成功判断已修订。补丁器拒绝未知中文资源哈希和覆盖已有输出。

仅替换 IPA 成员：

`Payload/Dead Space.app/published/stringdata/CHS_CN/deadspace_iphone.bin`

| 资源 | SHA256 |
|---|---|
| 原版中文 SBIN | `4732db2574300a5407b783c72e0caa6002e961441379cbb86ef5780fa06d36f0` |
| 本轮中文 SBIN | `e779145bb9c2415d23630bd844c141e7c208090e01f4a8e24193eecff6a2db9c` |
| 本轮测试 IPA | `fdcfb731e34f07ba218ddce8fc63ccc09101afd5a9c1e807172a781e74d37bb2` |

重新封装时整份 ZIP 的哈希可能不同，关键是只改上述成员、其资源哈希正确且其余成员内容保持一致。Android 将生成的 IPA 放入 `android/app/src/main/assets/touchHLE_bundled/DeadSpace.ipa`，把该整份文件的实际 SHA256 写入同目录 `bundle.id`；PC 启动器指向修订 IPA。

字幕原有 995 个索引保持稳定，只在末尾加入 `UNLOCK_NEWGAME+_NIGHTMARE`。同步 `src/ds_overlay_texts.json`、`src/ds_subtitle_ids.txt` 与 Android `overlay/subtitles-original.json`，保留占位符和台账“校订中”状态。

### 样式和选项优先级

- PC：逐条 `overrides[id].style.longTextMode` → 启动器环境变量 `TOUCHHLE_DS_SUBTITLE_MODE` → 字幕层 `longTextMode` → `three`。
- Android：保存的 `overlay_display.long_mode` → 当前样式层 `longTextMode` → `paged`。
- PC 关闭背景测试开关后正常启动；合并时不把诊断环境变量写成普通启动默认。
- 字体及来源文件一并带入；Russell Square 的分发授权说明由用户附带，来源记录不代替授权文本。

### 用主线工具链重新构建

在配置好的 VS x64 / Rust 开发环境内，PC 至少运行：

```text
cargo test --release --offline -j 4 --lib ds_
cargo build --release --offline -j 4 --bin touchHLE
```

用 `tools/pc-runtime/Build-Launcher.ps1 -OutputDirectory <整合输出目录>` 重建启动器。Android 沿用主线 NDK / JDK / Gradle 设置，重新构建 Java 与 Rust SO。

实验区 `build-pc.cmd`、`build-android.ps1` 等脚本含绝对工作目录，只作为构建参数参考。`prepare_lab.py`、`integrate_*.py`、`merge_main_window.py` 是过程脚本，不在主线上直接重跑。源码和资源通过提交 / 补丁合入后，再采用主线的构建入口。

测试使用 `.translationlab`、arm64-v8a、独立存档；正式构建沿用主线包名、签名和 ABI / SDK 策略。实验区的生成 JNI 库、目录联接替换造成的 `D` 状态、整份 IPA / APK 均未进入功能提交。

## 7. 合入后的必要验收

| 检查 | 通过条件 / 已有证据 |
|---|---|
| 构建与纯策略 | PC 本轮 130 项通过；Android 时间与恢复条件 6 项通过；整合后重新运行相应检查 |
| 资源 | 修订 SBIN 哈希正确，Android `bundle.id` 匹配打包 IPA，原字幕索引和占位符保持稳定 |
| 字体 / 排版 | 两个字体真实加载；第 / 章与数字比例正确；十二章统一单行字号，字距 0.10em，首字红粗 |
| 默认值与保存 | Android 首次默认 paged，PC 无旧设置时默认 three；切换后能保存 / 读取；旧用户已保存选择仍保留 |
| 分页 / 三行 | 118 字最长条目两种模式完整显示；页面拼接不丢字，三行模式不超过三行 |
| 原生时间 | 页面按当前片段进度切换；暂停不推进；恢复沿原位置继续；结束后无残留字幕 |
| Android 返回 | 设置返回日志有同一关卡的 `DScaption-menu … HUD 2->0`；原本暂停或关卡已变时不误恢复 |
| 恢复后触控 | 返回后实际移动，教程能从移动进入视角；触控文字与图形保持原版方式 |
| PC 教程 | 默认与交换后的映射，文字 / 按键图形 / 实际动作一致；原生教程进度仍能完成 |
| 回退 | 关闭覆盖层或遇到不支持的 native 签名时可用原生显示，未知字幕可回退 |
| 性能 | 记录配置和测试场景，检查首次排版与稳定帧；避免把 FPS 上限隐藏的余量当作零开销 |

本轮 PC 暂停实测：约 5.75 秒墙钟中，原生进度推进 2.633 秒。Android 两次设置操作约 43.203 秒，原生进度推进 2.751 秒；返回后完成移动到视角教程。恢复、结束画面及记录位于本机交付目录。

现有 6.31 / 6.35 ms 对比只测量固定场景的干扰开关差异，不能据此承诺主线全部关卡或所有显示功能的最大开销。尚未逐章通关，也未进行实体手柄全流程测试。

## 8. 回退与完成记录

尚未完成的挑选可 abort。若已合入共享分支，采用正常 revert 流程按反序回退本轮代码提交，并保留同期 BUG 修复；文档提交可保留为历史记录。PC 反序为 `dec32d9e`、`cf3df105`、`a2af13e2`；Android 为 `a94331bc`、`a14e6a29`、`ba332371`、`28d1222c`。

资源与二进制按同一版本回退：恢复上一份打包资源 / 启动器路径，再重建对应程序。测试存档和测试包偏好不迁入正式包。

整合完成后记录：目标主线原 HEAD、实际挑选后提交、资源哈希、构建配置 / 二进制哈希、复跑结果、冲突处理和仍待验证的关卡。文本无冲突只表示合并检查通过，正式验收以整合分支的新构建为准。

本机交付入口：`C:/Users/somjs/Documents/ChatGPT/DEMObile/translation-lab-20261030/delivery/DELIVERY.md`。机器可读索引为同目录 `mainline-handoff.json`；本轮复盘见 [LESSONS-2026-10-31.md](LESSONS-2026-10-31.md)。
