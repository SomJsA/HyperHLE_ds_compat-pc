# Android 接手文档（2026-10-26）

> 后续实机录像暴露了尚未覆盖的甲板扫描线和远处站体损坏：游戏会用自己的
> D16 深度附件替换已创建的 D24。alpha mip 的旧验收仅覆盖部分镜头。
> 最新调查、修复和最终包验收见 [ANDROID-DECK-DEPTH24-FIX-20261026.md](./ANDROID-DECK-DEPTH24-FIX-20261026.md)。

本文面向直接接手 Android 工作的 AI 或开发者。范围以小米平板
`2405CRPFDC`（`10.31.0.232:42417`）为准；用户已明确要求“只测小米平板，
其他设备不着急，修一台等于都修”。

## 1. 当前结论

| 项目 | 结论 | 证据 |
| --- | --- | --- |
| 开局极小正值雾 | `pass` | 实机日志命中 range `10..25`；过场环境恢复 |
| 伪 4× MSAA 黑屏 | `pass` | 驱动实际 1×，安全回退；世界恢复 |
| 命中血迹深度 | `pass` | 实机命中敌人；`enabling scene depth` 日志 |
| 命中血迹半尺寸 | `pass`（本次 client-array 样本） | `applied PC hit-blood half-size quads`；红色喷雾实拍 |
| HOME 15 秒恢复 | `pass`，连续 3/3 | 同 PID、完整前后台事件、无 ANR/panic |
| 游戏内返回启动器→重进 | `pass`，连续 3/3 | `:game` PID 被替换，Launcher 保持响应 |
| ZIP 存档导入 | `pass` | 导入 10/12 个 `.sb` 按包内容计数；导入前自动备份 |
| 扫描线/船体暗带 | `pass`（小米平板；其他设备未测） | 定位 `0712153f:11427`；alpha-aware CPU mip 同机位 A/B 消除主要条带 |

小米平板的扫描线条带已闭环，但仍不能把本表扩写成“其他设备也已验证”。
血迹也只证明了本次实机命中的 client-array 路径，没有故意制造 VBO/EBO
血迹批次。

## 2. 候选安装包

- 路径：
  `android/app/build/outputs/apk/release/app-release.apk`
- 大小：`593763543` 字节
- SHA-256：
  `F7F0B8E7DD84E7F0DEA22FCA1D53EDCE5E22E089BFA107395A247D1B237EF713`
- APK 已包含 bundled IPA、原游戏图标和当前 Launcher UI。
- 构建日志：
  `report/xiaomi-pad-scanline-20261026/build-final-alpha-mips.log`
- 可直接交付的副本：
  `E:\CodexProjects\DeadSpaceLegacy\dist\android-xiaomi-20261026\死亡空间IOS移植版-Android-arm64-20261026-alpha-mips.apk`
- `apksigner` 验证：v1/v2 均通过；APK 内含
  `assets/touchHLE_bundled/DeadSpace.ipa` 和
  `lib/arm64-v8a/libtouchHLE.so`。

构建命令：

```powershell
.\tools\build-android-fsr3.ps1 `
  -JdkHome 'C:\Program Files\Java\jdk-19' `
  -AndroidSdk 'E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3' `
  -GradleBat 'E:\CodexProjects\DeadSpaceLegacy\tools\gradle-8.11.1\bin\gradle.bat' `
  -RustRoot 'E:\CodexProjects\DeadSpaceLegacy\tools\rust' `
  -GradleUserHome 'E:\CodexProjects\DeadSpaceLegacy\work\gradle-home-fsr3' `
  -BuildType release -Abi arm64-v8a -TargetSdk 24 `
  -ScaleHack 3 -FastPresent:$false `
  -BundledIpaPath 'E:\CodexProjects\DeadSpaceLegacy\work\HyperHLE_ds_compat-final-20261025\android\app\src\main\assets\touchHLE_bundled\DeadSpace.ipa'
```

## 3. 本日源码改动

### 3.1 Android native 图形

- `src/gles/gles1_native.rs`
  - 语境级 `OpeningFog` 状态和逐 draw 恢复。
  - 命中血三种内容 key 的 scoped depth test。
  - client array 血迹 quad 绕自身中心缩放到 `0.5`。
- `src/gles/ds_blood_size.rs`
  - 从 PC 已验证实现复用的 quad 缩放逻辑。
- `src/frameworks/opengles/eagl.rs`
  - 只接受驱动实际创建成功且 `samples > 1` 的 MSAA FBO；否则安全回退 1×。
- `src/window.rs`
  - Android 终止事件覆盖未消费生命周期事件，避免正常销毁路径 panic。
- `android/app/src/main/java/org/touchhle/android/MainActivity.java`
  - debug logs 不再开启高频 `TOUCHHLE_DS_TEXTURE_TRACE`。
  - 返回/退出前释放手动暂停。
  - `onDestroy()` 安排 2 秒宽限后回收卡死的游戏进程。
- `android/app/src/main/AndroidManifest.xml`
  - `MainActivity` 运行在 `:game` 进程；Launcher 留在默认进程，避免
    SDL 原生线程卡死连带 Launcher 输入 ANR。
- `src/frameworks/opengles/gles_guest.rs`
  - 新增 `TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH`：按内容 key 从 level 0
    生成 alpha 加权 mip 链，避免透明区黑底污染船体颜色。
  - 只对精确命中的 PVRTC 批次生效，生成后仍忽略游戏随后上传的坏 mip。
- `src/lib.rs`
  - Android 默认启用 `0712153f,24bec7ad,60bb1a47,e0549cfb,02d3486c`
    五个开场船体/站体批次的 CPU mip 修复。

### 3.2 Launcher

- 标题：`死亡空间IOS移植版`
- 副标题：`让我们把泰坦空间站烧成灰。`
- 使用原游戏图标。
- 右下人员信息：
  - `键鼠测试：天雷本雷`
  - `剩余全部工作：SomJs`
- 存档编辑仍明确标为“计划中”，风险等级 1。

## 4. 小米平板实测记录

设备：

```text
model: 2405CRPFDC
Android: 14
serial: 10.31.0.232:42417
```

### 4.1 后台恢复

`HOME` 保持 15 秒，再通过最近任务返回：

- 连续 3 次 `beforePid == homePid == afterPid`。
- 每次都收到：
  - `app-will-resign-active`
  - `app-will-enter-foreground`
  - `app-did-enter-foreground`
- 无 `FATAL EXCEPTION`、panic 或新 ANR。
- 画面从原电影/游戏段继续。

证据：`resume-pass1-clean-small.jpg`、`resume-pass2-small.jpg`、
`resume-pass3-small.jpg`。

### 4.2 返回启动器 ANR

修复前，`SDLActivity.onDestroy()` 永久停在 `Thread.join()`；ANR 栈同时显示
`MainActivity.onDestroy`。修复后：

- Launcher 和游戏分别出现默认进程与 `:game` 进程。
- 返回 Launcher 后 `:game` PID 消失。
- 再次启动创建新的 `:game` PID。
- 连续 3 轮输入均有响应，无 `Input dispatching timed out`。

证据：`process-isolation-pause-small.jpg`、
`process-isolation-launcher-small.jpg`、`relaunch-after-fix.xml`、
`dropbox-data-anr.txt`。

### 4.3 命中血迹

进入“5 分钟杀敌 / 矿工的威胁”，敌人出现后点击屏幕开火：

```text
Android native: enabling scene depth for PC hit-blood particles
Android native: applied PC hit-blood half-size quads
```

截图可见红色命中喷雾；没有出现：

```text
Android native: blood size fix kept original VBO/EBO draw
```

证据：`blood-fire-center-spam-small.jpg`、`blood-combat-proof3.mp4`、
`blood-combat-proof3-contact.jpg`、`blood-native-proof.log`。

### 4.4 扫描线

已定位到小米平板的亮船体像素由：

- `0712153f:11427`
- `24bec7ad:8643`
- `60bb1a47:4503`
- `e0549cfb:1224`
- `02d3486c:405`

等批次覆盖。普通 `GenerateMipmapOES` 会在 RGBA PVRTC 的透明黑区产生颜色污染，
形成彩色/黑色水平条带；仅加 GPU mip 还会出现新的块状黑斑。

最终修复从 level 0 生成 alpha 加权 CPU mip：

- 颜色按 alpha 加权平均；
- alpha 单独平均；
- 全透明 2×2 区域保留平均色，避免黑边扩大；
- 完整 mip 链上传后再设为 `LINEAR_MIPMAP_LINEAR`；
- 标记纹理为已替换，后续游戏坏 mip 不再覆盖。

实机 `scale_hack=1` 与 `scale_hack=3` 的同时间轴 A/B 均显示主要重复横纹消失，
星场、岩石和字幕未出现整块缺失。当前默认构建无需诊断环境，启动日志稳定命中
5 个 `alpha_aware_cpu=true` 纹理。

曾证伪或单独无效的方向：

- 普通全局 PVRTC mip 重建；
- 各向异性；
- clamp；
- LOD bias；
- 全局无深度；
- 全局不剔除；
- 无限远投影。

主要证据目录：
`report/xiaomi-pad-scanline-20261026/`。关键文件：

- `scale3-no071-vs-cpumip-t12.jpg`
- `scale3-mip-matrix-t12.jpg`
- `final-default-scale3-t12.png`
- `final-default-scale3.log`
- `final-alpha-mips-opening.mp4`

## 5. 存档恢复与测试陷阱

本次测试前备份：

```text
report/android-3device-validation-20261025/save-backup-before-pc-checkpoint
```

打包后的恢复 ZIP：

```text
report/android-3device-validation-20261025/restore-before-pc-checkpoint-20261026.zip
SHA-256: E891BFDF0EBF85C1B7997F26BD302AFD7CFFA014CC272F9C29738C76AA6D2C5F
```

必须通过 Launcher 的 ZIP 导入恢复，不要在 ADB 中直接把 shell-owned `.sb`
覆盖到 `Documents/var`；否则会因 ownership 不一致产生 `EACCES`。导入后逐文件
比较 SHA-256，并确认文件所有者回到应用 UID。

测试 PC 存档时可按以下顺序进入战斗：

1. 等标题页出现，点“开始”。
2. 选一个档案，再点右下确认。
3. 选“5 分钟杀敌”。
4. 选“矿工的威胁”，再确认。
5. 点击提示页继续，等待敌人出现。

## 6. 接手下一步

1. 先处理扫描线分型；在闭环前保持 `fail`。
2. 若后续任何血迹批次出现 `kept original VBO/EBO draw`，需要在
   `gles1_native` 建立 buffer CPU shadow；ES1 没有可安全读取 VBO 的
   `GetBufferSubData`，不要用 `MapBufferOES` 的 write-only 映射赌博读取。
3. 对 return-to-launcher 和 exit 各做冷启动、HOME、强制终止压力测试。
4. 对 Launcher 的人员信息、标题、图标、无 touchHLE picker、返回确认和 ZIP
   导入做最终截图回归。
5. 最终交付前恢复测试前存档，重新计算 APK SHA-256，并明确暂存相关源码和文档；
   禁止 `git add -A`，工作区存在大量既有格式化、生成库和 `vendor/SDL` 变更。

## 7. 证据索引

根目录：

```text
E:\CodexProjects\DeadSpaceLegacy\report\android-3device-validation-20261025
```

重点文件：

- `resume-pass1-clean-small.jpg`
- `resume-pass2-small.jpg`
- `resume-pass3-small.jpg`
- `process-isolation-pause-small.jpg`
- `process-isolation-launcher-small.jpg`
- `relaunch-after-fix.xml`
- `blood-fire-center-spam-small.jpg`
- `blood-combat-proof3.mp4`
- `blood-combat-proof3-contact.jpg`
- `blood-native-proof.log`
- `final-validation-opening.mp4`
- `final-opening-contact2.jpg`
- `allimport.xml`
- `title-after-fullimport.jpg`
- `start-submenu.jpg`
- `dropbox-data-anr.txt`
