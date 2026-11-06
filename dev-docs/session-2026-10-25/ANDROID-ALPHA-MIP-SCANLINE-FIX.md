# Android 扫描线修复：alpha-aware CPU mip

> 补记：此处的旧验收不能代表完整开场无扫描线。用户后续录像仍复现甲板与远景
> 深度损坏，已确认场景附件被降为 D16；见
> [ANDROID-DECK-DEPTH24-FIX-20261026.md](./ANDROID-DECK-DEPTH24-FIX-20261026.md)。

日期：2026-10-26

设备：小米平板 `2405CRPFDC` / Android 14 / `10.31.0.232:42417`

范围：只测试该平板；另外两台设备未参与本轮验证。

## 1. 结论

小米平板的开场船体重复彩色/黑色横带已闭环。根因不是 FSR、CAS、MSAA、
无限远投影、全局深度或剔除，而是**按内容 key 命中的 RGBA PVRTC 船体 atlas
使用普通 GPU mip 时，透明黑区的颜色被平均进可见表面**。

最终构建在默认配置下稳定命中 5 个 `alpha_aware_cpu=true` 纹理。与关闭修复的
同档位、同时间轴截图相比，主要密集横带消失；星场、岩石、字幕和站体没有出现
整块黑化或跳过几何。

## 2. 定位证据

`TOUCHHLE_DS_PIXEL_TRACE` 在亮船体像素上确认：

```text
0712153f:11427  [1,1,3] -> [137,142,171]
24bec7ad:8643
60bb1a47:4503
e0549cfb:1224
02d3486c:405
```

其中 `0712153f:11427` 是实际把目标像素从背景色写成亮船体色的最大批次。
完整日志：

```text
report/xiaomi-pad-scanline-20261026/hull-point-30-35.log
```

已证伪：

- 全局 LINEAR；
- 普通 `GenerateMipmapOES`；
- 各向异性；
- clamp；
- LOD bias；
- 全局无深度；
- 全局不剔除；
- 无限远投影；
- FSR/CAS/fast-present/GPU-copy。

普通 GPU mip 的截图甚至出现新的块状黑斑，说明简单重建 RGBA mip 不是正确修复。

## 3. 实现

位置：`src/frameworks/opengles/gles_guest.rs`

新增开关：

```text
TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH=<content-key>[,<content-key>...]
```

仅当纹理已经通过现有 `ds_try_auto_opening_mips` 的目标、格式、尺寸和 payload
守卫后，才由 level 0 生成 CPU mip：

1. 对 2×2 源像素计算 alpha 和；
2. RGB 使用 `sum(rgb * alpha) / sum(alpha)`；
3. alpha 使用独立平均；
4. 全透明区域使用普通 RGB 平均，避免黑边扩大；
5. 每个 level 通过 `TexImage2D(GL_RGBA)` 上传；
6. 设置 `LINEAR_MIPMAP_LINEAR`；
7. 标记纹理已替换，忽略游戏随后上传的坏 mip。

Android 默认 key 位于 `src/lib.rs`：

```text
0712153f,24bec7ad,60bb1a47,e0549cfb,02d3486c
```

同时设置：

```text
TOUCHHLE_DS_AUTO_MIPS_HASH
TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH
```

不得改成 `AUTO_MIPS_ALL=1`。

## 4. 实测结果

最终包：

```text
android/app/build/outputs/apk/release/app-release.apk
size=593763543
sha256=F7F0B8E7DD84E7F0DEA22FCA1D53EDCE5E22E089BFA107395A247D1B237EF713
```

签名：`apksigner` v1、v2 均通过。

默认启动日志：

```text
[DSTEX opening-mip] rebuilt mip chain key=e0549cfb ... alpha_aware_cpu=true
[DSTEX opening-mip] rebuilt mip chain key=60bb1a47 ... alpha_aware_cpu=true
[DSTEX opening-mip] rebuilt mip chain key=0712153f ... alpha_aware_cpu=true
[DSTEX opening-mip] rebuilt mip chain key=02d3486c ... alpha_aware_cpu=true
[DSTEX opening-mip] rebuilt mip chain key=24bec7ad ... alpha_aware_cpu=true
```

证据根目录：

```text
E:\CodexProjects\DeadSpaceLegacy\report\xiaomi-pad-scanline-20261026
```

关键文件：

- `scale3-no071-vs-cpumip-t12.jpg`：scale-hack 3 同时间轴开关 A/B。
- `scale3-mip-matrix-t12.jpg`：关闭、仅 `0712153f`、五 key 组合三联图。
- `final-default-scale3-t12.png`：最终默认包实机截图。
- `final-default-scale3.log`：默认包 5 个命中日志。
- `final-alpha-mips-opening.mp4`：最终包连续运镜。
- `gpu-vs-cpu-mips-t12-crop.jpg`：普通 GPU mip 与 CPU alpha mip 的局部对照。

## 5. 回归注意

- 只允许对已定位 content key 启用，不要全局扫描并替换所有 mip。
- 当前结论只覆盖小米平板；其他 GPU 仍应检查启动日志是否命中 5 个 key。
- 修复保留游戏设计中的真实面板结构线，不把大型船体整体涂黑或跳过。
- 若后续更换 IPA 资源档，先确认这 5 个内容 key 是否仍一致。
- 若同一位置再次出现条带，先用 pixel trace 证明覆盖批次是否变化，再扩大 key 列表。
