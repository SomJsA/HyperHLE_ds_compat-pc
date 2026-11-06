# Android 分支现状与接手说明

更新时间：2026-10-25（Asia/Hong_Kong）

这是 Android 当前工作的权威入口。先读完本文，再进入专题文档。旧记录里的
“设备离线”“尚未合并 PC 画面修复”等表述是历史状态，不代表当前工作树。

## 1. 一分钟结论

| 项目 | 当前结论 |
| --- | --- |
| Git 分支 | `codex/android-mainline-merge` |
| 当前 HEAD（本次副本修正前） | `a44d9782` |
| Android 画面继承提交 | `99f08ff1` |
| 主仓库 | `E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat` |
| 内容权威 | PC 主线当前构建；冲突时 PC 优先 |
| Android 渲染后端 | `src/gles/gles1_native.rs`，原生 GLES 1.1 |
| 最终包 | `report\android-mainline-20261025\DeadSpace-IOS-Port-bundled-20261025.apk` |
| 最终包 SHA-256 | `4ACDFCC8CB39E2A3EC9AC0A40F6BC3C43FB090D0AD55809918B9C7ECAD3C029B` |
| 设备结论 | 小米平板最终包通过启动器、完整开局、移动/转向、BACK、HOME/返回 |
| FSR3 | 超分探针 PARTIAL-GO；帧生成 NO-GO；默认可交付路径是原生 3x |
| 存档 | ZIP 导入可用；存档编辑器仅为计划项，`.sb` 写入有损坏风险 |

不要因为文件里存在 PC 修复代码就宣布 Android 已继承。Android 只执行
`gles1_native.rs`；`gles1_on_gl2.rs` 的实现必须逐项适配到 native 路径。
本分支已经完成该适配，并用运行日志和设备录像确认实际命中。

## 2. 当前交付与设备验证

最终 APK 是包含式包，内置 IPA：

| 项目 | 值 |
| --- | --- |
| APK 大小 | `584,966,283` 字节 |
| APK SHA-256 | `4ACDFCC8CB39E2A3EC9AC0A40F6BC3C43FB090D0AD55809918B9C7ECAD3C029B` |
| IPA 大小 | `532,322,054` 字节 |
| IPA SHA-256 | `1381CBD7DE15BCAF8085EDF8F191CC54261C00376087D4893AC62FB9B9F2DA1C` |
| `libtouchHLE.so` SHA-256 | `06887DC7B002C04E904120B3E8020BCF566DED23479DE60A08E67BF951D6121B` |
| 包名 / ABI / SDK | `org.touchhle.android` / `arm64-v8a` / min 21, target 24 |
| 签名 | APK v1 + v2，当前为 Android debug 证书 |

最终包在 Xiaomi Pad `2405CRPFDC` 上完成以下回归：

- 启动器名称、原始游戏图标、最右侧人员信息显示正确；
- 副标题为“让我们把泰坦空间站烧成灰。检查画质与存档后开始游戏。”；
- 从“启动游戏”进入难度选择，确认后完整播放到空间站、标题和舱内场景；
- 没有“超大黑团子”；同轮 logcat 命中 `LoadMatrixf` 远平面修正；
- 左侧横滑实际转向，向上滑动实际前进；
- 局内 BACK 只暂停并显示“继续 / 返回启动器 / 退出”；
- “返回启动器”确实回到 `LauncherActivity`，没有直接退出；
- HOME 后游戏进程 PID 保持 `17824`，从最近任务返回仍是同一个
  `MainActivity` PID，没有重启 IPA。

证据目录：

- `E:\CodexProjects\DeadSpaceLegacy\report\android-mainline-20261025`
- `E:\CodexProjects\DeadSpaceLegacy\report\android-3device-validation-20261025`
- 开局录像：`report\android-mainline-20261025\opening-regression.mp4`
- 开局缩略图：`report\android-mainline-20261025\opening-regression-contact.jpg`
- 文案截图：`report\android-mainline-20261025\launcher-titan-copy.jpg`

ADB 三台无线设备当前均在线：

| 设备 | 地址 | 最终包状态 |
| --- | --- | --- |
| Xiaomi Pad `2405CRPFDC` | `10.31.0.232:42417` | 已安装并完成上述回归 |
| Xiaomi `M2012K11AC` | `10.31.2.162:37753` | 在线；最终包待回归 |
| OnePlus ACE3 `PJE110` | `10.31.2.194:37953` | 在线；最终包待回归 |

不要把历史上其他设备/旧包的通过结果冒充最终包结果。

## 3. PC 画面修复继承情况

Android 默认路径已经适配并实际执行：

- `LoadMatrixf` 有限远平面识别，修正只改变 `m[10]`；
- 背景层限定 `GL_LEQUAL` 且不写深度；
- 岩石 `d1feb687:600` 保持 `GL_LESS` 和深度写入；
- 地基 `02d3486c:405` 定向关闭剔除；
- `56c62e8a` / `7027023c` 覆盖层定向去雾；
- 正交 UI 去雾；
- 海报 `25a08a82` 小批次 LINEAR 过滤；
- 纹理上传 `Finish` 同步；
- 开场 PVRTC level 0 解码并重建 mip；
- 换头残留灯带补丁和 E/F 圆心交互逻辑位于共享层。

默认实际命中日志见
`dev-docs\session-2026-10-25\ANDROID-PC-VISUAL-INHERITANCE.md` 第 10 节。

仍只存在但不属于 PC 当前默认行为的高级实验包括：

- `TEXWRAP_HASH`；
- 按三角形跳过/修补；
- 全局 NO_DEPTH、全局 no-cull、`DST_FILL`；
- texture-matrix identity；
- 全局深度 reset。

不要把这些默认关闭的实验描述成已继承修复。PC 自身仍未解决运镜进入可操作阶段
时的局部黑场（录像约 3508–3556），这与 Android 曾出现的“大黑团”不是同一根因。

## 4. 启动器与交互边界

- 应用显示名：`死亡空间IOS移植版`；
- 图标：来自 IPA 的原游戏图标；
- 视觉：深黑蓝、细青线、左侧编号栏；
- 质量入口：推荐、性能优先、兼容优先、FSR3 实验、自定义；
- 推荐默认：原生 3x + FXAA + CAS 0.5，FSR3 关闭；
- 高级参数含“强制启用 FSR3（高风险）”，会绕过能力预检，可能黑屏/闪退；
- 不再回退到 touchHLE 的桌面式 app picker；
- ZIP 存档导入会先备份 `Documents`；
- 存档编辑器未实现，只显示风险说明。

Android 不需要继承 PC 键鼠、桌面窗口或启动器行为；只继承游戏内容与已验收兼容
修复。触摸移动/转向由 Android 窗口路径处理。

## 5. FSR3 现状

| 路径 | 状态 |
| --- | --- |
| 原生 GLES present | 可交付，默认最低回退 |
| FXAA / CAS / FSR1 | 可交付空间处理 |
| FSR3.1 超分探针 | PARTIAL-GO；真实 D16，但无 motion/jitter/history |
| FSR3.1 帧生成 | NO-GO |

3x 输入只从 `960x1440` 放大到有效 `1620x1080`，静态画面与原生 3x 很接近；
这不能证明动态时间超分质量。Adreno 740 历史测试中 FSR3 也不会自动带来性能优势。
强制启用开关只用于驱动取证，不是推荐播放配置。

## 6. 构建

工具链：

- JDK 19：`C:\Program Files\Java\jdk-19`
- Android SDK：`E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3`
- Gradle：`E:\CodexProjects\DeadSpaceLegacy\tools\gradle-8.11.1\bin\gradle.bat`
- Rust：`E:\CodexProjects\DeadSpaceLegacy\tools\rust`
- ADB：`E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3\platform-tools\adb.exe`

构建前必须先补齐 checkout 的非 Git 依赖：

1. `git submodule update --init --recursive`；
2. 提供 `vendor/boost`；
3. 创建 `android/local.properties`；
4. 执行
   `tools\materialize-android-assets.ps1 -Repo <repo>`；
5. 如需完全干净包含包，先跑 Gradle `clean`。

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

构建完成后恢复三个 symlink 占位：

- `android/app/src/main/assets/touchHLE_dylibs`
- `android/app/src/main/assets/touchHLE_fonts`
- `android/app/src/main/java/org/libsdl/app`

不要提交 materialize 展开后的整个 dylib/font/SDL 树，也不要提交
`jniLibs/*.so`、`android/app/build`、`target-*`。

## 7. 工作树与 Git 规则

这个 Windows checkout 的 `core.symlinks=false`，所以 Git 中的 symlink 会以
包含目标路径的小文本文件存在。`materialize-android-assets.ps1` 会临时把它们
替换成目录，并把旧占位备份到 `%TEMP%\android-symlink-placeholders-*`。

工作树可能仍显示许多 `src/**` 文件为 M，部分是早先全仓库格式化造成的改动。
不要 `git add -A`，不要把格式化噪声、`vendor/SDL` 用户修改和生成库混入功能提交。
按明确路径暂存。

本接手包只接受以下顺序：

1. `git status --short --branch --untracked-files=no`；
2. 审核目标文件 diff；
3. 按路径 `git add -- <files>`；
4. 构建、设备验收；
5. 复制最终 APK，记录 SHA-256；
6. 再做文档提交。

不使用 `reset --hard`、`checkout --`、`clean -fd` 或 `git add -A`。

## 8. 代码地图

| 路径 | 作用 |
| --- | --- |
| `src/gles/gles1_native.rs` | Android 实际 GLES1 后端与 PC 画面修复 |
| `src/frameworks/opengles/gles_guest.rs` | 纹理内容键、上传、PVRTC mip、共享兼容逻辑 |
| `src/ds_texid.rs` | 纹理内容指纹，避免依赖会复用的 GL texture ID |
| `src/frameworks/uikit.rs` | SDL Android 生命周期事件转接 |
| `src/frameworks/uikit/ui_application.rs` | iOS 原生前后台通知语义 |
| `src/window.rs` | 后台等待与前台恢复 |
| `android/app/src/main/java/org/touchhle/android/LauncherActivity.java` | 启动器、预设、ZIP 导入、FSR3 强开 |
| `android/app/src/main/java/org/touchhle/android/MainActivity.java` | 内置 IPA、环境变量、BACK 暂停菜单 |
| `android/app/src/main/java/org/touchhle/android/Fsr3Capabilities.java` | FSR3 能力预检 |
| `android/fsr3-bridge/` | FSR3.1 Vulkan/AHB bridge 与第三方说明 |
| `tools/build-android-fsr3.ps1` | release 构建入口 |

## 9. 下一步顺序

1. 在两台手机安装最终 SHA 包，确认启动器排版、开局、BACK、HOME 和触摸；
2. 单独验证“强制启用 FSR3”只做控制透传和失败行为，不宣称画质成功；
3. 用真实章节流程复测存档写入、HOME 后恢复和后台闪退概率；
4. 实现存档编辑器前先设计备份、格式校验和失败回滚；
5. 如继续追 PC 画面差异，先比较同一机位黄金帧，不要恢复已拒绝的全局实验；
6. 实现正式 release keystore；当前 debug 签名只适合内部交付。

## 10. 专题入口

- PC 修复继承与最终回归：
  `dev-docs\session-2026-10-25\ANDROID-PC-VISUAL-INHERITANCE.md`
- PC 侧源码权威清单：
  `dev-docs\session-2026-10-25\PC-VISUAL-FIXES-FOR-ANDROID.md`
- 合并审计：`dev-docs\session-2026-10-25\MERGE-AUDIT.md`
- FSR3 历史报告：`TAKEOVER-ANDROID-FSR3.md`
- 设备矩阵：`FSR3-ANDROID-DEVICE-TEST-MATRIX.md`
- 移植原则：`PC-PARITY-PORTING-CHECKLIST.md`
