# Android 重适配 PC 纹理与画面修复：实施、症状分诊与验收指南

更新时间：2026-10-26（Asia/Hong_Kong）

本文是安卓侧重新适配 PC 纹理、深度、雾、粒子与后处理接入工作的入口文档。
它不把“代码在同一个仓库里”当作“Android 已经执行”，也不把旧 APK 的一次启动
成功当作最新 PC 画面修复已经全部继承。

最近一次用户观察补充：Android 曾在小米平板上亲眼看到扫描线。2026-10-26 已
完成 key/count 定位与 alpha-aware CPU mip 修复；其他设备仍未实测，不能把
小米平板结论无条件外推到所有 Android GPU。

> **2026-10-26 小米平板实测快照（候选包
> `F7F0B8E7DD84E7F0DEA22FCA1D53EDCE5E22E089BFA107395A247D1B237EF713`）：**
> 开局极小正值雾、血迹深度/半尺寸、伪 4× MSAA 黑屏回退均已命中实机；
> HOME 15 秒恢复连续 3/3 通过；游戏内“返回启动器→重新启动”连续 3 轮通过。
> 扫描线已完成小米平板同时间轴 A/B：默认包稳定命中 5 个
> `alpha_aware_cpu=true` 船体/站体 mip，主要彩色和重复横带消失。
> 当前接手入口和证据索引见
> [`ANDROID-HANDOFF-20261026.md`](./ANDROID-HANDOFF-20261026.md)。

## 1. 结论先行

### 1.1 Android 实际执行的后端

Dead Space 在 Android 上走 OpenGL ES 1.1 的进程内驱动，后端主体是：

- `src/gles/gles1_native.rs`

PC 桌面使用的 GL 2.1 兼容后端是：

- `src/gles/gles1_on_gl2.rs`

共享纹理上传层是：

- `src/frameworks/opengles/gles_guest.rs`

因此必须分三类处理：

| 类别 | 可能自动继承 | 仍需 Android 设备实证 |
| --- | --- | --- |
| 共享上传、内容指纹、PVRTC 解码、字形、FBO 包装 | 是 | 是 |
| `gles1_native` 的 draw-time 状态 | 否，必须看 native 是否存在对应函数和调用 | 是 |
| 只在 `gles1_on_gl2` 中存在的桌面实验 | 否 | 不应直接复制，先判断是否真是最终修复 |

启动日志必须出现 `GLES1Native`，或至少能在
`GLESImplementation::GLES1_IMPLEMENTATIONS` 的选择结果中证明实际后端。不能仅凭
源码文件存在就宣布生效。

### 1.2 当前最重要的判断

1. **扫描线已在小米平板闭环；其他设备仍未测试。**
   `DISTANCE-RENDER-2026-10-22.md` 和 `VIDEO-AUDIT-20261023.md` 记录：mip、
   各向异性、clamp、LOD、全局无深度、全局不剔除、强制无限远投影等 A/B 都未
   消除船体上的密集水平/斜向暗带。最终有效修复不是这些全局实验，而是对
   `0712153f:11427` 等精确船体批次生成 alpha-aware CPU mip；透明区不再把
   黑色和杂色混进船体颜色。2026-10-26 小米平板在 `scale_hack=1/3` 均完成
   同时间轴 A/B。
2. **开局“极小正值雾范围”修复已进入 `gles1_native` 并通过实机。**
   Android 日志命中极小正值 LINEAR 黑雾，范围恢复为 PC 的 `10..25`；
   过场未再出现仅角色/HUD 可见的环境全黑。
3. **命中血迹的深度遮挡与 0.5 尺寸修复已进入 `gles1_native` 并通过实机。**
   小米平板“5 分钟杀敌”中实际命中敌人，画面出现红色血粒子，日志同时命中
   `enabling scene depth for PC hit-blood particles` 和
   `applied PC hit-blood half-size quads`。本次样本未出现 VBO/EBO 回退日志；
   后续若发现 `kept original VBO/EBO draw`，应按新缺口处理。
4. **伪 4× MSAA 已按驱动能力回退。** Adreno 请求 4× 但实际只有 1× 时不再提交
   无效多重采样 FBO；日志为 `[DSmsaa] requested 4x but driver/FBO setup failed;
   falling back to 1x`，黑屏世界恢复。
5. **返回启动器后的生命周期 ANR 已修复。** 游戏进程与 Launcher 进程隔离；
   终止回调结束后给 guest 2 秒收尾，再回收 `:game` 进程。连续 3 轮
   “游戏内返回→Launcher→重新启动”均未再出现输入 ANR。
6. 旧的有限远投影、有限范围的 `GL_LEQUAL`、岩石深度、站体不剔除、海报小片
   LINEAR、定向去雾、共享 mip、上传同步等确实已有 Android 适配记录；它们仍需
   在当前重新打包的 APK 上复验，不能拿来抵消上面的新缺口。

### 1.3 状态标记

本文统一使用以下标记：

- **A：Android 已适配，历史设备证据存在，当前包仍待复验**
- **S：共享层代码存在，理论上可继承，尚缺当前 Android 设备证据**
- **N：已核实 Android native 未适配**
- **O：已确认现象，但修复本身仍未闭环**
- **P：PC 诊断工具或实验，不默认移植**

## 2. 当前适配矩阵

| 项目 | 状态 | 当前证据/缺口 | 下一步 |
| --- | --- | --- | --- |
| PVRTC 干净 mip 重建 | A | 旧 GPU mip 会产生透明区污染；新 alpha-aware CPU mip 已实机命中 | 保持 key 精确匹配，其他 GPU 需独立回归 |
| phone/iPad2 内容指纹差异 | A/S | 覆盖 `7027023c...` 与 `56c62e8a...` 两套 key | 在实际资源档上确认命中 |
| 纹理上传同步 | A/S | `TOUCHHLE_DS_TEX_UPLOAD_FINISH=1` 已由 Java 和 Rust 默认设置 | 复验转场黑闪 |
| 有限远投影/开局黑团 | A | `native_adjust_frustum_far`、`native_proj_z_fix_begin` | 同机位冷启动复验 |
| 背景共面层 `GL_LEQUAL` | A | `native_texnodepth_begin`，排除岩石 `d1feb687:600` | 星空、墙面、海报复验 |
| 背景层不写深度 | A | `TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE=1` | 检查后续几何不被错误遮挡 |
| 岩石/地基深度 | A | `d1feb687:600` 不进入错误的无深度规则 | 岩石连续且正常遮挡 |
| 站体栏杆/地基反绕序 | A | `02d3486c:405` 有限范围关闭剔除 | 完整几何且无幽灵面 |
| 舷窗/墙面封面深度 | A | `TEXNODEPTH_HASH_COUNT`，开局 6-index 特判 | 黑三角、缺块复验 |
| 海报小片 LINEAR | A | `25a08a82` 且 count <= 6 | 海报清晰且墙面过滤不被污染 |
| 正常距离雾 | A | `NO_FOG=0`，只豁免 `7027023c,56c62e8a` | 远处走廊仍需自然渐显 |
| 开局极小正值雾 | **A / pass** | native 上下文已接 `ds_opening_fog`；实机确认 range `10..25` | 保持回归 |
| 扫描线/船体暗带 | **A / pass（小米平板）** | trace 定位 `0712153f:11427` 等批次；默认包 5 个 CPU mip 命中；同时间轴 A/B 通过 | 其他设备只做回归，不重复全局开关试错 |
| 命中血迹总在最前 | **A / pass** | native 按三种内容 key 临时启用深度；实机命中并出现血迹 | 补前景遮挡场景的重复回归 |
| 命中血迹 2 倍大 | **A / pass（本次样本）** | Android native 已接入 `ds_blood_size.rs`；实机日志命中半尺寸 quad | 后续若出现 VBO/EBO 回退，补 buffer 支持 |
| 伪 4× MSAA 黑屏 | **A / pass** | 驱动实际仅 1× 时安全回退；世界、HUD 和时间轴恢复 | Adreno/其他 GPU 重复 |
| 返回启动器后再启动 ANR | **A / pass** | `:game` 独立进程 + 终止后回收；3 轮 ADB 回归通过 | 冷启动/强制终止另行压力回归 |
| 字幕/字形尺寸与 alpha | S | `font.rs`、shared `gles_guest` 修复 | 当前 APK 检查字幕和菜单 |
| 拾取窗/工作台离屏布局 | S | 共享 `gles_guest` 的 scale-hack/FBO 修复 | 检查完整标签和布局 |
| 结局脸部残留灯带 | S | `ds_carrie_patch.rs` 为共享环境补丁 | 用结局存档复验 |
| 畸形三角形/逐三角诊断 | P | 主要是 PC draw 后端诊断工具 | 只有命中证据后才移植 |
| 全局 PVRTC mip、全局不剔除、每帧清深度、destination fill | P | PC 已多次证伪或仅用于诊断 | 禁止设为 Android 默认 |

## 3. 症状、分诊与验收总表

验收时不要只写“看起来正常”。每一项至少要有：同机位截图或短视频、命中日志、
开关状态和结论。

| 症状 | 最容易误判为 | 首选判断/修复 | Android 验收检查点 |
| --- | --- | --- | --- |
| 远处海报或墙面有梳状横纹、模糊 | 分辨率低、FSR/CAS 差 | 先查 PVRTC mip 是否污染；仅对已知 key 用 level 0 重建完整 mip | 静止和运镜都清楚；日志有对应 key 的 `[DSTEX opening-mip]` |
| phone/iPad2 其中一档不修复 | Android 后端没执行 | 同逻辑图在不同 IPA 中内容 hash 不同 | 在两档各截同机位；按 content key 命中，不靠 GL texture id |
| 大型船体有密集水平/斜向暗带（扫描线） | 普通 mip 条纹、UI 扫描线 | RGBA PVRTC 透明区污染 GPU mip；对已定位 key 使用 alpha-aware CPU mip | 启动日志命中 `alpha_aware_cpu=true`；同时间轴开关 A/B；不得用整屏黑色覆盖 |
| atlas 边缘重复亮条 | 纹理本身花纹 | 检查 wrap 模式和 UV 越界；只对确认批次临时修改并恢复 | 边缘无重复条，邻近纹理和后续材质不变化 |
| 海报出现黑三角/斜切块 | 海报贴图损坏 | 共面透明层深度冲突；使用 key+count 限定修复 | 海报边框、文字和图案完整；墙面无新黑块 |
| 海报整张发糊 | mip 太差 | 小面片临时 LINEAR，不能把整张 atlas 都改成 LINEAR | 海报小片清晰；大墙面保持原有过滤 |
| 舷窗、室内贴花、窗面发黑 | 曝光或贴图黑 | 检查极小正值雾和定向去雾；保留正常距离雾 | 正式版环境可见且远处仍渐显；可用 PC 红雾 A/B 辅助判因 |
| 一个 draw 后后续材质突然全黑 | 游戏材质坏 | 临时雾/深度/过滤状态泄漏 | 连续室内外镜头不变色；日志命中后紧接的材质正常 |
| 开局过场墙面黑、角色和 HUD 正常 | 贴图黑、深度 | 极小正值 LINEAR 黑雾。红雾 A/B 可区分 | 过场中墙面、角色同时可见；进入正常房间后雾渐显仍在 |
| 开门转场偶发一整帧 atlas 发黑 | 资源损坏 | 上传与渲染线程竞争；检查上传 Finish 同步 | 3 次冷启动、3 次后台恢复均无单帧黑闪 |
| 岩石缺块、地基消失、岩石挡住前景 | 剔除错误 | `d1feb687:600` 应保留 `GL_LESS`、深度写入和原剔除 | 岩石连续；前景遮挡符合几何；无反向缺面 |
| 站体栏杆/地基缺一大块 | 深度写错 | 反绕序批次 `02d3486c:405` scoped no-cull | 完整栏杆/地基；其他物体不出现幽灵背面 |
| 舷窗/墙面透明封面缺块 | 透明混合坏 | 仅指定 key/count 不测深度、不写深度 | 封面完整；深度测试不污染后续场景 |
| 星空、月面、星云被黑块盖住 | 星空贴图黑 | 共面背景 tie；`GL_LEQUAL` 只用于背景 key、且不写深度 | 星云和星空连续；玩法几何没有黑楔 |
| 开局只有 clear color 或超大黑团 | 黑贴图 | 有限 far 被裁剪；检查投影 `m[10]`/`m[14]` 与画幅范围 | 石壁、站体、星空、月面同时出现 |
| 命中血总是盖在角色/枪/墙前 | 血迹红色 alpha 错 | 血粒子 draw 临时开启深度测试，保持不写深度 | 近、中距离射击；前景遮挡正确，血仍可见并正常消退 |
| 命中血大约大两倍 | 屏幕缩放或 DPI | 每片粒子四边形绕自身中心缩至 0.5；中心、UV、运动不变 | 喷雾、血斑、血滴自然；不缩小烟雾、HUD、准星 |
| 字幕不可见、偏大、发黑 | 字体宽度或语言问题 | 检查 shared glyph scale、alpha 语义和扩展 alpha 纹理 | 中文/英文菜单、字幕、拾取说明均清晰 |
| 拾取悬浮窗/工作台只显示局部 | 图集 UV 错 | 离屏 `DynamicTexture` 的 FBO/viewport 必须使用自身尺寸 | 整个窗口和文字布局都出现，背景不再被二次放大 |
| 结局换头后灯带盖脸 | 脸贴图坏 | `ds_carrie_patch.rs` 同时隐藏残留发光层 | 脸部完整；无头部缺失；其他灯效不消失 |
| 所有上述缺陷只在 FSR/CAS/3x 下出现 | 纹理/深度修复没生效 | 输出后处理或显示缩放问题 | neutral 输出下先过；生产画质下再单独过 |

## 4. 安卓重适配顺序

### P0：先建立当前 Android 的视觉失败基线

不要先改代码。按同一存档、同一机位、同一资源档录一组当前 APK 基线：

1. 冷启动到标题页。
2. 完整开场过场，重点包含大型船体、舷窗、海报、岩石/地基、站体栏杆和转场。
3. 第一章走廊静止、左右转向、前后移动。
4. 近距离和远距离各射击敌人一次，并让血出现在墙后、角色后、角色前。
5. 打开背包、拾取说明和工作台。
6. 字幕、菜单焦点、切后台后恢复。
7. 有结局存档时验证换头。

每组至少保留：截图/视频、APK SHA256、后端、GPU/API、开关、命中 key。

### P1：扫描线分型

先把“扫描线”分成两个完全不同的问题：

| 类型 | 表现 | 处理 |
| --- | --- | --- |
| 游戏设计线条 | HUD 青色横线、全息面板、交互环纹理 | 不应删除；与官方画面比较 |
| 船体/建筑表面暗带 | 大型白色船体有密集水平或斜向条，运镜时随纹理移动 | 图形缺陷；进入 draw/纹理诊断 |

诊断必须记录：

- 实际 content key；
- draw count、mode、纹理绑定和过滤；
- 该 draw 的屏幕 NDC 范围；
- 透视或正交投影；
- texture matrix；
- 顶点 UV 范围；
- 是否混合、是否写深度、是否剔除。

若证据证明是小的全屏黑白 dither/coverage mask，PC 的窄范围思路是：

```text
TOUCHHLE_DS_TEXBLACK_HASH_COUNT=<key>:<count>[,<key>:<count>]
```

它在 draw 时临时关闭纹理/光照/混合/alpha test，并把颜色设为不透明黑，
只替换精确命中的 mask 批次。Android native 当前没有这个函数，不能在未命中
key/count 时直接添加。

若暗带位于船体/建筑本身，则不要使用 `TEXBLACK`。本轮已证明主船体批次
`0712153f:11427` 的根因是 RGBA PVRTC 的普通 mip 透明区污染；修复后不能再
回到全局清深度、跳过几何或整块黑化。

扫描线通过标准：

- 整个开场录像中目标船体没有密集重复暗带；
- 星空、海报、雾和站体不能因修复出现新缺块；
- 静止和运镜都通过；
- 若只是把同一表面换成纯黑或整块跳过，判失败。

### P2：移植开局极小正值雾修复

PC 修复针对的是：

```text
正常 10..25 -> 长时间 0..0 -> 极小正值约 0..0.25 -> 恢复 10..25
```

它不是全局最小距离雾，也不是关闭雾。Android native 需要增加上下文局部状态：

1. 只有 LINEAR、黑色雾、透视投影、near 系数约 `-0.1` 时参与。
2. 记住稳定的 `10..25` 基线。
3. `end == 0` 只进入等待，不计时。
4. 首个极小正值形成 `start/end ≈ 0.4` 时开始保护。
5. 恢复期临时把 draw 的 fog start/end 设为 `10/25`，draw 后立即恢复 guest 原值。
6. 恢复到正常范围、遇到无关范围或超过 240 帧时释放。
7. `TOUCHHLE_DS_OPENING_FOG_FIX=0` 可供 A/B 关闭。
8. 每次判断都要独立于其他 GL context，不能做进程级单例状态。

`GLES1NativeContext::make_current` 会创建新的 native wrapper，因此不能把状态只
放在短生命周期的 `GLES1Native` 字段中。可用 context 持有的
`Rc<RefCell<OpeningFog>>` 之类共享句柄，但必须保持单线程 GL 上下文边界。

PC 的 `TOUCHHLE_DS_FOG_RED=1` 和 draw-time `TOUCHHLE_DS_LOG_FOG=1` 目前只在
`gles1_on_gl2` 中实现。Android native 暂时不能使用“黑区变红”作为设备证据；
若确需该判据，应先移植只改雾色和打日志的诊断路径，再确保正式默认关闭。

开局雾通过标准：

- 过场中墙面/地面不再整片黑；
- 角色、灯光、HUD 与墙面关系正常；
- 进入正常场景后远处走廊仍有 10..25 的雾渐变；
- 红雾诊断不再显示“错误黑区其实是雾”；
- A/B 开启日志命中，关闭修复后原症状可复现。

### P3：移植命中血迹深度与尺寸

这组 PC 修复当前位于未提交/未跟踪的工作区文件中，不能假定已有可 cherry-pick 的
提交。Android 侧应以现工作区的 `gles1_on_gl2.rs` 和
`src/gles/ds_blood_size.rs` 为源，分别移植语义，并保留这些未提交改动。

#### 深度遮挡

只匹配内容 key：

```text
00b9b876  fd937e94  b3ca1a51
```

候选 draw 必须同时满足：

- `TRIANGLES`；
- count 是 6 的倍数；
- 2D 纹理已启用；
- alpha blend 已启用；
- 投影是透视，且符合 PC 已验证矩阵条件；
- 当前 depth func 为 `GL_LESS`；
- depth write 已关；
- 原 depth test 是关；
- content key 命中三种血迹之一。

命中后只在 draw 期间 `glEnable(GL_DEPTH_TEST)`，仍保持
`glDepthMask(GL_FALSE)`；draw 结束恢复原 depth-test 状态。不要改变深度函数、
投影、blend、alpha、UV 或位置。

#### 尺寸

PC 最终默认是 `TOUCHHLE_DS_BLOOD_SIZE=0.5`。要求：

- 每个六 index 必须组成一个独立、非退化、且不与邻近四边形共享顶点的两三角片；
- 以该四边形自己的四顶点平均位置为中心；
- 每个轴缩到中心距离的 0.5；
- 未被这些 index 使用的顶点不变；
- 中心位置、UV、纹理、运动、混合和深度遮挡不变；
- 任一四边形格式不满足时，整批回退原样，禁止“改了一半”。

Android native 直接转发客户端顶点，后端若要缩放需短暂复制和替换顶点数组。
必须处理并恢复 client array 与 VBO/EBO 绑定、pointer、size、type、stride。
当前 PC 已验收的血迹样本主要走 DrawElements；DrawArrays 只应在其同样满足独立
四边形证据时支持，不能因为“看起来像血”就缩放所有粒子。

血迹通过标准：

- 10 次近距离、10 次中距离敌人命中，血都正常出现并消退；
- 血在角色、武器、墙后的部分被正确遮挡；
- 血在角色前方时仍完整可见；
- 尺寸接近 PC 已验收版，约原尺寸一半，位置不漂移；
- 烟雾、枪口火焰、HUD、准星、字幕不被缩放；
- DrawArrays/Elements 连续执行后无顶点指针或缓冲绑定泄漏。

### P4：复核已有 shared/native 修复

旧适配不能删除或重写，只需在当前 APK 上逐项回归：

- `src/frameworks/opengles/gles_guest.rs` 的 mip、上传同步和 FBO 包装；
- `src/gles/gles1_native.rs` 的有限 far、key/count 深度规则、海报过滤、雾和剔除；
- `src/environment/ds_carrie_patch.rs` 的结局换头补丁；
- `font.rs` 和 `gles_guest` 的字形/alpha 语义。

如果当前画面只在某个后处理画质档出错，不要回头改写纹理修复；先隔离后处理。

## 5. 共享纹理层适配规则

### 5.1 必须使用内容指纹

Dead Space 会复用 GL texture name，同一 numeric id 在运行中可能装载完全不同的
图片。所有判断都必须走 `src/ds_texid.rs` 的 content key，不得硬编码运行时
texture id。

需要保留 phone 和 iPad2 两套图集 key：

| 角色 | Base IPA | iPad2 |
| --- | --- | --- |
| 墙/海报/舷窗 | `7027023c` | `56c62e8a` |
| 船壳/环 | `7bcfa36c` | `18c949fb` |
| 下层建筑 | `780d00eb` | `d1feb687` |
| 主开场平台 | `e2593d6c` | `2131afd5` |

当前 mip 修复实际还覆盖 `f4d178fa` 等已确认 key；以
`ds_try_auto_opening_mips` 的 matches 列表为准。

### 5.2 PVRTC mip 修复必须保持守卫

正确流程：

1. 只处理 `GL_TEXTURE_2D`。
2. 只处理 2bpp/4bpp PVRTC。
3. 校验 width、height、payload 精确长度。
4. 解码 level 0 为 RGBA。
5. 以 RGBA 上传 level 0。
6. 对 RGBA 船体 atlas 优先由 level 0 生成 alpha-aware CPU mip 链：
   颜色按 alpha 预乘后求平均，alpha 单独求平均；普通逐通道
   `GenerateMipmapOES` 会把透明黑区污染进颜色，产生黑块和彩色扫描线。
7. 设置 `LINEAR_MIPMAP_LINEAR`。
8. 标记该纹理已替换，忽略游戏随后上传的坏 mip。

不要用“全局 `GL_LINEAR`、无 mip、全局重建”替代已知 key 的修复。

### 5.3 上传同步是验证手段，不是性能默认长期方案

`TOUCHHLE_DS_TEX_UPLOAD_FINISH` 的判断是“变量是否存在”，不是值是否为 `1`。
即使设成 `"0"` 也会启用。A/B 时要用真正 unset，而不是写入空字符串。

只有能复现“上传/渲染竞争导致单帧黑 atlas”时才开启。验收通过后仍应记录它是否
保留在默认配置中，并评估首帧/加载时间成本。

## 6. `gles1_native` draw-time 状态硬规则

现有 native 的 `DrawArrays` 和 `DrawElements` 顺序大致是：

```text
投影保护
雾保护
正交覆盖层去雾
纹理过滤保护
深度保护
剔除保护
texture matrix 保护
纹理压平/跟踪
raw draw
逆序恢复
```

新增血迹、开局雾或扫描线修复时，必须继续遵守：

1. 任何临时 `Enable/Disable/DepthFunc/DepthMask/TexParameter/Fogf/MatrixMode/
   BindBuffer/VertexPointer` 都有可恢复 backup。
2. 正常 GL draw 返回后立即恢复，不依赖下一次材质自行纠正。
3. 所有参数守卫都在第一次修改 GL 状态之前完成。
4. `indices == null` 且没有 EBO 的提前返回可以发生在修改前；若修改过状态，
   提前返回前必须恢复。
5. DrawArrays 与 DrawElements 需要相同语义；若某条路径不支持，必须明确回退。
6. 不要用进程级布尔值代替真实 GL 状态；Android 可能有多次 context/重建。
7. 新增日志只打第一次和关键状态变化，避免每帧 logcat 拖慢。
8. 不要在 draw 里做全局 `Clear`、全场景深清、destination fill 或整张纹理替换。

推荐用 RAII guard 或成对的 `begin -> draw -> end`，但提前 return、panic/ unwind
和设备丢失路径也要有明确策略。最低要求是正常路径无状态泄漏。

## 7. 开关与默认值

| 开关 | 建议默认 | 用途 |
| --- | --- | --- |
| `TOUCHHLE_DS_NO_FOG` | `0` | 保留正常距离雾 |
| `TOUCHHLE_DS_NO_FOG_HASH` | `7027023c,56c62e8a` | 只豁免舷窗/墙面 |
| `TOUCHHLE_DS_NO_FOG_ORTHO` | `1` | 2D 覆盖层不参与 eye-space fog |
| `TOUCHHLE_DS_DEPTH_LEQUAL_HASH` | 已适配 key 列表 | 背景共面 tie |
| `TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE` | `1` | 背景 tie 不污染深度 |
| `TOUCHHLE_DS_NO_CULL_HASH_COUNT` | `02d3486c:405` | 站体反绕序批次 |
| `TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT` | 四个 key/count 组合 | 透明封面深度例外 |
| `TOUCHHLE_DS_TEXFILTER_LINEAR_HASH` | `25a08a82` | 海报小片 |
| `TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT` | `6` | 防止扩到整面墙 |
| `TOUCHHLE_DS_AUTO_OPENING_MIPS` | `1` | 已知开场 PVRTC atlas 干净 mip |
| `TOUCHHLE_DS_AUTO_MIPS_ALL` | `0` | 仅诊断，不能默认全局重建 |
| `TOUCHHLE_DS_AUTO_MIPS_HASH` | 五个已定位船体/站体 key | 允许精确 key 进入 mip 修复 |
| `TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH` | 同上五个 key | 生成不被透明黑区污染的 CPU mip |
| `TOUCHHLE_DS_TEX_UPLOAD_FINISH` | `1`（待性能确认） | 同步上传，防单帧黑纹理 |
| `TOUCHHLE_DS_OPENING_FOG_FIX` | 适配后默认开 | 允许 `0` 做 A/B |
| `TOUCHHLE_DS_BLOOD_DEPTH_FIX` | 适配后默认开 | 允许 `0` 对照 |
| `TOUCHHLE_DS_BLOOD_SIZE` | `0.5` | `1` 恢复原尺寸 |
| `TOUCHHLE_DS_TEXBLACK_HASH_COUNT` | 未定位前 unset | 只替换精确定位的小 dither mask |
| `TOUCHHLE_DS_TEXFLAT_HASH` | unset | 诊断纹理面积，不用于正式修复 |
| `TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT` | unset | 诊断 UV/矩阵 |
| `TOUCHHLE_DS_SKIP_TRI_HASH_COUNT` | unset | PC 畸形三角形临时诊断 |
| `TOUCHHLE_DS_PIXEL_TRACE` | unset | 单点前后像素诊断 |

默认值必须同时检查：

- `android/app/src/main/java/org/touchhle/android/MainActivity.java`
  `configureRuntimeOptions`
- `src/lib.rs` 的 Android Dead Space fallback

Java 层允许覆盖，Rust 层只在变量不存在时补默认。布尔变量必须解析值；只有明确
使用“presence semantics”的上传同步开关才可以检查是否存在。

## 8. 分辨率和后处理隔离

纹理修复验收不能和输出放大混在一起。每次记录以下四项：

| 项 | 示例/要记录什么 |
| --- | --- |
| Guest/internal render resolution | 实际 backbuffer 尺寸 |
| Post-processing input/output resolution | 超分前后尺寸 |
| Display/present resolution | 设备窗口实际像素 |
| Output features | FSR、CAS/Sharpen、AA、stretch、present copy |

建议三段测试：

1. **Neutral**：固定 `SCREEN_SCALE`、关闭 FSR/CAS/AA，仅验证纹理/draw 修复。
2. **Production quality**：开启用户实际使用的 FSR/CAS/AA，验证无回归。
3. **Low quality / 3x stress**：只用于确认问题是否由输出放大或 DPI 引入。

判定规则：

- Neutral 有异常：先查纹理上传或 native draw。
- Neutral 正常、Production 异常：查后处理输入/输出分辨率与采样。
- 只有某个 DPI/窗口模式异常：查窗口坐标、viewport、surface size，不重写 atlas。
- 只有 3x 或低内部分辨率异常：不能据此宣称材质修复失败。

后处理质量提升不能替代内部 mip、深度、雾和粒子修复。

## 9. 设备验收步骤

### 9.1 构建前/安装前记录

至少记录：

```text
APK SHA256
内置 IPA SHA256
native library SHA256
package / ABI / SDK
git branch / HEAD / 相关未提交文件 hash
设备型号、SoC、GPU、GLES version
Android version
实际 GLES 后端
资源档（phone/iPad2）
显示分辨率、DPI、SCREEN_SCALE
FSR/CAS/AA/stretch/present copy 设置
存档槽与测试前存档备份路径
```

可用于记录环境和证据的命令示例：

```powershell
Get-FileHash -Algorithm SHA256 <apk-path>
adb shell getprop ro.product.model
adb shell getprop ro.build.version.release
adb shell getprop ro.hardware.egl
adb shell dumpsys package org.touchhle.android
adb shell wm size
adb shell wm density
adb shell dumpsys SurfaceFlinger
adb logcat -c
adb logcat -v threadtime > evidence\logcat.txt
adb exec-out screencap -p > evidence\screenshots\shot.png
adb shell screenrecord --time-limit 180 /sdcard/ds-visual.mp4
adb pull /sdcard/ds-visual.mp4 evidence\video\opening-full.mp4
```

不同 Android 版本对 GLES/GPU 的 dumpsys 输出不同；实际后端仍以应用日志为准。

旧基线包信息仅作为历史参考：

| 项目 | 值 |
| --- | --- |
| APK | `E:/CodexProjects/DeadSpaceLegacy/report/android-mainline-20261025/DeadSpace-IOS-Port-bundled-20261025.apk` |
| APK SHA256 | `F7F0B8E7DD84E7F0DEA22FCA1D53EDCE5E22E089BFA107395A247D1B237EF713` |
| 内置 IPA SHA256 | `1381CBD7DE15BCAF8085EDF8F191CC54261C00376087D4893AC62FB9B9F2DA1C` |

该哈希对应本轮 alpha-aware CPU mip 最终包；仍须结合设备日志、截图和存档回归，
不能只凭哈希确认视觉结果。

### 9.2 日志准备

开始前清 logcat，并保留完整启动到测试结束的日志。至少搜索：

```text
GLES1Native
[DSTEX opening-mip]
TOUCHHLE_DS_TEX_UPLOAD_FINISH
Android native: corrected draw-time finite-far projection
Android native: scoped PC background GL_LEQUAL repair
Android native: disabled culling for PC station-hull batch
[DSopeningFog]
[DSbloodDepth]
[DSbloodSize]
TOUCHHLE_DS_TEXBLACK_HASH_COUNT
```

PC 的 `TOUCHHLE_DS_FOG_RED` 和 `TOUCHHLE_DS_LOG_FOG` 不是 Android native 的
现成入口。共享层目前可记录 fog 参数设置的是 `TOUCHHLE_DS_LOG_FOG_SET=1`；
draw-time native fog 日志需要另行适配。

应用日志存在只证明命中路径；最终结论仍必须由画面验收决定。

### 9.3 场景检查点

| 场景 | 操作 | 通过标准 | 失败证据示例 |
| --- | --- | --- | --- |
| 冷启动/标题 | 清进程后启动，进入标题 | 无旧 atlas 残留、无全黑帧 | 标题前后贴图单帧黑闪 |
| 开场大型船体 | 完整过场，不跳动画 | 无密集扫描线/斜向暗带 | 船体表面重复水平条 |
| 舷窗/窗面 | 过场原速播放 | 窗内环境可见，边缘无黑块 | 舷窗内黑、红雾下变红 |
| 海报 | 同机位停 2 秒并轻微运镜 | 海报完整清晰，无黑三角 | 边框被黑楔吃掉 |
| 岩石/地基 | 开场转场和近景 | 岩石连续、深度正确 | 地基消失或盖住前景 |
| 站体栏杆 | 过场扫过站体 | 几何完整，无幽灵背面 | 栏杆缺段/反向面 |
| 星空/月面 | 看石壁以外天空 | 星空和星云保留 | 背景整块黑 |
| 转场 | 门/舱内切换连续录制 | 无单帧整张 atlas 黑闪 | 一帧黑贴图后恢复 |
| 第一章走廊 | 静止 5 秒、左右转、前后走 | 墙面稳定，远近雾自然 | 运镜后材质变黑 |
| 战斗血迹 | 近/中各 10 次命中 | 遮挡正确、约半尺寸、正常消退 | 血盖脸/枪/墙；烟雾被缩小 |
| 字幕/菜单 | 中文和英文各切换一次 | 字完整、比例正常、可读 | 缺字、发黑、超大/不可见 |
| 拾取/工作台 | 打开一件拾取说明和工作台 | FBO viewport 正确、整窗显示 | 只显示左下局部 |
| 结局换头 | 使用备份存档触发对应段落 | 脸部完整，无头盔灯带残留 | 灯带盖脸 |
| 后台恢复 | HOME 30 秒后返回 | 同 PID/session，无黑贴图/状态泄漏 | 重启 IPA、黑帧、材质异常 |
| 输出画质 | Neutral 与 Production 各一次 | Production 不引入新画面缺陷 | 仅开 FSR/CAS 后出现条纹 |

### 9.4 重复次数

- 确定性静态缺陷：同一机位前后截图各 1 张即可作为证据，但必须能 A/B 复现。
- 运镜缺陷：连续录 20 秒以上，包含慢转和快速转。
- 单帧黑闪/竞态：至少 3 次冷启动、3 次后台恢复，每次观察到转场完成。
- 血迹：近、中各 10 次，至少 5 次有墙或角色遮挡关系。
- 扫描线：整个开场录一次，不能只看标题页或单个静止截图。

任意一次可复现即判该项失败；“多数时候正常”不能算通过。

## 10. 证据包格式

建议每个候选包建立：

```text
evidence/android-visual-<date>-<candidate>/
  result.json
  device.txt
  package-sha256.txt
  settings.txt
  logcat.txt
  screenshots/
    opening-hull-before.png
    opening-hull-after.png
    poster-before.png
    poster-after.png
    blood-depth-before.png
    blood-depth-after.png
  video/
    opening-full.mp4
  saves/
    save-inventory.txt
```

`result.json` 至少包含：

```json
{
  "candidate": "android-visual-date-candidate",
  "apk_sha256": "...",
  "ipa_sha256": "...",
  "backend": "gles1_native",
  "device": "...",
  "gpu": "...",
  "gles": "...",
  "resource": "iPad2",
  "screen_scale": 2,
  "fsr": "off",
  "sharpen": "off",
  "aa": false,
  "checks": [
    {
      "name": "opening_scanline",
      "status": "fail",
      "evidence": "video/opening-full.mp4",
      "note": "hull key/count ..."
    }
  ]
}
```

结论只能使用 `pass`、`fail`、`not_tested`。没有同机位证据的项目必须写
`not_tested`，不能从别的场景推断通过。

## 11. 禁止事项与已证伪方向

不要重新启用以下方案作为 Android 默认：

- 全局关闭雾；
- 全场景不写深度；
- 全场景不剔除；
- 每帧清深度；
- destination fill；
- `AUTO_MIPS_ALL=1` 作为正式默认；
- 把全部 atlas 改成 `GL_LINEAR`；
- 用普通逐通道 PVRTC mip 修复解释所有船体扫描线；
- 用 `TEXBLACK` 黑掉大型船体；
- 用纯黑遮住海报或 HUD 让缺陷“看不见”；
- 把 FSR/CAS 当作内部纹理修复；
- 未确认 key/count 就套用 `SKIP_TRI_HASH_COUNT`；
- 直接用最近一次构建成功替代视觉验收；
- 覆盖用户存档或正式 `run-host/Runtime-PC`。

已确认的易误诊：

- Android 开局“超大黑团”首先怀疑有限远平面投影，不是黑贴图。
- PC 过场局部黑场首先用红雾对照确认雾，不是深度或纹理。
- 玩家和 HUD 仍亮、环境变黑时，两者可能同时存在；要区分“旁路材质”和雾。
- 单个暗像素不能证明黑屏根因，必须与同机位截图和 draw 日志交叉验证。
- 游戏复用 GL texture name，numeric id 不是内容身份。
- `TEX_UPLOAD_FINISH` 是 presence 开关，设置 `"0"` 仍然启用。
- 编译成功、能进游戏、没崩溃，都不等于视觉修复通过。

## 12. 完成定义

一个 Android 画面移植项只有同时满足以下条件才算完成：

1. 能指出实际执行代码位置，并证明不是只在 `gles1_on_gl2` 中；
2. 代码按 content key、count、投影和 GL 状态做最小范围匹配；
3. 所有临时 GL 状态在 draw 后恢复；
4. 有独立单元测试或可重复的 A/B 开关；
5. 在当前 APK、实际 Android 后端和至少两台设备上复验；
6. 同一存档、同一机位、同一资源档和同一输出设置下有前后证据；
7. 日志确认命中，画面确认症状消失且没有新缺块；
8. 已更新本矩阵中对应项的最终状态和证据路径。

如果只完成了第 1 到第 4 项，应标为“代码已适配、待设备验收”，不能标为“修复
完成”。

## 13. 相关细节文档

- [Android 已适配基线与旧设备证据](./ANDROID-PC-VISUAL-INHERITANCE.md)
- [PC 给 Android 的原始画面修复说明](./PC-VISUAL-FIXES-FOR-ANDROID.md)
- [PC 开局极小正值雾修复](./PC-OPENING-FOG-FIX.md)
- [PC 命中血迹深度与尺寸修复](./PC-BLOOD-DEPTH-FIX.md)
- [旧帧 2016 的纹理、深度、剔除实验记录](./HANDOFF-2026-10-24-FRAME2016.md)
- [工作日志与当时的验收记录](./WORKLOG-2026-10-25-FRAME2016.md)
- [原生代码入口、FBO、拾取窗和输入反编译记录](./DECOMPILE-reference.md)
- [PC 当前接管状态与严格验收边界](./PC-HANDOFF-NOW.md)

仓库根目录的 `DISTANCE-RENDER-2026-10-22.md` 和
`VIDEO-AUDIT-20261023.md` 保留了开场船体扫描线仍未闭环的证据，扫描线任务必须先
阅读这两份记录。
