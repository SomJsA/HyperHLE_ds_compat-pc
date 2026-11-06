# 安卓源码备份与纹理/画面修复提取

> 用户澄清：需要的是告知安卓侧有哪些 PC 内部纹理修复可继承，不是备份安卓。
> 正式接入入口改为 [PC-VISUAL-FIXES-FOR-ANDROID.md](PC-VISUAL-FIXES-FOR-ANDROID.md)。
> 本文仅保留已经生成的额外归档记录，不要求安卓重新导入源码包。

2026-10-25：用户确认“测试灯带问题已消失”“EF也修好了”。两项均已验收；局部转场黑场和可靠即时读档仍未解决。
完整接手已提交 `3c344a7f`，后续安卓集成、启动器与构建记录也已进入 `codex/android-mainline-merge`。
本次备份面向该最新集成主线，未修改正在使用的安卓探索 worktree。

## 已完成交付

冻结源提交：`ef5ed43951279c6225e50094ef64c4da37589424`。
共 30 个归档/提取文件，清单见 [android-visual-export-manifest.json](android-visual-export-manifest.json)。
全部 ZIP CRC、Git bundle 完整性、渲染净差异在 `358c5731` 基线上的 `git apply --cached --check` 均通过。
没有执行安卓设备画面复测，也没有把此次备份推送到远端。
遗留 SDL 修改已单独备份；构建目录保持原样，工作树仍可能报告这些既有项目。

## 交付物

外层目录：`E:\CodexProjects\DeadSpaceLegacy\backups\android-visual-handoff-20261025`。

- `source.zip`：manifest 指定提交的全部受 Git 管理源码，顶层为 source；不含构建产物、原始游戏资源、存档或 `.git`。
- `android-mainline.bundle`：该分支完整可达 Git 历史，可离线 clone 后检查提交。
- `vendor-*.zip`：每个已初始化子模块的固定提交源码，按 manifest 放回相应 vendor 路径。
- `vendor-SDL-working.patch`：尚未提交的既有 Android 16 MiB 线程栈修改，单独保留；主源码 ZIP 不含该工作树修改。
- `manifest.json`：精确源提交、子模块版本、提取清单、尺寸、SHA256 和验证结果。
- `visual-fixes/`：画面代码净差异、逐提交历史和当前源码参照。

## 提取边界与接入

当前集成主线已经包含这些修复，**不要对当前 HEAD 再应用补丁**。旧安卓分支应先比较共同祖先并审查差异；不能直接覆盖整份 gles_guest.rs，它也包含输入逻辑。

`renderer-net-from-358c5731.patch` 是 `358c5731` 至导出提交的 `src/gles/gles1_on_gl2.rs` 净差异。
它保留最终修复和诊断代码，也保留回滚决定，适合查看最终渲染器变化；不是剥除所有诊断开关的最小补丁。
`history/` 按提交提取相关渲染文件，供定位原因，不应与净差异重复应用。部分历史提交包含后来已回滚的实验。
`carrie-head.patch` 是独立的换头灯带修复，原始提交为 `2ac477c4`。
`reference-source/` 是导出时完整文件参考，非全部都是可直接移植的纯纹理代码。

| 画面问题 | 代码入口/提交 | 结论 |
| --- | --- | --- |
| atlas wrap、空间背景深度分层 | gles1_on_gl2.rs；5adc70fa 及后续回滚 | 使用净差异中的最终状态，不恢复失败实验 |
| 距离雾、mipmap/过滤 | gles1_on_gl2.rs、gles_guest.rs；9b8d476a | 用户确认雾中渐显正常 |
| 舷窗 fog、海报过滤 | gles1_on_gl2.rs；261fe7a4 | 专项作用域修复 |
| iPad2 资源 key | gles_guest.rs；38ec3ce6 | 安卓设备分档/资源路径需核对 key 命中 |
| 开场异常三角形、透明覆盖物、地基 | b3fad9db、173574de、87a23def、bb420b34 | 查看最终条件及已撤销实验，避免全局禁用深度/剔除 |
| 岩石缺块/错误前景 | d4c65100、75587095；gles1_on_gl2.rs | d1feb687:600 保持正常深度写入和原始剔除 |
| 临时雾状态泄漏 | 75587095；gles1_on_gl2.rs | indexed float-array draw 也必须恢复临时 fog override |
| 脸部残留灯带 | environment.rs、ds_carrie_patch.rs；2ac477c4 | 用户实测消失；按符号和指令匹配后复用原生完整隐藏路径 |

启动器的环境变量也是效果的一部分。`tools/pc-runtime/Program.cs` 仅作配置参照：安卓启动端需检查是否仍强制开启旧背景深度、不剔除、depth reset 或 destination fill 实验。
不要整体移植 Windows 启动器。纹理哈希识别参照 `src/ds_texid.rs`，其源码随参考文件提供。

E/F 圆形交互、无重力输入属于输入修复，不纳入渲染补丁；它们仍完整保存在 source.zip/bundle。
Android FSR3、AHB/Vulkan bridge、D16、FSR present 等属于另一条探索线，不将其作为本轮纹理修复提取。
PC 验收不等于所有 Android GPU、设备分档都验收通过；此交付是源码备份与提取，不是新 APK 实测报告。

## 可重复导出

在仓库运行 `python tools/export_android_visual_handoff.py --output <新的空目录路径>`。
工具冻结调用时 HEAD、归档源码与子模块、备份 SDL 工作树差异，并验证 ZIP CRC、Git bundle、净补丁在准确基线上的可应用性。
不 checkout、不 merge、不改子模块和当前游戏。输出目录必须尚不存在，以保留旧备份。
导出后如又提交交付清单，manifest 中的源提交仍是正确冻结版本；后续清单提交不代表源码包过期。
