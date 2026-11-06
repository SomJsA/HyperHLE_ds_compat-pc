# Dead Space PC → Android 原生 GLES1 移植检查表

更新时间：2026-10-21

## 原则

PC 主线是渲染修复的唯一权威来源。PC 未定稿前，不为相似症状继续自行猜测修法。Android 只移植 PC 已经通过画面与回归验证的改动，并保留对应开关和回退路径。

## 等待 PC 主线提供

1. PC 修复提交哈希或完整 diff。
2. 最终启动参数和环境变量。
3. 修复前后黄金帧：同场景、同相机、同存档。
4. 回归日志：投影矩阵、深度状态、裁剪层、glReadPixels/present。
5. 涉及文件：`src/gles/gles1_on_gl2.rs`、`src/frameworks/opengles/eagl.rs`、`src/window.rs`、`src/font.rs` 及其他新增文件。
6. 确认 `--force-composition`、`--landscape-right`、`TOUCHHLE_DS_SCREEN_SCALE=2` 是否仍必需。

## Android 基线

- 分支 `master`，基线 commit `cadd485`，源码改动未提交。
- Android 原生 GLES1：`src/gles/gles1_native.rs`。
- PC GLES1-on-GL2：`src/gles/gles1_on_gl2.rs`。
- Android 入口：`android/app/src/main/java/org/touchhle/android/MainActivity.java`。
- Dead Space 默认参数：`src/lib.rs` 的 `com.ea.deadspace.bv` 分支。
- 构建脚本：`tools/build-android.ps1`。

## 已对齐或已有候选

- 中文：`--preferred-languages=zh-Hans,zh`。
- 2× 内部渲染：`--scale-hack=2`。
- 字体 remap：真机读档页 `档案 1+`、`05:04:33` 已验证正确。
- no-fog、glyph scale/dy、alpha 模式已移植。
- 服务器不可用弹窗已定向屏蔽。
- 全屏重复断言已写入源码，最新真机包待验证。
- 投影 z 行压缩与 `GL_LESS→GL_LEQUAL` 已加入 native 候选。
- `SCREEN_SCALE=2` 已加入 Android 默认，真机待验证。
- depth/fog 状态缓存已加入，避免 AVD 的 `glIsEnabled()` encoder 崩溃。

## 待 PC 定稿后逐项对照

| PC `gles1_on_gl2` | Android native 当前情况 | 动作 |
|---|---|---|
| `proj_z_zero_begin/end` | 已有 `native_proj_z_scale_begin/end`，默认 0.85，未真机验证 | 用 PC 最终缩放值和条件替换 |
| `GL_LESS→GL_LEQUAL` | 已有 `native_depth_lequal_begin/end` 和状态缓存 | 对照 PC 是否仍默认开启 |
| `TOUCHHLE_DS_FRUSTUM_FAR_SCALE` | 未移植 | 如 PC 使用，补到 native `Frustumf/x` |
| CPU matrix-palette skinning | palette 调用默认 no-op，扩展串被过滤 | 确认 PC 是否依赖；需要时移植 CPU skinning 或原生转发 |
| `TOUCHHLE_DS_NO_FOG` | 已移植 | 真机验证黑层与性能 |
| `TOUCHHLE_DS_DEPTH_LEQUAL` | 已移植 | 与 z-fix 一起 A/B |
| `SCREEN_SCALE=2` | 已加入默认 | 真机验证 UI、粒子和血液 atlas |
| `--force-composition` | 未显式传入，当前实际 slow present | 按 PC 最终配置决定 |
| `--landscape-right` | 未显式传入，当前方向正确 | 不轻易改，需触摸坐标回归 |
| `TOUCHHLE_DS_FORCE_NO_CULL` | 未移植 | 仅 PC 确认需要时增加 |
| 各类 texture/draw/proj trace | 未移植 | 诊断工具，按需补 |

## 验证门槛

1. 构建成功，无新增 fatal/panic。
2. x86_64 AVD：能过 first frame，不在 depth/fog/z-fix 路径崩溃；AVD slow-present 黑屏单独记录。
3. arm64 真机：中文读档页、第一章黑层/黑色球体、海报、血液粒子、2× 清晰度、帧率、第 12 章和无重力跳跃、HOME/后台生命周期。
4. 保存截图、app-only logcat、SurfaceFlinger latency、CPU/GPU 采样、SHA-256。
5. 新 APK 唯一命名，不覆盖旧构件；输出 README、BUILD-INFO、SHA256。

## 当前候选

- arm64：`artifacts/diagnostics/DeadSpace-arm64-target24-scale2-screen2-nofog-fontfix-zfix085-lequal-20261021_001613.apk`
  SHA-256 `2DFB7D4CDFF5471E622DB4C96B86A6BD5CBDD2102DA720CFC0B5DDAD38E515EF`
- x86_64：`artifacts/diagnostics/DeadSpace-x86_64-target24-scale2-screen2-nofog-fontfix-zfix085-lequal-statecache-20261021_003524.apk`
  SHA-256 `41C917F395DF633516CDD1C0CFF7D0EE17B5189D42E0697556B82FF84F7FD16E`

候选包不等同于 PC 主线最终修复。PC 定稿后需重新构建 arm64 并真机验收。