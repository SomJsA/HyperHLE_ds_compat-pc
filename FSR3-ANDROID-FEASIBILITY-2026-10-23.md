# Dead Space Android FSR3 可行性审计

日期：2026-10-23  
审计分支：`codex/android-fsr3-prototype-20261023`  
Android 基线：`d79eb59af344ff5ea51a0b8c12288da35c3eedda`

## 结论

**FSR3 的时间超分在 Android 上技术上可运行，但当前项目整体仍不可交付。**

- **FSR3 时间超分：有条件可交付。** 在 OnePlus Adreno 测试机上，已把真实
  Dead Space 颜色帧经 GLES `AHardwareBuffer` 送入 Vulkan FSR3，并持续完成
  数百次超分 dispatch，输出回 GLES 后可见。必须注意：捕获版探针把
  `upscale.reset` 固定为 `true`，即使 context 持久，也没有真正累积时间历史；
  因此它只验证调用链与输出，不验证时间超分画质。真实运动向量、相机 jitter、
  HUD reactive mask 仍缺失，深度仍是同步 CPU 读回。
- **FSR3 帧生成：不可交付。** 独立样例已证明该 Adreno 能创建四队列 FI
  swapchain，并把 presented/rendered 提高到约 1.8；游戏内也成功执行过
  frame-generation dispatch。但游戏实验使用合成运动和合成深度，关闭 reset，
  出现严重重影；没有真实 UI 分层、稳定 pacing、延迟、VRR 和长时间热测试。
- **现有 FSR1/EASU+RCAS、CAS、FXAA：可继续作为独立的空间放大/锐化路径。**
  它们不是 FSR3，不能混报。

当前来源包中记录的真实设备为 OnePlus、Android 15/API 35、arm64-v8a、
Adreno 740、Vulkan 1.3.128、driver 0.676.69。FSR3 结果文档写的是
`PJE110`，较早的构建元数据写的是 `PJZ110`；这一型号差异尚未通过当天
`getprop` 再确认，不能擅自抹平。

## 1. FSR3 不是单一滤镜

| 组成 | 官方要求/作用 | 本项目状态 |
| --- | --- | --- |
| 时间超分 | 当前低分辨率颜色、深度、运动向量、jitter，结合历史帧重建 | 只接入颜色与部分深度；运动、jitter、reactive 缺失 |
| 光流 | 从前后帧估计像素运动，供帧插值使用 | 官方 FSR3 内部实现；独立样例已执行，游戏链路未验证 |
| 帧生成/插值 | 在真实帧之间生成并呈现额外帧 | 独立样例成功约 1.8x 呈现；游戏内仅合成探针 |
| 延迟降低 | 需要与呈现队列协同；AMD Anti-Lag 2 是单独技术 | 未接入 |
| HUD/UI 处理 | callback、独立 UI surface 或 HUD-less color | 游戏 HUD 与 3D 同在最终颜色缓冲，未分层 |

FSR3.1 的关键变化是把超分和帧生成在 API 层解耦，允许分别启用或与其它上采样
器组合；FSR3.0 更强调成组集成。二者都不是 FSR1 的空间滤镜。

## 2. 当前渲染链路

默认 Dead Space 路径是 guest GLES1 + Android native ES 1.1，而不是 GLES2/3。
EAGL drawable 为 RGBA8，深度请求 24 bit、stencil 8 bit；若 RGBA8
renderbuffer 分配失败才退到 RGBA4。`--scale-hack` 只扩大 drawable，不扩大
guest 自己创建的离屏 FBO。

当前和原型链路：

```text
guest GLES1 / native ES1.1
  -> EAGL color renderbuffer (RGBA8) + guest depth FBO (D24S8)
  -> fast present: glCopyTexSubImage2D to present texture
  -> [prototype] shared ES2 context
  -> GLES texture -> EGLImage <- AHardwareBuffer -> Vulkan external image
  -> FSR3 context (color + depth + synthetic/zero motion)
  -> output AHardwareBuffer -> EGLImage -> shared ES2 present
  -> SDL EGL swap
```

图中也标出了缺失的数据。见
[FSR3-ANDROID-DATAFLOW.svg](FSR3-ANDROID-DATAFLOW.svg)。

### FSR3 输入缺口

| 输入 | 现有事实 | 结论 |
| --- | --- | --- |
| 颜色 | 最终 EAGL color 可零 CPU 读回复制到 AHB | 有 |
| 深度 | guest 有 D24；原型通过私有 ES2 context `glReadPixels` 转成 float 数组 | 有数据，但阻塞 CPU |
| 运动向量 | 源码没有 per-pixel velocity buffer；原型写零或常量 4 px | 缺失 |
| 相机 jitter | 没有逐帧 sub-pixel jitter 序列，也没有向 FSR3 上报 jitter offset | 缺失 |
| 历史帧/reset | FSR3 内部历史存在；菜单、过场、加载、镜头切换的 reset 规则未完成 | 部分 |
| HUD/UI 分层 | 游戏 HUD、字幕、准星都在同一最终颜色帧 | 缺失 |
| reactive/T&C mask | 无 HUD、武器、粒子、透明材质专用 mask | 缺失 |
| 帧时间 | 已有 present interval、均值、1% low、min 和 frame counter | 有，用于实验 |
| HDR/色彩 | 当前 RGBA8/UNorm 路径，未见 HDR 或独立线性色彩管线 | SDR；不能宣称 HDR 支持 |

因此，现在可验证的是“真实颜色帧驱动 FSR3 时间超分 API”，不是完整的
生产级 FSR3 集成，更不是完整帧生成。

## 3. 官方 SDK、平台与许可证

审计对象：

- AMD `FidelityFX-SDK` tag `v2.3.0`，commit
  `60f4ea81909200d8542eca14dccb2628b763a9a3`。
- AMD `FidelityFX-SDK` tag `v1.1.4`，commit
  `c6efa6bf7f2027b3ec94f28578bb5965eabb9e55`。
- `Snowapril/fsr3-android` commit `3a369b3`，Android/Vulkan 移植。

官方事实：

- AMD GPUOpen FSR3 页面声明 FSR3 支持 DirectX 12 和 Vulkan。
- 官方 SDK 2.3.0 当前源码树没有 Vulkan backend；其 README 明确写有
  “Vulkan is currently not supported in SDK”。因此不能把 2.3.0 直接接入
  Android Vulkan。
- 官方 v1.1.4 确实包含 Vulkan backend、FSR3 upscaler、optical flow 和
  frame-interpolation swapchain，但没有 Android 后端；工具链仍硬编码
  Windows/MSVC，需要移植。
- GLES/EGL 没有官方 FSR3 backend。GLES 只能作为颜色/深度输入和输出显示的
  桥接方，实际算法运行在 Vulkan；Android 侧用 `AHardwareBuffer` 与
  `VK_ANDROID_external_memory_android_hardware_buffer` 共享纹理。
- 社区 Android 端口要求 `minSdk 31`，当前 APK 仍保持 `minSdk 21`，所以
  FSR3 只能是 API 31+ 的能力门控路径。

许可证不能概括成“全部 MIT”。FSR3 核心 SDK 的捕获版 `LICENSE.txt` 是 AMD
自定义的 binary-form redistribution 许可，明确禁止反向工程；部分样例和
第三方文件才是 MIT。发布前必须保留完整 notice 并把源码/二进制分发方式交给
法务确认。

## 4. Android 现实性与风险

- **Vulkan：** Adreno 740 实测具备 compute、timeline semaphore、
  `VK_ANDROID_external_memory_android_hardware_buffer`、descriptor indexing
  等能力；四队列 FI 样例可运行。
- **GLES：** 当前游戏是 GLES1；FSR3 不直接运行在 GLES。共享 ES2 context
  只负责上传、读取 FSR3 输出和呈现。
- **格式：** 颜色桥为 RGBA8；深度输入是 D24，原型转 float；运动目标是
  RG16F；RGBA4 回退不适合作为 FSR3 正式格式。
- **色彩：** 当前未证明线性 HDR 或正确 transfer function；不能把 SDR
  RGBA8 输入描述成 HDR FSR3。
- **性能：** CPU 深度 `ReadPixels`、同步 fence、`glFinish` 和调试中心像素
  读回都会污染测试结果；必须流水化后才能做正式性能结论。
- **帧生成：** 低输入帧率、HUD 重影、QTE 输入延迟、菜单/镜头切换 reset、
  表面重建和 VRR pacing 都未通过；不允许默认开启。
- **驱动覆盖：** 只有 Adreno 740 有 FSR3 证据。Mali/Immortalis、Xclipse、
  PowerVR 和旧 Adreno 仍需 capability probe 与安全回退。

## 5. 已记录的设备实验

| 实验 | 结果 | 是否可作为交付性能 |
| --- | --- | --- |
| 独立 FI 样例 | rendered 1681 / presented 3009；约 31.6 rendered FPS、56.6 presented FPS；比值约 1.8 | 否，非游戏负载 |
| GLES 帧 -> AHB -> Vulkan 导入 | 真实游戏帧内容进入 Vulkan，中心像素随场景变化 | 功能证据 |
| 一次性 FSR3 超分 | 640x960 -> 1080x2376，输出中心像素与输入相关 | 功能证据 |
| 持久 FSR3 超分 | 541 次 dispatch；菜单/字幕可读；10 秒约 23.42 FPS、42.74 ms | 否，同步探针 |
| 游戏内 FG + 合成运动 | 输出已改变，但严重重影；约 16.8 FPS / 59 ms | 否 |
| 安全 v16（不显示合成运动） | 仍执行 FG dispatch，约 17.2 FPS / 58 ms；真实输出可见 | 否 |
| FSR1 quality A/B | 60 秒：52.08 FPS / 42.8 1% low / 19.21 ms | 仅是 FSR1 数据 |
| 原生 fast A/B | 60 秒：56.98 FPS / 38.84 1% low / 17.55 ms | 仅是同构建对比 |

FSR3 实验没有记录完整 1% low、最小帧时间、内存峰值、功耗和温度曲线，因此
不能写“性能提升”。已有证据只支持“算法与呈现链路确实在这台手机上跑通”。

## 6. 独立原型

本分支从 Android 基线 `d79eb59a` 单独建立，不包含之后 PC 侧的渲染修复。
新增内容：

- `src/gles/ds_ahb_bridge.c/.rs`：AHB、EGLImage、GPU 复制和诊断。
- `src/gles/ds_ahb_vulkan_probe.c`：Vulkan external-memory 探测。
- `src/gles/fsr_es2.rs`：共享私有无状态 ES2 present 上下文。
- `src/frameworks/opengles/eagl.rs`：仅在 FSR3 env 开启时接管最终 present。
- `android/fsr3-bridge/`：独立 JNI 库、CMake、AMD 静态库和构建脚本。
- `MainActivity` / `LauncherActivity`：实验开关，默认全部关闭。

默认路径不设置以下变量，因此保持原 GLES/Windows 行为：

```text
TOUCHHLE_DS_FSR3_AHB
TOUCHHLE_DS_FSR3_REAL_DEPTH
TOUCHHLE_DS_FSR3_FG
```

捕获版 JNI 库与源码一致地把 `reset` 固定为 `true`；以后增加历史帧实验时，必须
同时重建 `libdsfsr3bridge.so`，不能只改源码或开关。

当前机器没有 ADB 设备，也没有可用 Android NDK，因此本轮没有重新构建 APK、
没有在设备上重跑，不把历史日志冒充本轮验证。已验证的历史 APK：

```text
DeadSpace-arm64-fsr3-es2-depth-v6-20261021.apk
size:   592581765
sha256: C0E45BDA8306F607A86FA4A792795FCFC7DF959DFDA09665176C51A233D691B8
source: fa9a18f (per handoff)
```

## 7. 推荐路线

1. **先交付 FSR1/CAS 和原生回退。** 它们已有真实设备 A/B，不要改名成 FSR3。
2. **FSR3 只做超分的实验分支。** 用真实 D24 零拷贝输入替掉 CPU 深度读回；
   从当前/上一帧 view-projection 计算相机 reprojection motion；加入 jitter。
3. **建立 ROI/reactive mask。** 先保证 HUD、字幕、准星、QTE 和武器不参与历史
   重投影，再评估透明材质和粒子。
4. **补 safe-mode。** 菜单、过场、加载、镜头 cut、分辨率变化、app pause/resume
   强制 reset。
5. **再做 FG。** 必须单独 A/B，先证明四队列、持久 swapchain、真实运动、
   HUD 无重影、延迟可接受，并至少跑 10 分钟。
6. **扩设备矩阵。** Adreno 740 之外至少覆盖一个 Mali/Immortalis 和一个旧
   Adreno；未知驱动走原生/FSR1。

下一轮实机验收必须同时保存：logcat、`dumpsys meminfo`、GPU/驱动信息、固定
存档与输入脚本、逐帧时间、1% low、min/max、温度和功耗，以及 FSR3 开关两组的
截图/录屏。没有这些记录，只能算功能探针。

## 8. 证据索引

- `FSR3-INTEGRATION-PLAN.md`
- `ANDROID-AUTOMATED-BENCH.md`
- `ANDROID-UPSCALING-FRAMEGEN.md`
- 补充包中的 `artifacts/research/fsr3-probe/FSR3-M0-RESULT.md`
- 补充包中的 `DEADSPACE-AHB-BRIDGE-RESULT.md`
- 补充包中的 `DEADSPACE-FSR3-UPSCALE-RESULT.md`
- 补充包中的 `DEADSPACE-FSR3-DOUBLE-AHB-RESULT.md`
- 补充包中的 `DEADSPACE-FSR3-FRAMEGEN-PROBE-RESULT.md`

官方资料：

- <https://gpuopen.com/fidelityfx-super-resolution-3/>
- <https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK>
- <https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK/tree/v1.1.4>
