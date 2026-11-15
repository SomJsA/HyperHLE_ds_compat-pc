# DEAD SPACE MOBILE PC — R4.9 预发布 / Prerelease

本版本用于继续进行多 IPA 全适配与多语言高清文字组件验收。安装器和启动器会按 IPA 内部可执行文件与 `Info.plist` 的 SHA-256 指纹识别版本；文件名不会作为版本依据。

This prerelease continues multi-IPA compatibility and multilingual HD text validation. The installer and launcher identify an IPA by the SHA-256 fingerprints of its uncompressed executable and `Info.plist`; the filename is never used as the version identifier.

## 使用建议 / Recommendation

在全适配完成前，优先使用 **1.3.34**。其他版本可以尝试对应的兼容 profile，但不能把下面的“已识别”理解为“第12章全操作已验收”。

Until full adaptation is complete, use **1.3.34** whenever possible. Other editions can try their identified compatibility profile, but “recognized” below does not mean that all chapter-12 gameplay actions have been accepted.

## IPA 适配表 / IPA compatibility matrix

| IPA / build | 中文状态 | English status |
| --- | --- | --- |
| **1.3.34** (`Dead_Space__1.3.34_ios_4.3.ipa`) | **已深度验证**：第12章可进入；移动、E 交互、F 念力箱、瞄准、开火、静滞均走原生路径；无崩溃或未定义指令。 | **Deeply verified**: chapter 12 is reachable; movement, E interaction, F kinesis box, aim, fire and stasis use native paths; no crash or undefined instruction was observed. |
| **iPad World 1.3.11** | 已识别独立 ARMv7 profile；主菜单、章节选择、高清文字框架通过。但进入第12章后 `HUD+4 world=0`，尚未生成玩家对象，局内操作无法验收。 | Dedicated ARMv7 profile recognized; main menu, chapter selection and HD text framework pass. After selecting chapter 12, `HUD+4 world=0` remains and no player object is created, so in-game controls are not accepted. |
| **iPad 1.0.3 / build 1.3.11** (`DeadSpace_1.0.3_ios_3.2.ipa`) | 与上一个使用同一 iPad 旧版 profile；菜单和选择流程通过，同样卡在章节加载 / 存档入口。 | Shares the legacy iPad profile; menu and selection flow pass, but it remains blocked at the chapter-loading / save entry boundary. |
| **iPad 1.0.3 iOS 3.2** (`Dead_Space__for_iPad_1.0.3_ios_3.2.ipa`) | 首帧、菜单、8×分辨率和资源识别通过；属于同一旧 iPad ABI，但尚未单独完成第12章深度操作认证。 | First frame, menu, 8x resolution and resource detection pass. It uses the same legacy iPad ABI, but chapter-12 deep input has not been separately accepted. |
| **iPhone 1.0.3 / build 1.3.8** | ARMv6 profile、启动、菜单、E 输入队列和叠加层路径已接通；没有可认证的第12章存档，因此瞄准、开火、念力等尚未完成实机级验收。 | ARMv6 profile, launch, menu, E input queue and overlay path are connected. No certifiable chapter-12 save fixture is available, so aim, fire and kinesis have not received device-level acceptance. |
| **World 1.0.1** | 能到首帧，但旧二进制记录约 707,185 次 `UndefinedInstruction`，目前判定为不稳定旧构建，不能称为完全兼容。 | Reaches the first frame, but the old binary records about 707,185 `UndefinedInstruction` events. It is currently classified as an unstable legacy build and is not fully compatible. |

## 识别、警告与开关 / Detection, warnings and switches

- 1.3.34 is recognized as the most fully adapted edition. / 1.3.34 会显示为当前最完整适配版本。
- Recognized legacy editions receive a bilingual verification warning before launch. / 已识别旧版本启动前会显示双语验收警告。
- An unknown but readable IPA receives an explicit bilingual experimental warning. HD text and compatibility input are offered by default and can be turned off independently. / 未识别但具备标准资源布局的 IPA 会显示双语实验性警告；高清文字和兼容操作默认尝试，之后可以分别关闭。
- HD text, host fonts and language-bank support are bundled with the PC program and do not require a marker. External texture tiers remain edition-specific. / 高清文字、宿主字体和语言库随 PC 程序提供，不依赖 marker；外置纹理档位仍按 IPA 版本匹配。
- The installer keeps the language list hidden; choose the HD language at first launch. / 安装阶段隐藏语言列表，首次启动时选择高清语言。
- Legacy progress stays isolated by IPA profile; save editing and cross-version transfer remain disabled for unverified legacy editions. / 旧版进度按 IPA profile 隔离；未验收旧版本暂不开放存档编辑和跨版本互导。

## 已完成检查 / Checks completed

- Rust release build for the native runtime.
- Profile-aware native input regression for the current 1.3.34 build.
- Launcher and installer bilingual UI paths, fingerprint inspection, warning path and HD text default behavior.
- No derived IPA is produced; the original user IPAs remain untouched.

This is a **pre-release**. Please report the IPA version, executable SHA-256, `Info.plist` SHA-256, device/host, selected language and the first failing action in [DeadSpaceMobile Community issues](https://github.com/SomJsA/DeadSpaceMobile-Community/issues).

这是**预发布版本**。反馈时请附 IPA 版本、可执行文件 SHA-256、`Info.plist` SHA-256、设备 / 主机、选择的语言以及第一个失败的操作，并提交到 [DeadSpaceMobile Community issues](https://github.com/SomJsA/DeadSpaceMobile-Community/issues)。
