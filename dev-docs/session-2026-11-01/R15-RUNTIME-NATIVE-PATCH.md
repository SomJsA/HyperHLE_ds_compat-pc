# R15 原版 IPA 与运行时原生中文补丁

2026-11-01，Asia/Hong_Kong。模型见 [PATCH-MODEL](../../tools/translation/PATCH-MODEL.md)。

PC 与 Android 在 VFS 初始化时把 `res/overlay/deadspace_iphone.bin` 挂到原有
`published/stringdata/CHS_CN/deadspace_iphone.bin` 节点。补丁由 EXE／原生库携带，
启动器继续选择原运行基底 IPA，Android APK 中的 IPA 也保持基底原始字节。
本轮未生成派生 IPA，未访问平板 ADB，未替换已验收发布件或活动存档。

只对 `com.ea.deadspace.bv` 的已知中文资源修订启用。未知修订回退原节点；
`TOUCHHLE_DS_NATIVE_TEXT_PATCH=0` 可在启动前关闭原生补丁。其他语言、文件、
bundle 标识和存档路径不变。只读保护、VFS size、descriptor length/fstat、seek、
EOF 和复制句柄均使用同一份补丁字节，哈希仅启动时计算。

真正原始 1.3.34 备份和原运行 iPad2 基底中的中文 SBIN 为
`47864a0c746cf439b9bab93e66aea621ae3896f2e517f710436b6302b0a3db77`。
旧工具标为 original 的 `4732db25...d36f0` 是历史翻译基底，仍保留兼容。
真实原版与新版均有 996 个键，845 项不同；新版 SBIN 为
`71f989b9cea4ae2dedb58b4e66279513ac06f077eb30e65ef7bfcdcfeada5bce`。

本轮 release 验证：167 项 DS 测试通过，1 项外部 IPA 测试默认忽略；随后分别使用
原始备份 IPA 与原运行 iPad2 基底执行该外部测试，两次通过且整体 IPA 哈希不变。
3 项 data-root、2 项 frame-pacing 通过。PC 性能 harness 68 项：67 通过、1 跳过。
PC release EXE、启动器及 Android arm64 release 均构建成功。
APK 签名 v1/v2 校验通过；仍沿用项目现有 debug release 签名。

后台 PC 已观察到：补丁关闭时难度“正常”，开启后为台账“普通”；启动日志报告
实际源哈希及新版补丁哈希；第一章为“当初善意 / Honorable Intentions”；移动教程
经过真实前进输入后转入视角教程；原生 locator 显示新版任务“找到并摧毁3个排水保护装置的电源箱”。
截图来自进程内 present capture，控制来自专用 inbox，已检查测试窗口不可见且非前台。
首次任务截图处于入场背景阶段，不算文字验收；补充截图确认文字及两行排版。

主机显式关闭并确认 VSync 的主矩阵完成 29 项；扩展呈现与文字矩阵完成 16 项，
均通过对应有效性检查。CPU/GPU 文字三组交替样本及六张截图确认实际绘制、完整
两行目标和画面更新。报告保留重复样本波动、CPU 调用耗时和滚动分位数的解释，
不据此改变生产默认；静止场景不等于加载、跨关或其他驱动的正确性验收。

用户明确允许的 VMware 功能检查已完成。相同候选 EXE 和未改动的 IPA 在显式
GLES1-on-GL2、llvmpipe 软件渲染器下显示新版难度、首章、移动/视角教程及目标。
客体 C 盘无剩余空间，测试使用只读运行资源共享和独立可写数据共享，未删除旧
客体文件。测试后移除两项临时共享并将本次启动的 VM 恢复为关闭；另一台原已
运行的项目 VM 未动。详细限制及清理日志记录于报告目录的 VM-VALIDATION.md。

证据与新候选独立保存：

- `E:/CodexProjects/DeadSpaceLegacy/report/runtime-native-patch-20261101/`
- `E:/CodexProjects/DeadSpaceLegacy/builds/pc-user-acceptance-20261101-r15-runtime-patch/`
- `E:/CodexProjects/DeadSpaceLegacy/builds/android-user-acceptance-20261101-r15-runtime-patch/`

本项通过不等于完整 R15 最终验收。完整质量/性能矩阵、加载与跨关、第二 GPU/驱动、
实体输入和动作/音效、Android 实机及双端用户验收继续保留。Finish、GPU copy off、
GPU text off 的生产默认不因本次挂载改动而改变。平板 ADB 须等待用户明确释放。
