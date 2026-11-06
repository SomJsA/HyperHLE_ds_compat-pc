# Dead Space Android 实机测试记录（2026-10-20）

## 设备与环境

- 设备：OnePlus PJZ110，Android 15 / SDK 35，arm64-v8a
- SoC：Snapdragon SM8750，GPU：Adreno 830，OpenGL ES 3.2
- 画面：2376x1080（原生横屏，沉浸式/短边刘海适配已启用）
- 当前 APK SHA-256：624585AAB397CBDC7000651760880465C578974BB8DCE08F4F95D06FB02696AA
- ADB：3B1F6KE5MS11JRVM

## 已验证

- APK 可安装、可启动。
- 启动日志包含：
  - `[Android] installed native crash signal handlers`
  - `[Android] SDL_main argc=3`
  - `--preferred-languages=zh-Hans,zh`
  - 内置 IPA 路径：`/storage/emulated/0/Android/data/org.touchhle.android/files/touchHLE_apps/DeadSpace.ipa`
- 修复了难度选择后的 JNI/协程栈溢出：
  - `SDL_GetPowerInfo` 已通过 `on_parent_stack_in_coroutine` 切到父线程栈；
  - 崩溃栈不再出现 `Android_JNI_GetPowerInfo -> JNI NewStringUTF -> StackOverflowError -> SIGABRT`。
- 已进入关卡并验证：
  - 场景、角色、HUD、中文教程正常显示；
  - 蓝环交互区域可见；
  - `adb shell input swipe` 能改变角色朝向/移动；
  - 倾斜手机切换枪械正常（用户实机反馈）。
- 服务器不可用弹窗已定向屏蔽：
  - 仅拦截 `com.ea.deadspace.bv` 的 `Server Unavailable / online game functionality has been disabled`；
  - 不全局屏蔽其他 UIAlertView。

## 存档

- 导入包：`D:\BaiduSyncdisk\DeadSpace-Android-SaveBank-2026-10-20_214656-final.zip`
- ZIP SHA-256：`245779FD7D8C9442DFE8EA1AB99D6933F353F5EB0E8768DF6E64B8CBB69419C2`
- 使用变体：`save-final-level-test`
- 读档页实测三条线均显示“第 12 章”；
- 2012 原始档在 `save-original-2012`，三条线为早期章节，且可能处于二三周目状态；
- 最终区域/Boss 测试使用 `06_titan_reactor_01` 派生存档；
- 导入前安卓存档备份目录：
  `C:\Users\SomJs\Documents\ChatGPT\DSMB安卓\artifacts\SaveBank-214656\save-backup-20261020-215239`

## 字体/字符问题（未修复）

### 症状

读档页出现经典叠字/错字：

- `档案 1` 显示成类似 `档案 I+`；
- `05:04:33` 显示成错位/叠印字符；
- 中文字体本身可读，但数字/Latin 字形与布局基线异常。

证据截图：

- `artifacts\SignalDiag-185828\live\ds-final-save-01.png`
- `artifacts\SignalDiag-185828\live\ds-chs-07.png`
- `artifacts\SignalDiag-185828\live\ds-quality-current.png`

### 日志证据

- `[DSfont] NotoSansSC-Bold.otf replaced by Android bundled override ...`
- `[DSfont] NotoSansSC-Regular.otf replaced by Android bundled override ...`
- `[DSfont] LiberationSans-Regular.ttf replaced by Android bundled override ...`
- 读档页出现大量：
  - `CGFontGetGlyphBBoxes ... glyphs=[...]`
  - `[textpath] font::glyphs len=0`
  - `[glyphdbg] pixel_bounding_box() = None -> glyph GlyphId(3) skipped`

### 初步判断

这是字体替换后的 **guest glyph id -> substitute font glyph id** 映射不完整，不是简单的字符编码错误：

- 游戏用原始字体计算 glyph id；
- 替换字体用同一 glyph id 查找时，数字 `1`、时间数字等会落到错误字形；
- 源码 `src/font.rs` 注释已记录同类问题：
  - `档案 1` -> `档案 .(`；
  - `05:04:33` -> `-27-1700`；
  - 已存在 `gid_remap: guest glyph id -> char` 机制，但当前路径仍漏掉读档页的时间/数字字形。
- `src/font.rs` 当前 Android 默认对所有字体启用 `SweiShearSansCJKsc-Medium.ttf` 覆盖；
  `TOUCHHLE_DS_FONT_OVERRIDE_FOR=cjk` 可以限制为只替换 CJK 字体，但 Android 构建目前没有便捷的环境变量入口。

### 修复方向

1. 优先把 `TOUCHHLE_DS_FONT_OVERRIDE_FOR` 在 Android 端默认为 `cjk`，保留游戏原始 Latin/数字字体；
2. 或者在替换字体时建立完整的原始 `cmap` 反向映射，并覆盖：
   - `CGContextShowGlyphsAtPositions`
   - `CGContextShowGlyphsWithAdvances`
   - `CGContextShowGlyphs`
   - UIKit 单字符 `font::draw`
3. 对缺失 glyph 不要跳过整段文本；按字符或 cluster 回退到原始字体；
4. 增加读档页专用回归用例：`档案 1`、`档案 2`、`第 12 章`、`05:04:33`；
5. 修复后重新截取读档页与关卡 HUD 对比。

## 画质/渲染问题（未修复）

### 观察

- 开局黑罩/大块黑色区域持续一段时间，约 25 秒后完整 3D 场景出现；
- 用户反馈海报嵌入墙体、开场渲染错误加重；
- 画质明显偏低，纹理/UI 边缘模糊；
- 帧率不稳定。

### 已有 PC 修复

Android 源码已有：

- `src/window.rs`：请求 `SDL_GL_DEPTH_SIZE=24`、`STENCIL_SIZE=8`；
- `src/frameworks/opengles/eagl.rs`：MSAA depth renderbuffer 使用 `GL_DEPTH_COMPONENT24_OES`；
- `src/frameworks/opengles/gles_guest.rs`：用户 FBO 自动挂载 24 位深度；
- `--force-composition` / 慢路径 `glReadPixels` 呈现。

因此不能直接判定为“fork 没带 PC 修复”，更可能是：

- 原始移动端素材分辨率低；
- Android 原生 GLES1 的默认分辨率/呈现路径与 PC OpenGL 路径不同；
- 慢路径每帧 `glReadPixels` + RAM 合成造成卡顿和帧时间抖动；
- 24 位深度在 Adreno 原生 ES1 上是否真正生效还需要运行时查询/验证。

### 可选验证

- `--scale-hack=2`：提升内部渲染分辨率，观察文字/边缘和 FPS；
- `--print-fps`：记录实际帧率与波动；
- 对比 `TOUCHHLE_DS_DEPTH_SIZE=16/24` 的读档页/关卡海报；
- 进一步优化 present 路径，避免每帧 `glReadPixels`。

## 未完成/风险

- 无重力跳跃尚未测试（需要 final 存档第 12 章和实际关卡输入）；
- 暂停/Home 后曾有进程重启或 `SIGABRT`，生命周期仍需单独验证；
- 内存 PSS 约 0.8GB，native heap 约 0.6GB，需关注低内存门槛；
- 字体修复、画质/FPS 优化尚未验证；
- 没有把 IPA/APK/存档提交进 Git。

## 字体问题的更精确根因（2026-10-20 21:55 补记）

源码核对后发现两个 glyph 绘制入口没有应用 `gid_remap`：

- `src/font.rs:1124 draw_glyphs_at_positions`
- `src/font.rs:1215 draw_glyphs_with_advances`

两者直接执行 `self.rt().glyph(*glyph_id)`，没有先查 `gid_remap`：

- 读档页时间/数字很可能走 `CGContextShowGlyphsWithAdvances` -> `draw_glyphs_with_advances`；
- 位置排版文本走 `CGContextShowGlyphsAtPositions` -> `draw_glyphs_at_positions`；
- 只有 `font::glyphs` 路径（`src/font.rs:899` 附近）做了 guest glyph id 到 substitute glyph id 的转换；
- 因此 `档案 1` 的 `1` 和时间 `05:04:33` 会拿原始字体的 glyph id 去 CJK 替代字体里查，得到错字形或空字形；
- 日志中的 `[textpath] font::advances len=0` 和 `pixel_bounding_box() = None -> glyph GlyphId(3) skipped` 与该路径吻合。

### 最小修复草案

在 `draw_glyphs_at_positions` 和 `draw_glyphs_with_advances` 中，对每个 glyph 先做：

```text
guest_gid -> gid_remap[guest_gid] -> substitute_font.glyph(char).id()
```

若映射不到，保留原 glyph id 或按字符回退原字体；同时保持原有 position/advance，不要重新计算排版。

这是比“只改默认 override 为 cjk”更直接的修复，能保留中文字体替换，同时恢复数字/Latin 字形。

## 2026-10-20 21:56 实机补充

- 无重力跳跃已验证正常（用户实机反馈）。
- 倾斜手机切换枪械已验证正常（用户实机反馈）。
- 当前剩余已知问题：
  - 读档页数字/时间 glyph 错位（已定位到 `draw_glyphs_at_positions` / `draw_glyphs_with_advances` 未应用 `gid_remap`，修复已写入源码并正在构建）；
  - 开局巨大黑色遮罩/局部渲染错误；
  - 模型/纹理精度低；
  - 帧率不稳定；
  - 暂停/Home 生命周期曾有进程重启/`SIGABRT`。
- 用户判断开局黑罩与 PC 现有渲染问题来源一致；需要继续对比 Android native GLES1 与 PC `gles1_on_gl2` 的深度、FBO、present 路径。
## 2026-10-20 性能监视补充

用户实机观察：

- 游玩时 GPU 基本不动；
- CPU 调用也不算积极；
- 首次进入没去过的区域会出现巨大的瞬时 CPU 负载。

结合日志判断：

- `presentRenderbuffer` 当前走“无 fullscreen layer -> `glReadPixels` 到 RAM -> 合成器上传”的慢路径；
- 每帧同步读回会阻塞渲染线程，GPU 利用率自然不高，帧时间会抖动；
- 首次进入新区时的峰值更像资源解压/上传、纹理首次解码、Dynarmic 翻译/缓存构建和场景对象初始化，而不是持续 GPU 负载；
- `--force-composition` 是默认选项，但不是唯一原因；`find_fullscreen_eagl_layer` 找不到 fullscreen layer 时也会强制慢路径。

后续优化顺序：

1. 先尝试让 Android 的 EAGL drawable 被识别为 fullscreen layer，走 `TOUCHHLE_USE_FAST_PRESENT` 直接呈现；
2. 若不可行，优化 slow path：复用 PBO/像素缓冲、减少每帧分配、避免不必要的合成；
3. 对首次进入新区域做资源预取/缓存，降低瞬时 CPU 峰值；
4. 再考虑 `--scale-hack=2`，因为当前瓶颈不是 GPU。
## targetSdk 24 与“安卓版本太低”的判断

- 当前 APK：minSdk 21，targetSdk 24，compileSdk 35，arm64-v8a。
- targetSdk 24 确实很低，但它主要影响 Android 兼容行为：
  - 全屏/边到边/insets；
  - 旧式存储与权限；
  - SurfaceView、窗口布局和后台限制；
  - 不是 GPU/纹理/模型精度的直接决定因素。
- Android 版本/驱动碎片可能造成：
  - depth/z-fighting、present 行为、色彩带、纹理采样异常；
  - 但不会凭空提升模型面数或纹理分辨率。
- 现有“过低精度”更可能来自：
  1. IPA 原始 iPad2 时代素材；
  2. 内部渲染分辨率低于屏幕；
  3. 原生 GLES1 纹理过滤/压缩路径；
  4. slow present 的 `glReadPixels` 同步。
- 值得做一个 **targetSdk 35 诊断版**，保持 minSdk 21，验证 Android 15 的 edge-to-edge、insets、SurfaceView 和 present 行为；不要直接把正式包切到 35，先 A/B 对比。

## 2026-10-21 00:00 真机验证：targetSdk 35 / scale-hack 2 / no-fog / 字体修复

设备：

- OnePlus PJZ110
- Android 15，SDK 35
- arm64-v8a
- 2376x1080 横屏

安装包：

- `android/app/build/outputs/apk/release/app-release.apk`
- SHA-256 `F66F3EA9631ECD9318BCC9DF3E62477745DC7D472A26EB6538EA9E4DF2979F56`
- 大小 582,997,416 bytes
- 使用 `adb install -r -d` 覆盖安装，存档保留

已验证：

- 应用冷启动成功，PID 稳定；
- 中文读档页正常；
- `档案 1+`、`档案 2+` 是游戏原始二周目标记，不是错字；
- `05:04:33`、`05:55:40`、`08:04` 已全部正确，字体 remap 修复在真机生效；
- 第 12 章存档可进入；
- 截图中角色、场景、中文 HUD、拾取提示和敌人均正常绘制；
- 未发现应用自身的 fatal/panic/SIGABRT；日志中仅有预期的联网不可用 warning。

当前真机证据：

- `artifacts/device-live-20261020_235842/screen-35s.png`：读档页，字体正确；
- `artifacts/device-live-20261020_235842/screen-000052.png`：进入第 12 章后的游戏画面；
- `artifacts/device-live-20261020_235842/live-logcat.txt`：HDMI/系统日志持续采集中。

新发现问题：

- targetSdk 35 下状态栏重新显示。`dumpsys window` 显示应用窗口最终带有 `FORCE_NOT_FULLSCREEN`，systemBars insets `visible=true`；说明 SDL 在 `MainActivity.onCreate` 之后覆盖了全屏状态；
- 已修改 `MainActivity.java`：增加 `FLAG_FULLSCREEN`、清除 `FORCE_NOT_FULLSCREEN`，并在 decor view attach 后 0/250/1000/2000 ms 重新断言沉浸式模式；还需构建并真机回归。

性能采样（第 12 章，游戏运行中）：

- SurfaceFlinger 最近 69 帧：平均间隔 14.53 ms，约 68.8 FPS；
- P50 8.3 ms、P90 16.61 ms、P95 41.51 ms、P99 257.39 ms；
- 说明常态帧率尚可，但仍有明显长帧/瞬时卡顿；
- 进程 CPU 约 92.8%，几乎全部集中在 `SDLThread`；GPU 侧没有持续高占用；
- 进程 RSS 约 909-955 MB；
- 与本机判断一致：瓶颈主要是单线程模拟/`glReadPixels` 慢 present，而不是 GPU 算力。

## 2026-10-21 00:16 PC 渲染修复对齐审计

用户实机确认第一章存在“固定距离黑色层/巨大黑色球体与模型穿插”，与 PC 端记录的同源问题吻合。PC `gles1_on_gl2` 注释明确说明：

- 太空背景层是共面四边形；
- 背景层 clip-space z 实测约为 1.003，落在远平面外；
- 仅做投影 z 压缩后，多层深度会变成完全相同；
- 若仍使用 `GL_LESS`，最先绘制的黑色层占像素，正确背景被拒绝；
- 因此 PC 默认同时启用：
  1. 投影 z 行压缩；
  2. 每次 draw 将 `GL_LESS` 临时改为 `GL_LEQUAL`，让最后提交的正确层获胜。

Android 原生 GLES1 原先两项都没有移植。已在 `src/gles/gles1_native.rs` 增加：

- `native_proj_z_scale_begin/end`，默认 `TOUCHHLE_DS_PROJ_Z_SCALE=0.85`；
- `native_depth_lequal_begin/end`，默认启用 `GL_LESS -> GL_LEQUAL`。

另外发现并补齐 PC 参数：

- `TOUCHHLE_DS_SCREEN_SCALE=2`：
  - PC wrapper 显式设置；
  - Android 原先因 `host_screen_size` 走默认值 1.0；
  - Dead Space 会用 UIScreen.scale 推导 atlas UV，1.0 会导致 sprite/particle/blood atlas 放大和采样错误；
  - 已在 `src/lib.rs` 为 Dead Space 默认设为 2.0。

仍未完全对齐：

- `--force-composition`：PC 显式传入，Android 没传入；当前 Android 实际走 slow present，但应显式统一。
- `--landscape-right`：PC 显式传入，Android 依赖设备方向；当前画面方向正确，需触摸映射回归后再决定。
- PC 的 CPU matrix-palette skinning：
  - PC 在 `gles1_on_gl2` 内实现了完整的 palette/weight/index CPU 蒙皮；
  - Android 原生 backend 对 `CurrentPaletteMatrixOES`、`LoadPaletteFromModelViewMatrixOES`、`MatrixIndexPointerOES`、`WeightPointerOES` 仍使用默认 no-op；
  - 同时原生 `GetString(GL_EXTENSIONS)` 主动裁掉了 `GL_OES_matrix_palette`；
  - 这是潜在的几何/角色变形风险，需要真机确认 Adreno 扩展与游戏实际路径。
- `TOUCHHLE_DS_FRUSTUM_FAR_SCALE`：PC 有远平面缩放诊断参数，Android 未移植。
- PC 的 draw/UV/fog/texture trace 诊断开关大多只在 `gles1_on_gl2` 存在，原生路径未移植；这些不是发行必需，但会影响后续定位。

最新构建（尚未安装）：

- `artifacts/diagnostics/DeadSpace-arm64-target24-scale2-screen2-nofog-fontfix-zfix085-lequal-20261021_001613.apk`
- 大小 592,601,652 bytes
- SHA-256 `2DFB7D4CDFF5471E622DB4C96B86A6BD5CBDD2102DA720CFC0B5DDAD38E515EF`

注：投影 z 修正是逐 draw 读投影矩阵并 push/pop，可能增加 ES1 驱动同步开销；正确性验证后应评估是否改为在 `Frustumf/x` 一次性调整远平面，或缓存投影矩阵。

## 2026-10-21 00:36 AVD 回归与限界

目标 x86_64 AVD：`DS_API35_x64_20261020`，Android 15，host GPU（NVIDIA RTX 4060）。

测试中发现两个模拟器侧问题：

1. 第一版 z/depth 补丁在 AVD 首个 splash draw 崩溃：
   - 栈指向 `libGLESv1_enc.so GLEncoder::s_glIsEnabled`
   - 触发点包括 `native_depth_lequal_begin` 和 `native_nofog_begin`
   - 根因是 Android Emulator 的 GLES1 encoder 对运行期 `glIsEnabled` 的缺陷，不一定是真机问题。

2. 已把 depth-test / depth-func / fog 状态改为 wrapper 内部原子变量跟踪，不再在 draw 前调用 `glIsEnabled`：
   - `NATIVE_DEPTH_TEST_ENABLED`
   - `NATIVE_DEPTH_FUNC_LESS`
   - `NATIVE_FOG_ENABLED`
   - `native_depth_lequal_begin` 直接使用缓存状态；
   - `native_nofog_begin` 直接使用缓存状态。

3. 状态缓存版 AVD 成功进入第一帧，随后崩溃于：
   - `gfxstream::guest::GLClientState::removeFramebuffers`
   - `touchHLE::frameworks::opengles::eagl::read_renderbuffer`
   - 即 AVD 的 slow-present `glReadPixels` / FBO 路径，而不是 z/depth/no-fog 逻辑。
   - 说明 AVD 不能作为最终 present/画面截图判定依据；真机仍必需。

相关证据：

- `artifacts/avd-zfix-20261021_002157/logcat-retry-20261021_002335.txt`
- `artifacts/avd-zfix-20261021_002157/logcat-cached-20261021_002953.txt`
- `artifacts/avd-zfix-20261021_002157/logcat-fogcached.txt`

最新 x86_64 诊断包：

- `artifacts/diagnostics/DeadSpace-x86_64-target24-scale2-screen2-nofog-fontfix-zfix085-lequal-statecache-*.apk`
- 当前源码已包含 depth/fog 状态缓存，arm64 最终交付包需在 PC 主线移植完成后重建并真机验证。

当前策略：等待 PC 主线完全定稿，再逐项抄作业移植，不继续自行猜测 PC 修法。

## 2026-10-21 07:38 启动设置层与真实 FPS 覆盖

新增 Android `LauncherActivity`：

- 独立于 SDLActivity，游戏 native/GL 启动前显示；
- 底部启动按钮固定；
- 设置持久化到 SharedPreferences；
- 支持 1×/2×/3× 内部渲染、30/60/不限帧率、快速呈现、详细日志、帧转储、PC 兼容修复、FPS 覆盖。

新增实际 present FPS 统计：

- Rust `DS_FRAME_HISTORY` 保存最近 1024 帧间隔；
- JNI `nativeGetFrameStats()` 返回平均 FPS、1% low、最低 FPS；
- 覆盖层同时显示平均帧时间与累计 frame 号；
- 不依赖 Choreographer 或 SurfaceFlinger 刷新率。

真机 A/B：

- 2×：约 44–52 FPS，1% low 约 12 FPS，最低 9–11 FPS；
- 3×：约 38.5 FPS，1% low 11.5 FPS，最低 10.2 FPS；
- 1×：约 47.3 FPS，1% low 11.3 FPS，最低 9.6 FPS；
- 降低内部渲染分辨率没有改善瓶颈。
- SurfaceFlinger 合成层接近 60Hz，但 SDLThread 单线程占用约 85%，实际 present 只有 44–52 FPS。

快速呈现：

- `--fast-present` 在真机产生黑一帧、亮一帧交替闪烁；
- 截图全黑，FPS 约 25.8，1% low 7.8；
- 已保留启动页实验开关，但默认强制关闭。
- 当前真机设置已恢复：2×、60 FPS、FPS 显示开启、日志/转储/快速呈现关闭、PC 兼容开启。

最新交付包：

- `artifacts/diagnostics/DeadSpace-arm64-target24-settings-fps-stats-20261021_073803.apk`
- SHA-256 `E4DE5F072196D1F8EAA651C1653EAC4179ABE03BAE908663DA5EF51CCE9C6E46`

## 2026-10-21 07:48 性能修复候选

新增两个读回层优化：

- `read_renderbuffer` 缓存来源 FBO，不再每帧 Gen/Delete FBO；
- 新增 `TOUCHHLE_DS_SKIP_READBACK_FINISH=1` 诊断开关，可跳过每帧 glFinish；
- 启动页新增“跳过读回 Finish（实验）”开关，默认关闭。

修正 fast-present：

- `recomposite_if_necessary` 在 fast_present 时直接跳过 Core Animation 重组；
- 目标是避免 direct EGL 帧与 CA 黑色帧交替；
- 仍需真机确认是否消除“黑一帧、亮一帧”。

性能结论补充：

- 2×/1×/3× 的 1% low 都在 11–12 FPS 左右，说明降分辨率不是主要解决方案；
- 现阶段优先验证 fast-present + readback FBO cache + skip Finish，而不是继续降画质；
- 若 fast-present 修复仍不稳定，下一步需要做 PBO/异步读回或 EGLImage/共享纹理直出。

候选包：

- `artifacts/diagnostics/DeadSpace-arm64-target24-settings-fps-readbackfix-20261021_074820.apk`
- SHA-256 `445E9E21BB690A6C7B7ACD91DB70D1C7C18839D5419A18EEE79F86BE4BE0E4A2`
