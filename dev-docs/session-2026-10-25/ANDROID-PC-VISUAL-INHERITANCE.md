# Android 继承 PC 画面修复与原生生命周期

更新时间：2026-10-26（Asia/Hong_Kong）

本文是安卓接管 PC 画面修复、后台生命周期和返回键行为的实现记录。PC
修复的权威说明由 PC 侧提交 `05853f7c` 提供：

`dev-docs/session-2026-10-25/PC-VISUAL-FIXES-FOR-ANDROID.md`

> 2026-10-25 后续状态：用户已在当前 Android 画面中亲眼确认仍有扫描线，
> 最新 PC 的开局极小正值雾与命中血迹深度/尺寸也未进入 `gles1_native`。
> 本文第 1、10 节只代表当时旧 APK 的历史基线，不再表示最新 PC 画面已全部继承。
> 当前重适配顺序、症状分诊和验收检查点以
> [`ANDROID-PC-VISUAL-PORTING-GUIDE.md`](./ANDROID-PC-VISUAL-PORTING-GUIDE.md)
> 为准。

> **2026-10-26 最新实机状态：** 小米平板 `2405CRPFDC` 上，开局极小正值雾、
> 伪 4× MSAA 黑屏回退、命中血迹深度与 0.5 尺寸、HOME 15 秒恢复和
> “返回启动器→重新启动”均已通过。扫描线仍为 `fail`。当前候选包与完整证据
> 索引以 [`ANDROID-HANDOFF-20261026.md`](./ANDROID-HANDOFF-20261026.md) 为准。

## 3.1 返回启动器 ANR 修复（2026-10-26）

旧实现先从暂停菜单返回 `LauncherActivity`，再销毁 `MainActivity`。SDL 的
`onDestroy()` 会发送 `SDL_QUIT` 后无限期等待原生线程；Dead Space 的
`AppWillTerminate` 回调又会重新进入 guest 调度器并停在
`Environment::schedule_next_thread`，导致主线程永久阻塞。Launcher 与游戏在同一
进程时，下一次按钮输入会直接触发 5 秒 ANR。

现在的行为：

1. `LauncherActivity` 保持在默认进程，`MainActivity` 放入 `:game` 进程。
2. 返回启动器时先启动 Launcher，再正常发送退出事件。
3. `MainActivity.onDestroy()` 安排 2 秒宽限；若 guest 没有自行退出，只回收
   `:game` 进程，不阻塞 Launcher。
4. 小米平板连续 3 轮返回/重进均得到新 `:game` PID，Launcher 始终可响应，无新
   ANR。

证据见 `report/android-3device-validation-20261025/process-isolation-*`、
`resume-pass*-small.jpg` 和 `dropbox-data-anr.txt` 的修复前记录。

安卓实际后端是 `src/gles/gles1_native.rs`，不是桌面 GL2 后端
`src/gles/gles1_on_gl2.rs`。因此“仓库里有 PC 修复代码”不等于安卓已经执行；
本记录只列已经适配到 native ES 1.1 并实际进入 draw 路径的行为。

## 1. 已移植的 PC 行为

| PC 行为 | Android native 实现 |
| --- | --- |
| 远平面/投影修正 | `native_adjust_frustum_far` + 每 draw 的 `native_proj_z_fix_begin` |
| 背景共面层 `GL_LEQUAL` | `native_texnodepth_begin`，按 content key、屏幕尺寸和深度状态限定 |
| 背景层 tie-break 不写深度 | `TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE=1` |
| 岩石 `d1feb687:600` 保留 `GL_LESS` 和深度写入 | native 层显式排除该 content/count 批次 |
| 站体地基反绕序批次 | `TOUCHHLE_DS_NO_CULL_HASH_COUNT=02d3486c:405` |
| 舷窗/墙面封面深度例外 | `TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT`，开局 6-index 封面仍跳过 |
| 海报小面片 LINEAR 过滤 | `native_texfilter_begin/end`，仅 `25a08a82` 且 count <= 6 |
| 正常距离雾 + 2D 覆盖层去雾 | `native_nofog_begin`、`native_nofog_ortho_begin` |
| 舷窗/窗面定向去雾 | `TOUCHHLE_DS_NO_FOG_HASH=7027023c,56c62e8a` |
| 纹理上传同步 | `TOUCHHLE_DS_TEX_UPLOAD_FINISH=1` |
| PVRTC 干净 mip | 共享 `ds_try_auto_opening_mips`，显式 `AUTO_OPENING_MIPS=1` |
| 换头残留灯带 | 共享 `src/environment/ds_carrie_patch.rs`，Android 复用 |
| alpha 纹理语义 | 共享 `gles_guest` 的 alpha expansion；Dead Space 默认关闭 tint |

所有 draw 临时状态都在 draw 后恢复，包括深度函数、深度写掩码、剔除、
纹理过滤、雾和 texture matrix。没有恢复全局 `NO_FOG=1`，也没有恢复
“岩石不写深度/不剔除/每帧清深度/destination fill”等已被 PC 拒绝的实验。

## 2. “开局超大黑团”根因与修复

本地平板复现中，黑团不是黑色贴图遮挡，也不是雾。对照 PC 最终录像后确认：
本应出现的石壁、站体和月面完全被远平面裁掉，只留下黑色 clear color。

漏掉的 PC 代码路径是：游戏有时不调用 `glFrustum*`，而是直接
`glLoadMatrixf` 上传有限远平面投影。原先 Android 只在 `Frustumf/Frustumx`
里修正 far，因此这条路径完全没有命中。

`native_proj_z_fix_begin` 现已按 PC 的条件增加逐 draw 自动识别：

```text
m[10] in [-1.006, -1.001]
m[14] in [-0.105, -0.095]
```

命中时只把 `m[10]` 从略小于 `-1` 调整为 `-1`，保留 near 映射、XY
投影和 `m[14]`，不再全局压缩整行 z。

平板实测同一镜头已恢复连续石壁、站体栏杆、星空和月面材质。证据：

- `report/android-3device-validation-20261025/pcmerge-opening60-small.jpg`：修复前黑团
- `report/android-3device-validation-20261025/pcmerge-projfix-scene-small.jpg`：修复后同镜头
- `report/android-3device-validation-20261025/pcmerge-pixel-scene.png`：单点逐 draw 诊断

## 3. Android 生命周期原生化

旧 `frameworks/uikit.rs` 在 Android `onPause` 后直接
`UIApplication exit`，等价于把 HOME 当成立即终止。现在改为：

1. `AppWillResignActive`
2. `applicationWillResignActive:`
3. `applicationDidEnterBackground:`
4. guest 主循环在后台等待，不退出进程
5. 回前台发送 `applicationWillEnterForeground:`
6. 再发送 `applicationDidBecomeActive:`

`did_enter_background` 是幂等的，避免 SDL 随后再送一次通知。进入后台前同步
`NSUserDefaults`，让游戏的原生生命周期保存有机会落盘。

小米平板本地验证：HOME 后进程保持同一 PID，任务栈保留；回到同一 task
后继续原游戏 session，未重新启动 IPA。

## 4. 返回键

启动器：

- BACK 只弹出“离开启动器？”；
- 选择“返回桌面”才离开，取消不改变状态。

游戏：

- `SDLSurface` 会先消费 BACK，因此 `MainActivity.dispatchKeyEvent` 在宿主层
  截获 `KEYCODE_BACK`；
- 第一次 BACK 暂停 native thread 并显示“继续游戏 / 返回启动器 / 退出游戏”；
- 返回启动器和退出游戏都保留确认步骤；
- “退出游戏”还有第二次“未保存进度可能丢失”的确认。

实测截图：

`report/android-3device-validation-20261025/pcmerge-back-pause-small.jpg`

## 5. FSR3 强开

能力预检仍默认生效；不满足时普通 FSR3 预设保持禁用。高级参数新增：

“强制启用 FSR3（高风险）”

打开后，即使 `Fsr3Capabilities.supportsBasicVulkanPath()` 失败，也会向 native
传入：

```text
TOUCHHLE_DS_FSR3_AHB=1
TOUCHHLE_DS_FSR3_FORCE_UNSUPPORTED=1
```

同时仍可使用真实 D16、GPU 深度、异步和 direct-output 子开关。该路径只用于
驱动实测，可能黑屏/闪退；关闭强开时，若不满足能力预检则自动关闭 FSR3 总开关。

## 6. 禁止 touchHLE 原生选择页

打包版没有内置 IPA 时，旧 `getArguments()` 返回空数组，native 会显示
touchHLE 自带 app picker。现在：

- Java 层准备 IPA 失败时提示并回到启动器；
- `SDL_main` 在 Android 上拒绝无 IPA 的 app picker 回退；
- 最终包继续要求包含式 IPA。

## 7. 启动器人员信息

主视觉图标行最右侧固定显示：

```text
键鼠测试：天雷本雷
剩余全部工作：SomJs
```

平板 UI hierarchy 实测位于 `[1598,332][1839,438]`，未与主标题和游戏名重叠。

## 8. 构建与设备检查

构建命令模板与 `MERGE-AUDIT.md` 相同。注意：

- 工作树若仍是占位文件，需要先运行
  `tools/materialize-android-assets.ps1 -Repo <repo>`；
- 需要 `android/local.properties`；
- 切换源码根目录后必须清理
  `target/aarch64-linux-android`，否则 C/C++ wrapper 会缓存旧绝对路径；
- 输出 APK 前应执行 Gradle `clean`，避免 Gradle 增量打包在超大 IPA 条目后保留
  上一版尾部数据。

本次本地回归设备：

| 设备 | 地址 | 结果 |
| --- | --- | --- |
| Xiaomi Pad `2405CRPFDC` | `10.31.0.232:42417` | 启动器、开局黑团修复、BACK 暂停、HOME/回前台均通过本地检查 |
| Xiaomi `M2012K11AC` | `10.31.2.162:37753` | 已重连；最终包回归待执行 |
| OnePlus ACE3 `PJE110` | `10.31.2.194:37953` | 已重连；最终包回归待执行 |

设备回归不能只看到难度页或教程就宣称“可玩”；仍需观察角色移动、转向、后台恢复
和存档写入。

## 9. 诊断开关

以下开关默认关闭，仅用于后续定位：

- `TOUCHHLE_DS_DEPTH_TRACE=1`
- `TOUCHHLE_DS_PIXEL_TRACE="x%,y%"`
- `TOUCHHLE_DS_TEXFLAT_HASH=<key[,key...]>`
- `TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT=<key:count[;...]>`

不要把这些诊断路径当成默认修复。当前默认修复由
`MainActivity.configureRuntimeOptions` 和 `src/lib.rs` 的 Android Dead Space
默认值共同配置，且每个预设都保留 PC 内容修复。

## 10. 2026-10-25 最终包含式 APK 回归

本轮没有沿用旧安装包，而是从当前工作树先执行 Gradle clean，再重新编译 Rust、
Java 和内置 IPA。最终设备包：

Android 画面继承功能提交：`99f08ff1`。本轮文案修正构建的当前 HEAD：
`a44d9782`。

| 项目 | 值 |
| --- | --- |
| APK | `E:\CodexProjects\DeadSpaceLegacy\report\android-mainline-20261025\DeadSpace-IOS-Port-bundled-20261025.apk` |
| 大小 | `584,966,283` 字节 |
| SHA-256 | `4ACDFCC8CB39E2A3EC9AC0A40F6BC3C43FB090D0AD55809918B9C7ECAD3C029B` |
| 内置 IPA 大小 | `532,322,054` 字节 |
| 内置 IPA SHA-256 | `1381CBD7DE15BCAF8085EDF8F191CC54261C00376087D4893AC62FB9B9F2DA1C` |
| native library SHA-256 | `06887DC7B002C04E904120B3E8020BCF566DED23479DE60A08E67BF951D6121B` |
| package / ABI / SDK | `org.touchhle.android` / `arm64-v8a` / min 21, target 24 |

平板 `2405CRPFDC` 覆盖安装后从启动器冷启动，连续抓取 55 秒开局视频：
难度页、月球与碎石、空间站、标题和舱内可操作场景均出现，没有再出现只有
clear color 的超大黑团。证据：

- `report\android-mainline-20261025\opening-regression.mp4`
- `report\android-mainline-20261025\opening-regression-contact.jpg`

同一轮 logcat 明确命中以下实际执行路径，而不是只存在于源码：

```text
[DSTEX opening-mip] rebuilt mip chain key=56c62e8a ...
[DSTEX opening-mip] rebuilt mip chain key=2131afd5 ...
[DSTEX opening-mip] rebuilt mip chain key=18c949fb ...
[DSTEX opening-mip] rebuilt mip chain key=d1feb687 ...
Android native: corrected draw-time finite-far projection
Android native: applied scoped PC fog repair
Android native: scoped PC background GL_LEQUAL repair
Android native: disabled culling for PC station-hull batch
TOUCHHLE_DS_TEX_UPLOAD_FINISH: synchronizing texture uploads
```

可操作阶段还验证了：

- 左侧滑动可横向转向；
- 向上滑动可前进，画面和角色位置实际变化；
- BACK 弹出“继续游戏 / 返回启动器 / 退出游戏”，选择“返回启动器”后确实回到
  `LauncherActivity`；
- HOME 后进程 PID 保持 `17824`，从最近任务返回仍是同一个 `MainActivity` PID，
  没有重新启动 IPA。

因此“黑团再次出现”当前不能在最终包上复现。若旧安装包仍出现，首要核对 APK
SHA-256 是否为本节的 `4ACDFC...`，再看日志是否出现
`corrected draw-time finite-far projection`。不要先恢复全局无雾、岩石不写深度、
整场景不剔除或全局 depth reset，这些方向已于 PC 侧被证伪。
