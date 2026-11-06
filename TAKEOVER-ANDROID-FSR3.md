# Android / FSR3 接手入口

更新时间：2026-10-25（Asia/Hong_Kong）

> 当前状态请先读根目录 [`ANDROID-BRANCH-TAKEOVER.md`](ANDROID-BRANCH-TAKEOVER.md)。
> 本文保留 FSR3 原型和性能实验的历史细节；其中“设备当前离线”“原型分支是当前
> 分支”等陈述已经被 2026-10-25 最终集成状态取代。

本文件是 Android 探索工作的当前入口。它描述 FSR3 原型、启动器画质预设、
实机证据、可复现构建和未完成项。PC 主线的游戏内容修复具有最高优先级；
本分支中的渲染兼容实验不能反向覆盖 PC 已验收构建。

## 1. 先分清三条线

| 工作线 | 位置 | 当前状态 | 用途 |
| --- | --- | --- | --- |
| PC 主线 | `E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat` | 当前安装版含岩石、无重力、E/F、结局灯带修复 | PC 游戏内容与兼容修复的权威来源 |
| Android FSR3 原型 | `E:\CodexProjects\DeadSpaceLegacy\work\HyperHLE_ds_compat-fsr3-prototype` | `0a527845`，画质预设已构建安装 | FSR3.1 探针、性能实验、设备兼容 |
| Android 审计树 | `E:\CodexProjects\DeadSpaceLegacy\work\HyperHLE_ds_compat-fsr3-audit` | 只读审计与移植差异 | 不用于发布构建 |

共同基线是 `d79eb59af344ff5ea51a0b8c12288da35c3eedda`。PC 主线此后有大量
Dead Space 专属内容修复；Android 分支不应把基线中的旧行为覆盖回来。

## 2. 当前分支、提交和交付物

- 分支：`codex/android-fsr3-prototype-20261023`
- 最新代码/APK 提交：`0a527845d8cf317e85fbb2113cbf213c5c97c6af`
- 最近主体提交：
  - `0a124847`：启动器“常用预设 + 自定义”
  - `0a527845`：恢复默认值和诊断项时使用推荐预设
  - `98ce6456`：FSR3.1 异步一帧流水线
  - `4d92da95`：内存采样解析修复
  - `3d17938b`：第 8 章九档录像矩阵
  - `ff7bdcc8` / `e014718f`：细节对照与尺寸标注
- 最新 APK：
  `E:\CodexProjects\DeadSpaceLegacy\report\FSR3-2026-10-23\deliverables-0a527845\DeadSpace-FSR3-presets-0a527845.apk`
- APK SHA-256：
  `ce1c13c2a970c069d4cbfe3492c2000f62f7abbf962716023fa02f547eb5a4ae`
- 技术报告：
  `E:\CodexProjects\DeadSpaceLegacy\report\FSR3-2026-10-23\FSR3-ANDROID-TECHNICAL-REPORT.md`
- 细节对照：
  `E:\CodexProjects\DeadSpaceLegacy\report\FSR3-2026-10-23\video-batch-2026-10-24\chapter8-full-batch\20261024-075154\visual-comparison\fsr3-chapter8-detail-compare.html`
- 第 8 章性能 CSV：
  `E:\CodexProjects\DeadSpaceLegacy\report\FSR3-2026-10-23\video-batch-2026-10-24\chapter8-full-batch\20261024-075154\performance-table.csv`
- 仓库内机器可读副本：
  [`bench-results/CHAPTER8-PERFORMANCE-20261024.csv`](bench-results/CHAPTER8-PERFORMANCE-20261024.csv)
- 归档目录：
  `E:\CodexProjects\DeadSpaceLegacy\report\FSR3-2026-10-23`

`deliverables-f119b2d0`、`deliverables-4d92da95` 等是历史交付，不是当前实现。
`video-batch-2026-10-24` 中误录的第 12 章和 smoke 已移到 `stale/`，不属于交付。

主线集成后的包含式 Android 交付包（内置 508 MB IPA、当前启动器、ZIP 存档导入）：

`E:\CodexProjects\DeadSpaceLegacy\report\deliverables-e1306dcc\DeadSpace-IOS-Port-e1306dcc-bundled.apk`

APK SHA-256：
`c4fc0f71585516e1817c19ff18dbfe55abf0523fb520ee91f72e65236dd9fc5e`。

## 3. 当前产品结论

| 路径 | 结论 |
| --- | --- |
| 原生 GLES present | 可交付；所有不支持设备的最低回退 |
| FXAA / CAS / FSR1 | 可交付的空间处理；必须独立命名，不能冒充 FSR3 |
| FSR3.1 超分探针 | PARTIAL-GO；真实 D16 深度已通，但不是完整时间超分 |
| FSR3.1 帧生成 | NO-GO；缺少真实运动、UI 分层、低延迟 pacing 和端到端验收 |

当前 FSR3 探针使用 FSR3.1.4 超分器、真实 D16 深度、AHB 和异步一帧，
但运动向量为零、jitter 为 `(0,0)`、每帧 reset、没有有效历史，也没有
HUD reactive/T&C mask。准确名称是：

> FSR3.1 异步超分探针（真实 D16，无时间历史）

它不是完整的 AMD FSR3 时间超分，更不是帧生成。

## 4. 推荐设置

新安装默认选择“推荐画质”：

| 参数 | 推荐值 |
| --- | --- |
| 内部渲染 | 原生 3×（`960x1440` 输入档） |
| FSR3 / FSR1 | 关闭 |
| FXAA | 开启 |
| CAS | `0.5` |
| 画面比例 | 保持 3:2 |
| 快速呈现 | 开启 |
| 跳过读回 Finish | 开启 |
| GPU 复制呈现 | 开启 |
| PC 渲染兼容修复 | 开启 |

其他预设：

- **性能优先**：原生 1×，FSR/FXAA/CAS 关闭，保留快速呈现。
- **兼容优先**：原生 1×，关闭快速呈现、跳读回和 GPU 复制，保留 PC 修复。
- **FSR3 实验**：2×、真实 D16、GPU 深度、异步一帧，直写关闭。
- **自定义**：展开全部底层参数；手动改动后自动保存为自定义。

推荐组合只完成了原生 3× 基线与 FSR 路径测试。FXAA/CAS 组合尚未在所有目标设备
独立 A/B，不能把 Adreno 740 的 `60 FPS` 外推到其他 GPU。

当前启动器显示名为“死亡空间IOS移植版”，使用 IPA 中 `iTunesArtwork` 原游戏
图标，采用深黑蓝/细青线/编号侧栏风格。启动器支持导入 ZIP 存档，导入前自动
备份 `Documents`；存档编辑器仅作为计划项展示，并明确提示 `.sb` 写入可能导致
存档损坏。

## 5. 已测设备与第 8 章结果

设备：OnePlus `PJE110`，SM8550 / Adreno 740，Android 15 / API 35，
arm64-v8a，GLES `V@0676.69`，Vulkan 1.3 class。

ADB：
`E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3\platform-tools\adb.exe`

用户已明确当前**无法连接 Android 设备**。此前只读检查看到 USB serial
`c86d3430`、Surface `1080x2376`，系统状态 `Dozing`；这些只是历史观察。
设备恢复前不要反复重试，也不要把旧状态当成在线证据。

| 路径 | 输入 | FPS | 1% low | 帧时间 | PSS |
| --- | ---: | ---: | ---: | ---: | ---: |
| 原生 1× | 320x480 | 60.2 | 42.1 | 16.56 ms | 1.390 GB |
| 原生 2× | 640x960 | 60.0 | 40.6 | 16.56 ms | 1.381 GB |
| 原生 3× | 960x1440 | 60.2 | 41.8 | 16.56 ms | 1.391 GB |
| 同步 FSR 2× | 640x960 | 21.1 | 5.7 | 49.26 ms | 2.787 GB |
| 异步 FSR 1× | 320x480 | 50.9 | 36.3 | 19.31 ms | 2.767 GB |
| 异步 FSR 2× | 640x960 | 50.5 | 38.1 | 19.84 ms | 2.817 GB |
| 异步 FSR 3× | 960x1440 | 44.8 | 35.4 | 22.17 ms | 2.795 GB |
| 异步直写 2× | 640x960 | 46.7 | 35.3 | 21.41 ms | **4.373 GB** |
| 异步直写 3× | 960x1440 | 43.2 | 31.3 | 22.88 ms | 2.802 GB |

异步路径固定多约一帧显示延迟；直写没有稳定领先，2× 还出现异常 PSS。
3× 输入对 1620x1080 内容只有约 1.125 倍放大，所以静态画面接近原生 3×，
不能据此证明动态时间超分质量。

## 6. 代码地图

| 路径 | 作用 |
| --- | --- |
| `android/app/src/main/java/org/touchhle/android/LauncherActivity.java` | 预设、自定义参数、配置迁移 |
| `android/app/src/main/java/org/touchhle/android/Fsr3Capabilities.java` | API/ABI/Vulkan/AHB 基础门控 |
| `android/fsr3-bridge/ds_fsr3_bridge.cpp` | JNI 与 FSR3.1 dispatch |
| `android/fsr3-bridge/CMakeLists.txt` | Android arm64 Vulkan bridge 构建 |
| `src/gles/ds_ahb_bridge.c` / `.rs` | GLES/AHB 桥 |
| `src/gles/ds_ahb_vulkan_probe.c` | Vulkan 外部内存与设备探针 |
| `src/gles/fsr_es2.rs` | FSR 呈现路径 |
| `src/frameworks/opengles/eagl.rs` | Android present 生命周期 |
| `src/gles/present.rs` | present 快路径与 FSR 集成 |
| `tools/build-android-fsr3.ps1` | 独立 Android FSR3 构建入口 |
| `tools/run-fsr3-device.ps1` | 实机运行与证据采集 |
| `tools/record-fsr3-video-batch.ps1` | 九档录像矩阵 |
| `tools/compare-fsr3-video-frames.py` | 固定帧、ROI、指标与 HTML 对照 |

FSR3 原生库位于 `android/fsr3-bridge/bin/ffx_sdk/`。授权与二进制通知见同目录
`LICENSE-AMD.txt`、`LICENSE-SNOWAPRIL.txt` 和 `THIRD-PARTY-NOTICES.md`；
发布前仍需法务复核，不要把 AMD 二进制限制误写成普通 MIT。

## 7. 构建与运行

参考工具链：

- JDK 19：`C:\Program Files\Java\jdk-19`
- Android SDK/NDK：`E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3`
- Gradle：`E:\CodexProjects\DeadSpaceLegacy\tools\gradle-8.11.1\bin\gradle.bat`
- Rust：`E:\CodexProjects\DeadSpaceLegacy\tools\rust`

在干净 Windows checkout 上，先满足以下不随 Git 提交的前置条件：

1. `git submodule update --init --recursive`；
2. 将 Boost 1.81 放到 `vendor/boost`；
3. 创建 `android/local.properties`，写入 `sdk.dir=<Android SDK 路径>`；
4. 运行 `tools/materialize-android-assets.ps1 -Repo <repo>`，替换 Windows
   无法直接使用的 symlink 占位和无效图标。

否则 Gradle 会在资源、`libxml2s`、Boost 或 `local.properties` 阶段失败。

```powershell
& '.\tools\build-android-fsr3.ps1' `
  -JdkHome 'C:\Program Files\Java\jdk-19' `
  -AndroidSdk 'E:\CodexProjects\DeadSpaceLegacy\work\android-sdk-fsr3' `
  -GradleBat 'E:\CodexProjects\DeadSpaceLegacy\tools\gradle-8.11.1\bin\gradle.bat' `
  -RustRoot 'E:\CodexProjects\DeadSpaceLegacy\tools\rust' `
  -GradleUserHome 'E:\CodexProjects\DeadSpaceLegacy\work\gradle-home-fsr3' `
  -BuildType release -Abi arm64-v8a -TargetSdk 24 -ScaleHack 3 -FastPresent:$false
```

构建后使用 `adb install -r` 覆盖安装，不卸载应用，不清数据。真实游戏 IPA 和
用户存档不入 Git。

## 8. 未提交状态和工作树规则

接手时先运行：

```powershell
git status --short --branch
git log -10 --oneline --decorate
```

当前分支原有未提交内容包括：

- `android/app/src/main/assets/touchHLE_dylibs` 等资源占位删除；
- 三个图标资源修改；
- `android/app/src/main/jniLibs/arm64-v8a/*.so` 未跟踪。

这些可能是用户原有构建/测试改动。不要 `git add -A`，不要 reset/checkout，
除非逐项证明归属并得到授权。

## 9. 下一步优先级

1. 用户解锁后完成启动器五档点按、布局截图和持久化回归。
2. 在 60/90/120 Hz、720p/1080p/1440p、不同 aspect 下执行横屏矩阵；
   Dead Space 固定横屏，用户旋转不是产品验收项。
3. 消除每帧 GLES/Vulkan 同步和串行等待，再做性能结论。
4. 接入真实相机/几何运动、jitter、有效历史和 HUD reactive/T&C mask。
5. 只有完整时间输入和动态画质通过后，才评估四队列帧生成和低延迟 pacing。
6. 移植 PC 内容修复时，以 PC 主线当前提交与黄金帧为源，逐项 A/B，禁止把
   Android 分支的旧兼容实验当作内容修复权威。

详细设备矩阵见 `FSR3-ANDROID-DEVICE-TEST-MATRIX.md`，移植原则见
`PC-PARITY-PORTING-CHECKLIST.md`，架构计划见 `FSR3-INTEGRATION-PLAN.md`。

## 10. 会话审计

本轮已用 `tools/audit_codex_sessions.py` 逐行扫描指定的 Android 主会话及
续会话，并同时审计 PC 会话和其上游会话。完整审计清单和原始 rollout
SHA-256 位于：

`E:\CodexProjects\DeadSpaceLegacy\audit\session-2026-10-25\session-audit.json`

该目录中的 `.dialogue.md` 是本地审计索引，包含大量工具输出，不提交 Git。
