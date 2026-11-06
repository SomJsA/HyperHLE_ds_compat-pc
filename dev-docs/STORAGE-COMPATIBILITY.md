> PC 2026-11-04修订优先：见[PC-DISTRIBUTION-LAYOUT.md](PC-DISTRIBUTION-LAYOUT.md)。首次预置后读取配置，无启动短测；文档/DeadSpaceLegacy保存配置、进度与备份，程序/diagnostics保存新诊断，Game收纳原IPA与运行时。下文旧PC目录/短测条款仅作历史。

# 配置与存档兼容性契约 v1

新 PC 分发包没有根目录活动 INI，首次启动默认采用当前用户的系统“文档”目录。
稳定叶目录沿用 `DeadSpace IOS移植`，不能为改名而另建目录、丢失既有进度。

| 路径 | 用途 |
| --- | --- |
| `storage-layout.json` | schema=1、游戏身份与路径契约；未知或损坏版本拒绝写入 |
| `config/pc` | 启动器、按键、手柄、覆盖样式及有效画面配置 |
| `touchHLE_sandbox/com.ea.deadspace.bv` | 原生 Library/Preferences 与 Documents，保持游戏原有格式 |
| `diagnostics` | 本次启动自检和用户请求的短测结果；不是游戏进度 |

分发包 `Defaults` 只提供首次缺失配置的模板。已有文件不覆盖，硬件识别不重置画质。
既有启动器根目录 `DeadSpace-Launcher.ini` 视为显式旧配置，仍按其存储选择工作。
替换发布目录不会替换文档目录；选择便携模式也不会自动搬运或合并进度。
主 INI 修改采用临时文件和原文件备份。原生文件的导入和编辑继续使用既有整组三槽备份/恢复边界。

PC 与 Android 使用三槽 ZIP v1 交换原生存档及公共依赖，包括合法零字节依赖；
不是直接复制整个启动器配置。Android 的公共 Documents 需系统授权目录，不能把 SAF URI 当作 POSIX 路径。
两端平台画质/输入配置各自保存；Android 公共目录桥接实现及 APK 验证在后续 Android 发布中单列。
游戏身份始终 `com.ea.deadspace.bv`，与 Android 宿主应用 ID（含 somjs）独立。
不自动转换未知 SBIN 字段、改变槽位语义或并行合并进度。

## 回归入口

编译启动器后，用 .NET Framework csc 编译 `tools/tests/StartupStorageTest.cs`。
运行 `StartupStorageTest.exe <启动器绝对路径> <全新夹具目录>`。
验证新包默认、稳定旧叶目录、显式旧配置、缺失才复制、原生进度不变、INI 原文件备份、未知/损坏布局拒绝。
三槽 ZIP/编辑器负例另由 `tools/project-guard/run_save_tests.py` 执行。

启动自检只展示当前 PID 实际报告；待确认和回退分别展示影响。
硬件建议限定有证据的确切配置，用户明确应用后才变更设置。未知配置请求短测，默认不运行。
短测采用原生 present 间隔及可操作 HUD/状态，不读 framebuffer，不注入输入。
有效游戏长帧保留；加载、菜单、暂停、过场按状态排除。30 秒采样不能证明全关稳定 60。

## Android接入进度与短测状态边界

Android现在有统一RuntimeStorage、应用专属Documents默认根与版本化配置接入，说明见Android树dev-docs/ANDROID-STARTUP-STORAGE.md。旧根保留；公共Documents的SAF桥接与具体APK/设备负例继续单列，不能用新类或编译通过宣布完成。宿主org.touchhle.android.somjs与原生游戏ID独立。

双端短测不再以0/7/9/10的窄白名单遗漏已观察到的state3开火帧；可用世界/HUD及当前玩家身份相符是先决条件，保留原生游戏长帧，不按耗时筛除。排除站点模态、死亡26、过场29和未识别状态；实际各动作/跨关仍须验证，不能以纯策略测试冒充实战60或时钟验收。
