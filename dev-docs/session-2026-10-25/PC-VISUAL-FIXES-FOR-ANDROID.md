# 给安卓侧：继承 PC 已完成的内部纹理与画面修复

> 本文是较早一轮 PC→Android 交接。后续已确认 Android 仍可见扫描线，且开局
> 雾、命中血迹深度与尺寸存在新缺口。执行与验收请先看
> [`ANDROID-PC-VISUAL-PORTING-GUIDE.md`](./ANDROID-PC-VISUAL-PORTING-GUIDE.md)。

> **2026-10-26 补记：** 开局雾、血迹深度/半尺寸和伪 4× MSAA 黑屏已在小米平板
> 实际命中；扫描线仍未解决。不要继续用“代码存在”或旧 APK 截图代替设备验收。
> 当前唯一接手入口见 [`ANDROID-HANDOFF-20261026.md`](./ANDROID-HANDOFF-20261026.md)。

这是 PC 侧给安卓开发的接入说明。用户要的是把 PC 已做好的纹理/画面修复带到安卓，不是备份安卓工程。
此前 source.zip 是额外归档，不是本任务的交付重点；不需要安卓侧下载或重新导入整套源码。

PC 源码仓库：`E:\CodexProjects\DeadSpaceLegacy\repo\HyperHLE_ds_compat`。
下面路径相对该仓库。当前集成分支 `codex/android-mainline-merge` 已有这些代码，旧安卓分支应先核对是否缺失。
PC 用户已确认岩石、雾中渐显、脸部灯带及 E/F 修复有效；E/F 属于输入，不在本说明的移植范围。

## 可以继承的作业

| PC 已处理的问题 | 直接查看的代码 | 安卓侧接入要点 |
| --- | --- | --- |
| 内部墙面/海报 atlas mip 链与过滤 | `src/frameworks/opengles/gles_guest.rs` 的 `ds_try_auto_opening_mips` 及其上传调用点；`src/gles/gles1_on_gl2.rs` 的 `texfilter_begin/end` | 保留纹理内容 key 和 draw 范围限制；海报小面片的过滤不能扩散到同 atlas 的整面墙 |
| 不同资源档位导致修复不命中 | 同上 `ds_try_auto_opening_mips`，提交 `38ec3ce6` | 同时覆盖 phone 和 iPad2 atlas key；按实际内容 key 匹配，不依赖运行时 GL texture ID |
| 舷窗/室内贴花被雾染黑，远处走廊又需要渐显 | `gles1_on_gl2.rs` 的 `nofog_begin/end`，提交 `9b8d476a`、`261fe7a4` | 保留正常距离雾；只对指定 draw 或无效线性雾区间抑制雾，不能全局关 fog |
| 一次 draw 的雾设置污染后续材质 | `gles1_on_gl2.rs` 的 DrawElements 收尾与 `nofog_end`，提交 `75587095` | float-array 和 fixed-array 路径均恢复临时状态，包括提前退出路径 |
| atlas 边缘采样/重复 | `gles1_on_gl2.rs` 的 `texwrap_begin/end` | 继承最终作用域和恢复逻辑，避免全局改 wrap；以当前文件为准，不逐个重放已回滚实验 |
| 岩石缺块、前景遮挡及地基显示 | `gles1_on_gl2.rs` 的 `texnodepth_begin/end`、`no_cull_hash_count_begin/end`；`d4c65100`、`75587095` | 岩石 `d1feb687:600` 必须正常写深度、保留原始剔除；不能把它当“不写深度”的背景层 |
| 结局脸上残留头盔灯带 | `src/environment.rs`、`src/environment/ds_carrie_patch.rs`，提交 `2ac477c4` | 这是游戏模型可见性修复，跨宿主平台复用；符号及 8 个指令字匹配后才应用，保留版本保护 |

纹理识别基础在 `src/ds_texid.rs`。mip 修复覆盖的资源 key 包含 `7027023c`、`7bcfa36c`、`780d00eb`、`56c62e8a`、`18c949fb`、`d1feb687`、`2131afd5`；仍应继承函数内的格式、目标等守卫，不能仅凭 key 强制处理所有上传。

## 安卓移植时特别注意

1. `gles_guest.rs` 纹理上传处理和换头补丁处于共享层，优先核对安卓是否已直接继承；不需要复制 PC 键盘输入代码。
2. `gles1_on_gl2.rs` 的 draw 状态修复只在执行该后端时生效。先确认安卓实际使用的 GLES 后端；若走 `gles1_native.rs` 等路径，需要把对应语义适配到该路径，不能只因文件在仓库里就宣布生效。
3. PC 的若干修复由启动器设置环境变量开启。配置参考 `tools/pc-runtime/Program.cs`，移植相关值到安卓自己的配置入口，不搬 Windows 启动器。
4. 检查 `TOUCHHLE_DS_NO_FOG*`、`TOUCHHLE_DS_TEXFILTER_*`、`TOUCHHLE_DS_AUTO_OPENING_MIPS`、`TOUCHHLE_DS_DEPTH_LEQUAL_*` 及剔除相关配置。不要恢复旧的全局无雾、岩石不写深度、不剔除、depth reset 或 destination fill 实验。
5. PC 用户验收证明修复方向有效；安卓仍需核对资源 key、实际后端和设备画面。不要把 FSR/锐化或超分当作内部材质修复的替代品。

## 查代码与提取补丁

先阅读上表中的函数和原始提交，再按最终代码移植。`75587095` 混有输入及文档变更，不能整提交盲目 cherry-pick。
已提取的代码在：

`E:\CodexProjects\DeadSpaceLegacy\backups\android-visual-handoff-20261025\visual-fixes`

- `reference-source/`：PC 当前相关文件参照，完整 gles_guest.rs 也含输入，不要整份覆盖安卓文件。
- `renderer-net-from-358c5731.patch`：相对该基线的最终渲染器差异，包含诊断代码，供审查或针对旧基线迁移。
- `carrie-head.patch`：独立灯带修复。
- `history/`：定位历史原因，含后来被回滚的实验，不能与净补丁叠加应用。

若安卓基于当前集成主线，修复源码已在分支中；实际工作是核对后端覆盖、配置是否生效及设备验收，而不是再次导入源码。

## 安卓画面验收清单

观察墙面和海报是否清楚、舷窗和室内材质是否被染黑、远处走廊是否随雾渐显；检查岩石连续且不错误遮挡前景；结局换头后灯带不覆盖脸部。
对比时保留同一机位和资源档位，记录实际渲染后端、命中的 atlas key 与相关开关。
开场运镜转可操作阶段的局部黑场（旧录像 3508–3556）仍未解决，不能将其列为可抄的已完成修复。
