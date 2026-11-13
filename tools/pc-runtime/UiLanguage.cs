using System;
using System.Collections.Generic;
using System.Globalization;
using System.Text;
using System.Windows.Forms;
using System.Runtime.CompilerServices;

namespace DeadSpaceUi
{
    internal static class UiLanguage
    {
        private static readonly Dictionary<string, string> ZhToEn = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            {"00 关于", "00 About"}, {"已启动", "Running"},
            {"高级光影（默认开启）", "Advanced lighting (on by default)"},
            {"高清中文文字组件（章节 / 任务 / 字幕）", "HD Chinese text (chapters / objectives / subtitles)"},
            {"快行动，破坏者！\r\n让我们把泰坦空间站烧成灰！", "Wake the fuck up, Vandal !\r\nWe Have a Titan Station to Burn !"},
            {"死亡空间移动版（IOS端高清移植） · 启动器", "DEAD SPACE MOBILE · Launcher"},
            {"死亡空间移动版（IOS端高清移植） · 安装与更新", "DEAD SPACE MOBILE · Install & Update"},
            {"iOS 移植 / PC", "iOS PORT / PC"}, {"IOS 移植  /  PC", "iOS PORT / PC"},
            {"安装程序与游戏资源", "Installer and Game Resources"},
            {"程序位置", "Program Location"}, {"游戏资源位置", "Game Data Location"},
            {"资源来源 · marker / IPA", "Resource Source · marker / IPA"},
            {"高清纹理 · 可多选", "HD Textures · Multi-select"}, {"快捷方式与启动", "Shortcuts and Launch"},
            {"桌面快捷方式", "Desktop Shortcut"}, {"开始菜单", "Start Menu"}, {"完成后启动", "Launch when complete"},
            {"开始安装", "Install"}, {"更新 / 更改安装", "Update / Change"}, {"退出", "Exit"}, {"关闭", "Close"}, {"重试安装", "Retry"},
            {"浏览…", "Browse..."}, {"全选", "Select all"}, {"推荐", "Recommended"},
            {"画质与显示", "Graphics Display"}, {"鼠标与操作", "Mouse & Controls"}, {"键盘绑定", "Keyboard Bindings"},
            {"手柄与摇杆", "Gamepad & Stick"}, {"游戏与存档位置", "Game & Save Locations"}, {"存档编辑器", "Save Editor"},
            {"存档插件", "Save Plugins"}, {"兼容与高级", "Compatibility & Advanced"}, {"调试", "Debug"}, {"内部渲染", "Internal Render"},
            {"01 画质与显示", "01 Graphics"}, {"02 鼠标与操作", "02 Mouse"},
            {"03 键盘绑定", "03 Keyboard"}, {"04 手柄与摇杆", "04 Gamepad"},
            {"05 游戏与存档位置", "05 Game & Saves"}, {"06 调试", "06 Debug"},
            {"输出分辨率", "Output Resolution"}, {"窗口模式", "Window Mode"}, {"帧率上限", "FPS Limit"}, {"超分算法", "Upscaler"},
            {"鼠标捕获", "Mouse Capture"}, {"输入设备", "Input Device"}, {"兼容与诊断", "Compatibility & Diagnostics"},
            {"纹理上传同步", "Texture Upload Sync"}, {"长句字幕", "Long Subtitle"}, {"存档位置", "Save Location"},
            {"恢复默认", "Restore Defaults"}, {"撤销", "Undo"}, {"应用", "Apply"}, {"启动游戏 →", "Launch Game →"}, {"启动游戏  →", "Launch Game →"}, {"停止游戏", "Stop Game"},
            {"选择 IPA 安装源", "Choose IPA Source"}, {"选择游戏资源", "Choose Game Resource"}, {"选择 Dead Space IPA", "Choose Dead Space IPA"},
            {"启动自检", "Startup Review"}, {"进入游戏", "Enter Game"}, {"已安装游戏数据", "Installed Game Data"},
            {"读取当前存档", "Read Current Save"}, {"保存插件开关", "Save Plugin Switches"}, {"启用高清纹理包", "Enable HD Texture Pack"},
            {"详细调试日志", "Verbose Debug Log"}, {"原生切枪、定位与暂停界面", "Native weapon, locator, and pause UI"},
            {"原生视角与移动", "Native PC camera and movement"},
            {"自动诊断（默认关闭；F7 单次保存仍可用）", "Automatic diagnostics (off by default; manual F7 remains available)"},
            {"自动配置与启动状态", "Startup Review"}, {"English", "中文"}, {"中文", "English"},
            {"窗口", "Window"}, {"无边框全屏", "Borderless Fullscreen"}, {"独占全屏", "Exclusive Fullscreen"},
            {"不限", "Unlimited"}, {"跟随内部", "Follow Internal"}, {"自动（键鼠 + 手柄）", "Auto (Keyboard + Gamepad)"},
            {"仅键鼠", "Keyboard + Mouse only"}, {"仅手柄", "Gamepad only"}, {"按住空格为副模式", "Hold Space for alternate mode"},
            {"按空格切换主／副模式", "Press Space to toggle primary/alternate"}, {"Finish（保守，默认）", "Finish (Conservative, Default)"},
            {"Flush（A/B）", "Flush (A/B)"}, {"关闭（诊断）", "Off (Diagnostics)"}, {"整段三行", "Three-line block"},
            {"大字分段", "Large segmented text"}, {"便携（启动器目录）", "Portable (Launcher Folder)"}, {"文档（当前用户）", "Documents (Current User)"},
            {"读取", "Read"}, {"预览更改", "Preview Changes"}, {"备份并保存", "Backup and Save"},
            {"导出三槽位 ZIP", "Export Three-Slot ZIP"}, {"导入 ZIP 并恢复三槽位", "Import ZIP and Restore Slots"},
            {"选择安装位置和资源后，点击开始安装。", "Choose locations and resources, then click Install."}
            ,{"分辨率按屏幕独立识别；画质档位调整采样和过滤。", "Resolution is detected independently; quality presets adjust sampling and filtering."}
            ,{"多重采样抗锯齿", "MSAA"}, {"各向异性过滤", "Anisotropic Filtering"}
            ,{"追加 CAS 锐化（可独立于 FSR1 使用）", "Add CAS sharpening (independent of FSR1)"}
            ,{"快速呈现", "Fast Present"}, {"3D加速模式（GPU copy，默认开启；关闭为兼容模式 CPU copy）", "3D acceleration (GPU copy, on by default; off uses compatible CPU copy)"}
            ,{"护眼模式（实验性）：将接近黑色的区域替换为暗灰，减少整屏黑闪", "Eye Protection Mode (Experimental): replace near-black pixels with dark gray to reduce full-screen black flashes"}
            ,{"原游戏含大量高闪烁场景，这是营造氛围的原生行为。游玩时如有不适，请停止游玩并及时休息。\r\n护眼模式默认关闭。开启后会改变黑色、暗部和原有氛围，影响观感；灰度过高可能使黑场发白。\r\n可拖动滑块调整灰度；此开关不会消除所有闪烁。", "Frequent intense flashing is part of the original game's atmosphere. If you feel unwell, stop playing and take a break.\r\nEye Protection Mode is off by default. It changes blacks, shadows and atmosphere, affecting the intended look; high values may wash out dark scenes.\r\nAdjust the gray level with the slider. This option does not remove all flashing."}
            ,{"黑色替换灰度（默认 10 / 255）", "Black replacement gray (default 10 / 255)"}
            ,{"高闪烁场景提醒", "Flashing Scenes Notice"}
            ,{"本游戏含高强度闪烁场景，这是原游戏为营造氛围设计的原生行为。\r\n\r\n游玩时如有不适，请立即停止游玩并及时休息。\r\n\r\n实验性护眼模式会改变黑色、暗部和原有氛围，影响观感；默认关闭，可在“画质与显示”中调整。此开关不会消除所有闪烁。", "This game contains intense flashing scenes. This is part of the original game's visual design and atmosphere.\r\n\r\nIf you feel unwell while playing, stop immediately and take a break.\r\n\r\nExperimental Eye Protection Mode changes blacks, shadows and atmosphere, affecting the intended look. It is off by default and can be adjusted in Graphics Display. It does not remove all flashing."}
            ,{"返回启动器", "Return to Launcher"}
            ,{"我已了解，开始游戏", "Understood, Start Game"}
            ,{"屏幕匹配：1080P用4×、1440P用6×、4K用8×；内部尺寸独立于画质。\r\nMSAA 受驱动上限约束；本管线尚无 TAA，不能承诺消除所有运动闪烁。", "Screen matching: 4x at 1080p, 6x at 1440p, 8x at 4K; internal size is independent of quality.\r\nMSAA is limited by the driver; this pipeline has no TAA."}
            ,{"作者与鸣谢", "Authors & Credits"}
            ,{"项目与第三方组件的来源、作者和版权说明。", "Project, third-party component, author, and copyright information."}
            ,{"项目：死亡空间移动版（IOS端高清移植）\r\nSomJs：项目主要开发、手柄游玩测试、骁龙 Android 设备游玩测试\r\nlxyun_2（腹黑小喵）：超级校对、超级测试\r\nMisakaCharlotte（是的呀啊）：超级校对、超级测试\r\n特洛克·洛克莱斯：超级检查测试\r\n天雷本雷：键鼠游玩测试、天玑 Android 设备游玩测试\r\n豆沙包里有磁：键鼠测试（顶着极低的全转储帧率）\r\n基础项目：HyperHLE_ds_compat（非官方分支）；上游：HyperHLE、touchHLE\r\n运行库致谢：dynarmic、SDL、rust-sdl2、stb_image、openal-soft 等\r\n字体与输入资源：Noto CJK、Liberation Fonts、Kenney input prompts\r\n版权说明：Dead Space 与相关游戏素材归 Electronic Arts 所有；本项目不包含游戏数据\r\n许可：源码与第三方组件按各自许可证提供，详见 LICENSE、NOTICE 与 Docs\r\n原版 credits_list 制作名单继续保留；本名单只记录本项目贡献。", "Project: DEAD SPACE MOBILE (IOS HD port)\r\nSomJs: project development, gamepad playtesting, and Snapdragon Android playtesting\r\nlxyun_2 (腹黑小喵): super proofreading and super testing\r\nMisakaCharlotte (是的呀啊): super proofreading and super testing\r\n特洛克·洛克莱斯: super inspection testing\r\n天雷本雷: keyboard/mouse playtesting and Dimensity Android playtesting\r\n豆沙包里有磁: keyboard/mouse testing (against an extremely low full-dump frame rate)\r\nBase project: HyperHLE_ds_compat (unofficial fork); upstream: HyperHLE and touchHLE\r\nRuntime credits: dynarmic, SDL, rust-sdl2, stb_image, openal-soft, and others\r\nFonts and input assets: Noto CJK, Liberation Fonts, and Kenney input prompts\r\nCopyright: Dead Space and related game assets belong to Electronic Arts; no game data is included\r\nLicensing: source and third-party components follow their respective licenses; see LICENSE, NOTICE, and Docs\r\nThe original credits_list is preserved; this list records project contributions only."}
            ,{"屏幕匹配：3840 × 2560（底分 8×；屏幕高度 2160） 当前内部：3840 × 2560 · 输出：3840 × 2160 · 手动", "Screen match: 3840 x 2560 (base 8x; screen height 2160)  Current internal: 3840 x 2560 · Output: 3840 x 2160 · Manual"}
            ,{"鼠标捕获、瞄准与常用操作；手柄摇杆在04页单独调整。", "Mouse capture, aiming and common actions; gamepad sticks are configured on page 04."}
            ,{"视角方向", "Look Direction"}, {"反转水平+垂直", "Invert Horizontal + Vertical"}, {"只反水平", "Invert Horizontal Only"}, {"只反垂直", "Invert Vertical Only"}, {"标准", "Standard"}
            ,{"鼠标默认", "Mouse Defaults"}, {"镜头灵敏度", "Look Sensitivity"}, {"%（100%为默认）", "% (100% default)"}
            ,{"右键瞄准，左键射击", "Right mouse aim, left mouse fire"}, {"右键：按住瞄准，松开收枪", "Right mouse: hold to aim, release to lower"}, {"右键：点击举枪，再点收枪", "Right mouse: click to raise, click again to lower"}
            ,{"举枪前移", "Aim Walk"}, {"举枪禁用前向疾跑", "Disable forward sprint while aiming"}, {"% 步行上限", "% walk limit"}
            ,{"移动摇杆增益", "Movement Stick Gain"}, {"空格副模式", "Space Alternate Mode"}
            ,{"举枪步行上限共用于键鼠和手柄；100%为最快步行，小拉保持原幅度。\r\nWASD 移动 · E 交互 · Ctrl 无重力跳跃 · G / B 定位 · Z 快速转身\r\nF 念力 · R 装弹 · X 旋转 · F6 检查点 · F7 画面标记", "Aim walk limit applies to keyboard and gamepad; 100% is fastest walk.\r\nWASD move · E interact · Ctrl zero-G jump · G / B locator · Z quick turn\r\nF kinesis · R reload · X rotate · F6 checkpoint · F7 frame mark"}
            ,{"手柄设置", "Gamepad Settings"}, {"数字按钮可交换；下方有独立的摇杆镜头设置。", "Digital buttons can be swapped; stick camera settings are below."}
            ,{"十字键：↑等离子切割器 · →切割锯 · ↓线性枪 · ←岩石取芯器\r\n长按任一方向约 450 ms 选择大脉冲。LS = L3，RS = R3。\r\n同一按钮不能绑定两个动作；更改下次启动生效。", "D-pad: Up plasma cutter · Right ripper · Down line gun · Left core extractor\r\nHold any direction for about 450 ms to select the pulse rifle. LS = L3, RS = R3.\r\nOne button cannot bind two actions; changes apply on next launch."}
            ,{"摇杆镜头", "Stick Camera"}, {"独立于鼠标设置；更改下次启动生效。向下滚动可调灵敏度与死区。", "Independent of mouse settings; changes apply on next launch. Scroll down for sensitivity and deadzone."}
            ,{"反转横向", "Invert Horizontal"}, {"反转纵向", "Invert Vertical"}, {"右摇杆方向", "Right Stick Direction"}, {"右摇杆死区", "Right Stick Deadzone"}, {"%（默认14%，用于抑制漂移）", "% (14% default; suppresses drift)"}
            ,{"恢复手柄默认键位", "Reset Gamepad Buttons"}, {"恢复镜头默认", "Reset Camera Defaults"}
            ,{"交互 / QTE", "Interact / QTE"}, {"近战 / 踩踏", "Melee / Stomp"}, {"瞄准时主攻击", "Primary Fire While Aiming"}, {"光标模式", "Cursor Mode"}, {"暂停 / 恢复", "Pause / Resume"}, {"定位器（默认 R3）", "Locator (default R3)"}, {"快速转身（默认 L3）", "Quick Turn (default L3)"}, {"LT 瞄准方式", "LT Aim Mode"}, {"RB／RT 射击方式", "RB / RT Fire Mode"}
            ,{"按住瞄准，松开收枪", "Hold to aim, release to lower"}, {"按下举枪，再按收枪", "Press to raise, press again to lower"}, {"RB 主射击／RT 副射击", "RB primary / RT secondary"}, {"RB 切换主副模式，RT 射击", "RB switches mode, RT fires"}
            ,{"切换模式会先完成原生切换，再射击；RT 不会同时强制切副模式。\r\n键鼠空格设置独立，位于02页。", "Mode switching completes natively before firing; RT does not force secondary mode.\r\nKeyboard Space settings are independent on page 02."}
            ,{"游戏文件与存档", "Game Files & Saves"}, {"选择游戏文件和存档位置；迁移不会覆盖已有进度。", "Choose game files and save location; migration never overwrites existing progress."}
            ,{"安装位置", "Install Location"}, {"安装纹理", "Install Textures"}, {"更改 / 升级安装", "Change / Upgrade Install"}, {"选择 vandal.marker", "Choose vandal.marker"}, {"选择目录", "Choose Folder"}
            ,{"语言组件 / Language mode", "Language components / Language mode"}, {"高清中文组件（对话字幕 + 章节头）", "HD Chinese component (dialogue subtitles + chapter headers)"}, {"ⓘ 说明", "ⓘ Info"}
            ,{"关闭后回归原生语言行为：安装版本不启用高清中文对话字幕和章节头；重新运行安装器即可切换。其他语言将在后续开放，欢迎通过 GitHub issue 投稿。", "Disable this to restore native language behavior: the installed version will omit the HD Chinese dialogue and chapter layers. Re-run the installer to switch. More languages will follow; submit requests through GitHub Issues."}
            ,{"语言模式说明", "Language mode"}
            ,{"编辑所选存档位置的 settings.sb，写入前自动备份。", "Edit settings.sb in the selected save location; a backup is made before writing."}
            ,{"攻击增强（attack）", "Attack boost (attack)"}, {"防御增强（defense）", "Defense boost (defense)"}, {"资源增强（loot）", "Loot boost (loot)"}
            ,{"常规游玩保留雾效；实验选项只用于定位问题。", "Fog stays enabled for normal play; experimental options are for diagnosis only."}
            ,{"启用已验证的 PC 渲染兼容修复", "Enable verified PC rendering fixes"}, {"修正开场贴图的远距离采样", "Fix distant sampling for opening textures"}, {"实验：重建全部贴图的远距离采样", "Experimental: rebuild distant sampling for all textures"}
            ,{"诊断：关闭场景雾", "Diagnostic: disable scene fog"}, {"实验：背景层排除雾效", "Experimental: exclude background layer from fog"}, {"实验：慢呈现路径跳过同步等待", "Experimental: skip sync wait on slow present path"}, {"定期保存画面到 frames（影响性能）", "Periodically save frames (performance cost)"}
            ,{"关闭游戏窗口前确认退出", "Confirm before closing the game window"}, {"PC 原生视角与移动", "Native PC camera and movement"}, {"高清图文组件（章节 / 任务追踪 / 字幕；样式由外部工作台调整）", "HD text and graphics (chapters / objectives / subtitles; styled externally)"}
            ,{"性能监视器与记录（2Hz）", "Performance monitor and logging (2 Hz)"}, {"实验：GPU 章节 / 任务 / 字幕（取消勾选恢复 CPU）", "Experimental: GPU chapters / objectives / subtitles (uncheck for CPU)"}
            ,{"触摸回退瞄准点", "Touch fallback aim point"}, {"画质档位", "Quality Preset"}, {"高级画面设置", "Advanced Graphics"}, {"高级移动设置", "Advanced Movement"}, {"展开实验设置与诊断", "Expand Experimental & Diagnostics"}
            ,{"流畅", "Performance"}, {"均衡", "Balanced"}, {"清晰", "Clear"}, {"高画质", "High Quality"}, {"自定义", "Custom"}
            ,{"前进", "Forward"}, {"左移", "Strafe Left"}, {"后退", "Back"}, {"右移", "Strafe Right"}, {"念力", "Kinesis"}, {"静态能量", "Stasis"}, {"装弹", "Reload"}, {"近战", "Melee"}, {"旋转切割器", "Rotate Cutter"}, {"定位器", "Locator"}, {"快速转身", "Quick Turn"}, {"无重力跳跃", "Zero-G Jump"}
            ,{"1 等离子切割器", "1 Plasma Cutter"}, {"2 切割锯", "2 Ripper"}, {"3 线性枪", "3 Line Gun"}, {"4 岩石取芯器", "4 Core Extractor"}, {"5 大脉冲", "5 Pulse Rifle"}
            ,{"屏幕匹配（启动时识别）", "Screen Match (detect at launch)"}, {"1x 性能", "1x Performance"}, {"2x 平衡", "2x Balanced"}, {"3x 清晰", "3x Clear"}, {"4x 精细（1920×1280）", "4x Fine (1920x1280)"}, {"5x 手动（2400×1600）", "5x Manual (2400x1600)"}, {"6x 高画质（2880×1920）", "6x High Quality (2880x1920)"}, {"7x 手动（3360×2240）", "7x Manual (3360x2240)"}, {"8x 超采样（3840×2560）", "8x Supersampling (3840x2560)"}
            ,{"2× · 已安装", "2x · Installed"}, {"4× · 已安装", "4x · Installed"}, {"6× · 已安装", "6x · Installed"}
            ,{"2× · 未安装", "2x · Not installed"}, {"4× · 未安装", "4x · Not installed"}, {"6× · 未安装", "6x · Not installed"}
            ,{"已恢复默认；启动时保存。", "Defaults restored; changes will be saved when the game starts."}
            ,{"已撤销未应用的设置。", "Unapplied changes were undone."}
            ,{"设置已应用并保存；下次启动游戏时生效。", "Settings applied and saved; they take effect next time the game starts."}
            ,{"从另一模式复制存档到所选模式", "Copy saves from the other mode"}
            ,{"保存改键", "Save Key Bindings"}
            ,{"让我们把泰坦空间站烧成灰", "Let's burn the Sprawl to ash"}
            ,{"静滞", "Stasis"}
            ,{"＋ 高级画面设置", "+ Advanced Graphics"}
            ,{"－ 高级画面设置", "- Advanced Graphics"}, {"－ 高级移动设置", "- Advanced Movement"}
            ,{"＋ 展开实验设置与诊断", "+ Experimental & Diagnostics"}, {"－ 展开实验设置与诊断", "- Experimental & Diagnostics"}
            ,{"＋  高级画面设置", "+ Advanced Graphics"}, {"－  高级画面设置", "- Advanced Graphics"}
            ,{"＋  高级移动设置", "+ Advanced Movement"}, {"－  高级移动设置", "- Advanced Movement"}
            ,{"＋  展开实验设置与诊断", "+ Experimental & Diagnostics"}, {"－  展开实验设置与诊断", "- Experimental & Diagnostics"}
            ,{"始终捕获", "Always capture"}, {"仅瞄准时捕获", "Capture while aiming"}
            ,{"F8 切换点击/镜头", "F8 toggles click/look"}, {"Alt 按住临时点击", "Hold Alt for temporary click"}, {"Win 临时释放", "Win releases capture"}
            ,{"＋ 高级移动设置", "+ Advanced Movement"}
            ,{"键盘改键", "Keyboard Bindings"}
            ,{"无重力跳跃默认 Ctrl；定位默认 G / B；自定义后按所选键生效。", "Zero-G jump defaults to Ctrl; locator defaults to G / B; custom bindings apply after selection."}
            ,{"无重力跳跃可填 CTRL；其他动作使用字母或数字，一个键对应一个动作。\r\nF5 / F6 / F7 / F8、其余修饰键及 M / N / P / V 保留原功能。", "Zero-G jump accepts CTRL; other actions use one letter or number.\r\nF5 / F6 / F7 / F8, other modifiers, and M / N / P / V keep their original functions."}
            ,{"文件或位置已切换，请点击读取。", "File or location changed; click Read."}
            ,{"存档位置已切换，请重新读取插件设置。", "Save location changed; read the plugin settings again."}
            ,{"三个位分别编辑；设置与章节/成就记录由三个槽共享。保存前完整备份。", "The three slots are edited separately; settings and chapter/achievement records are shared. A full backup is made before saving."}
            ,{"0/1 为开关；物品类型须在本档中已有。背包格数量与场景状态保留。数值含义仍在核对，修改前请先导出三槽位 ZIP，错误值可能损坏存档。", "0/1 fields are switches; item types must already exist. Inventory slots and scene state are preserved. Numeric meanings are still being verified; export a three-slot ZIP first because bad values may damage a save."}
            ,{"资源与升级", "Resources & Upgrades"}, {"字段", "Field"}, {"当前值", "Current"}, {"修改为", "Change to"}
            ,{"共享设置 / 内购", "Shared Settings / Purchases"}, {"共享设置 / 内购插件", "Shared Settings / Purchase Plugins"}, {"内购三件套", "Purchase Plugins"}, {"内购插件", "Purchase Plugins"}, {"共享设置", "Shared Settings"}, {"进度（只读）", "Progress (Read-only)"}
            ,{"攻击增强", "Attack Module"}, {"防御增强", "Defense Module"}, {"资源增强", "Loot Module"}, {"节点增强", "Node Module"}, {"注册增强", "Registration Module"}
            ,{"返回所选存档位置", "Return to selected save location"}, {"恢复整组备份", "Restore backup set"}
            ,{"安装游戏资源", "Install Game Resources"}
            ,{"尚未安装游戏资源。选择 marker 或 IPA；也可把 marker 放在启动器同目录。", "Game resources are not installed. Choose a marker or IPA, or place marker beside the launcher."}
            ,{"自动诊断默认关闭；F7 单次保存仍可用。", "Automatic diagnostics are off by default; manual F7 saves remain available."}
            ,{"自动诊断：标记前30秒", "Automatic diagnostics: mark the previous 30 seconds"}
            ,{"导出诊断 ZIP…", "Export diagnostics ZIP..."}, {"查看本次诊断状态", "View current diagnostic status"}, {"打开诊断文件夹", "Open diagnostics folder"}
            ,{"采集档位", "Capture tier"}, {"基础：帧时间 / 原生状态", "Basic: frame time / native state"}
            ,{"增强：全程资源 / 渲染 / JIT 活动", "Enhanced: full resource / render / JIT activity"}, {"超级：全程活动 + 慢调用定位（开销较高）", "Super: full activity + slow-call attribution (higher cost)"}
            ,{"采集档位仅用于自动诊断；F7 单次保存不会启用持续采集。", "Capture tier is for automatic diagnostics only; manual F7 does not enable continuous capture."}
            ,{"自动诊断默认关闭；F7 可单次保存。导出记录前请先退出游戏。", "Automatic diagnostics are off by default; F7 can save one issue. Exit the game before exporting records."}
            ,{"F7 单次保存问题截图和信息；自动诊断关闭时，没有前30秒历史。\r\n开启自动诊断后记录前后时序；不录屏、不记逐键输入、不自动上传。", "F7 saves one issue frame and details; with automatic diagnostics off there is no previous 30-second history.\r\nWhen enabled, timing before and after the issue is recorded; no screen recording, keystroke logging, or automatic upload."}
            ,{"切换位置不会自动合并进度；空目录会新建进度。复制只接受空目标，保留源档并逐文件校验。\r\n复制方向：另一模式 → 当前所选模式。请先退出游戏；已有目标存档时拒绝覆盖。", "Changing location does not merge progress; an empty directory starts a new profile. Copying accepts only an empty destination and verifies each file while preserving the source.\r\nDirection: other mode → selected mode. Exit the game first; existing destination saves are never overwritten."}
            ,{"三个槽位，一个 ZIP：同时包含全部现有档案和公共设置，可与 Android 互导。\r\n导入前自动备份当前三槽位；请先退出游戏。", "Three slots, one ZIP: includes all existing saves and shared settings and can be exchanged with Android.\r\nThe current three slots are backed up before import; exit the game first."}
            ,{"每次只写当前文件；检查点和章节档分别保存。读取后可编辑资源、武器、已有背包格与真实内购字段。", "Only the current file is written; checkpoint and chapter saves are separate. After reading, you can edit resources, weapons, existing inventory slots, and real purchase fields."}
            ,{"0/1 为开关；物品类型须在本档中已有。背包格数量与场景状态保留。", "0/1 values are switches; item types must already exist in this save. Inventory slot count and scene state are preserved."}
            ,{"屏幕匹配：", "Screen match: "}, {"当前内部：", "Current internal: "}, {"输出：", "Output: "}, {"手动", "Manual"}
            ,{"全量 mip 显存估算：", "Full mip VRAM estimate: "}, {"另需 CPU 解码和驱动空间；估算不是实测峰值。", "CPU decode and driver space are additional; this is an estimate, not a measured peak."}
            ,{"暂无全量显存估算", "No full VRAM estimate"}, {"未启用：使用原版纹理。", "Disabled: use original textures."}
            ,{"内部与输出分辨率保持独立。", "Internal and output resolution stay independent."}
            ,{"06 存档编辑器", "06 Save Editor"}
            ,{"07 调试", "07 Debug"}
            ,{"专项快速编辑（上）", "Quick Editor (Top)"}
            ,{"先读取共享设置；下方自由字段表可直接修改金钱 / 信用点、能量节点和大脉冲。这里集中处理三个内购插件开关。", "Read shared settings first. Edit credits, Power Nodes and Heavy Pulse fields in the table below. This section contains the three purchase plugin switches."}
            ,{"金钱 / 信用点、大脉冲持有、弹匣和副模式都保留原字段路径；按钮只负责快速筛选，下半区保存时会自动备份并校验。", "Credits, Heavy Pulse ownership, ammo and alternate mode retain their original field paths. These buttons filter the table; saving below creates and verifies a backup."}
            ,{"快速定位：金钱 / 信用点与节点", "Find Credits / Nodes"}
            ,{"快速定位：大脉冲", "Find Heavy Pulse"}
            ,{"读取共享 settings.sb", "Read Shared settings.sb"}
            ,{"保存三个插件开关", "Save Three Plugin Switches"}
            ,{"游戏运行时可以读取，保存需先退出游戏；插件缺失字段会在保存时补齐。", "Reading is allowed during play. Exit the game before saving; missing plugin fields will be added when saved."}
            ,{"应用全部存档修改", "Apply All Save Changes"}
            ,{"游戏运行时可以读取；请在下方统一应用所有已读取的插件勾选和字段修改，保存前会自动备份。", "Reading is allowed during play; use the single apply action below for plugin checks and field edits. A backup is created before saving."}
            ,{"已应用全部存档修改并重读。自动备份：", "All save changes were applied and reloaded. Automatic backups:"}
            ,{"从备份创建副本", "Create Copy from Backup"}
            ,{"自由字段修改（下）", "Field Editor (Bottom)"}
            ,{"读取后按分组编辑全部可编辑字段；金钱 / 信用点、能量节点和大脉冲可在资源与武器分组中修改。修改前请先导出 ZIP。", "Read a save, then edit fields by category. Credits, Power Nodes and Heavy Pulse are in Resources and Weapons. Export a ZIP before editing."}
            ,{"检查点存档", "Checkpoint Save"}
            ,{"检查点", "Checkpoint"}
            ,{"存档 1（00）", "Slot 1 (00)"}, {"存档 2（01）", "Slot 2 (01)"}, {"存档 3（02）", "Slot 3 (02)"}
            ,{"章节存档", "Chapter Save"}
            ,{"生存存档", "Survival Save"}
            ,{"共享章节 / 成就", "Shared Chapters / Achievements"}
            ,{"武器", "Weapons"}
            ,{"背包", "Inventory"}
            ,{"章节与周目", "Chapters & Playthrough"}
            ,{"全部", "All"}
            ,{"帮助：0/1 为开关；物品类型须在本档中已有。只写当前文件，保存前自动备份并检查源文件未变化；不确定的数值不要修改。", "Help: 0/1 fields are switches; item types must already exist in this save. Only the current file is written, after a backup and source check. Leave unknown values unchanged."}
            ,{"只读", "Read Only"}
            ,{"生命值", "Health"}
            ,{"金钱 / 信用点", "Money / Credits"}
            ,{"能量节点", "Power Nodes"}
            ,{"护甲等级（RIG）", "Armor Level (RIG)"}
            ,{"生命升级", "Health Upgrade"}
            ,{"氧气升级", "Oxygen Upgrade"}
            ,{"念力可用", "Kinesis Available"}
            ,{"切割锯可用", "Ripper Available"}
            ,{"通关标志", "Completion Flag"}
            ,{"周目标志", "Playthrough Flag"}
            ,{"持有", "Owned"}
            ,{"弹匣 / 能量数值", "Ammo / Energy"}
            ,{"副模式状态", "Alternate Mode"}
            ,{"当前装备", "Equipped"}
            ,{"已解锁", "Unlocked"}
            ,{"完成标志", "Completion Flag"}
            ,{"关卡", "Level"}
            ,{"难度", "Difficulty"}
            ,{"名称", "Name"}
            ,{"保存关卡", "Saved Level"}
            ,{"保存类型", "Save Type"}
            ,{"字幕", "Subtitles"}
            ,{"自动装弹", "Auto Reload"}
            ,{"教程提示", "Tutorials"}
            ,{"反转垂直", "Invert Y"}
            ,{"新周目解锁提示", "New Game Plus Notice"}
            ,{"音效音量", "Effects Volume"}
            ,{"音乐音量", "Music Volume"}
            ,{"亮度", "Brightness"}
            ,{"灵敏度", "Sensitivity"}
            ,{"等离子切割器", "Plasma Cutter"}
            ,{"切割锯", "Ripper"}
            ,{"线性枪", "Line Gun"}
            ,{"岩石取芯器", "Core Extractor"}
            ,{"大脉冲", "Heavy Pulse"}
            ,{"项目、作者与版权说明。", "Project, authors and copyright information."}
        };
        private static readonly Dictionary<string, string> EnToZh = Reverse(ZhToEn);
        // Keep historical already-translated captions reversible so toggling
        // language never leaves a mixed page.
        private static readonly Dictionary<string, string> EnglishOnlyToZh = new Dictionary<string, string>(StringComparer.Ordinal)
        {
            {"Graphics Display", "画质与显示"}, {"Graphics & Display", "画质与显示"},
            {"Internal", "内部渲染"}, {"Internal Render", "内部渲染"},
            {"Output", "输出分辨率"}, {"Output Resolution", "输出分辨率"},
            {"Window", "窗口模式"}, {"Window Mode", "窗口模式"},
            {"FPS Limit", "帧率上限"}, {"Enable HD Texture Pack", "启用高清纹理包"},
            {"Automatic", "自动（主屏分辨率）"}, {"Close", "关闭"},
            {"MSAA", "多重采样抗锯齿"}, {"Anisotropic Filtering", "各向异性过滤"},
            {"Upscaler", "超分算法"}, {"Fast Present", "快速呈现"},
            {"Advanced Graphics", "高级画面设置"}, {"Advanced Movement", "高级移动设置"},
            {"Quality Preset", "画质档位"}, {"Performance", "流畅"},
            {"Balanced", "均衡"}, {"Clear", "清晰"}, {"High Quality", "高画质"},
            {"Custom", "自定义"}, {"Exclusive Fullscreen", "独占全屏"},
            {"Borderless Fullscreen", "无边框全屏"}, {"Unlimited", "不限"},
            {"Close (Diagnostics)", "关闭（诊断）"}, {"Undo", "撤销"}, {"Apply", "应用"}, {"2x · Installed", "2× · 已安装"},
            {"4x · Installed", "4× · 已安装"}, {"6x · Installed", "6× · 已安装"},
            {"2x · Not installed", "2× · 未安装"}, {"4x · Not installed", "4× · 未安装"}, {"6x · Not installed", "6× · 未安装"},
            {"FSR1 EASU (Light)", "FSR1 EASU（轻量）"},
            {"FSR1 EASU + RCAS", "FSR1 EASU + RCAS"},
            {"Automatic Configuration & Startup Status", "自动配置与启动状态"},
            {"Startup Review", "自动配置与启动状态"},
            {"+ Advanced Graphics", "高级画面设置"}, {"- Advanced Graphics", "高级画面设置"},
            {"+ Advanced Movement", "高级移动设置"}, {"- Advanced Movement", "高级移动设置"},
            {"+ Experimental & Diagnostics", "展开实验设置与诊断"}, {"- Experimental & Diagnostics", "展开实验设置与诊断"}
        };
        private static bool english = InitialEnglish();
        internal static bool English { get { return english; } }
        internal static string ToggleCaption { get { return english ? "中文" : "English"; } }
        internal static void Toggle()
        {
            english = !english;
            // Installer and launcher share an explicit UI choice. Test/CLI
            // overrides never change the user's saved preference.
            if (!String.IsNullOrEmpty(Environment.GetEnvironmentVariable("TOUCHHLE_UI_LANGUAGE"))) return;
            try {
                string file = LanguageFile();
                System.IO.Directory.CreateDirectory(System.IO.Path.GetDirectoryName(file));
                System.IO.File.WriteAllText(file, english ? "en" : "zh");
            } catch (System.IO.IOException) { }
              catch (UnauthorizedAccessException) { }
        }
        internal static string Translate(string text)
        {
            if (String.IsNullOrEmpty(text)) return text;
            string translated;
            if (english && ZhToEn.TryGetValue(text, out translated)) return translated;
            if (!english && EnglishOnlyToZh.TryGetValue(text, out translated)) return translated;
            if (!english && EnToZh.TryGetValue(text, out translated)) return translated;
            // WinForms can trim or collapse repeated spaces in button captions
            // after DPI/layout negotiation. Match the same caption by its
            // normalized whitespace so numbered navigation and headings do
            // not remain in the other language.
            string normalized = Normalize(text);
            Dictionary<string, string> table = english ? ZhToEn : EnToZh;
            foreach (KeyValuePair<string, string> pair in table)
                if (String.Equals(normalized, Normalize(pair.Key), StringComparison.Ordinal)) return pair.Value;
            // Runtime paths and hardware values make some captions dynamic.
            // Translate their stable fragments while preserving those values.
            if (english)
            {
                string dynamic = text
                    .Replace("屏幕匹配：", "Screen match: ")
                    .Replace("底分", "base")
                    .Replace("屏幕高度", "screen height")
                    .Replace("当前内部：", "Current internal: ")
                    .Replace("输出：", "Output: ")
                    .Replace("全量 mip 显存估算：", "Full mip VRAM estimate: ")
                    .Replace("：", ": ")
                    .Replace("约", "about ")
                    .Replace("当前", "Current ")
                    .Replace("\r\n可在「游戏与存档位置」多选安装纹理，在本页选择使用的档位。", "\r\nSelect multiple texture tiers on Game Data and choose the active tier here.")
                    .Replace("启动器配置：", "Launcher config: ")
                    .Replace("原生配置与存档：", "Native config and saves: ")
                    .Replace("已识别：", "Detected: ")
                    .Replace("已识别:", "Detected: ")
                    .Replace("已按屏幕预置；保存后下次直接读取配置。", "screen preset applied; the saved configuration will be reused next launch.")
                    .Replace("另需 CPU 解码和驱动空间；估算不是实测峰值。", "CPU decode and driver space are additional; this is an estimate, not a measured peak.")
                    .Replace("已选择「", "Selected ")
                    .Replace("已恢复默认；启动时保存。", "Defaults restored; changes will be saved when the game starts.")
                    .Replace("已撤销未应用的设置。", "Unapplied changes were undone.")
                    .Replace("设置已应用并保存；下次启动游戏时生效。", "Settings applied and saved; they take effect next time the game starts.")
                    .Replace("」；内部与输出分辨率保持独立。", "; internal and output resolution stay independent.")
                    .Replace("。", ".")
                    .Replace(".CPU", ". CPU");
                if (!String.Equals(dynamic, text, StringComparison.Ordinal)) return dynamic;
            }
            return text;
        }
        private static string Normalize(string text)
        {
            StringBuilder result = new StringBuilder(text.Length);
            bool space = false;
            foreach (char value in text.Trim())
            {
                if (Char.IsWhiteSpace(value)) { space = true; continue; }
                if (space && result.Length != 0) result.Append(' ');
                result.Append(value); space = false;
            }
            return result.ToString();
        }
        internal static bool IsApplying { get; private set; }
        internal static void Apply(Control root)
        {
            bool previous = IsApplying;
            IsApplying = true;
            try { ApplyControl(root); }
            finally { IsApplying = previous; }
        }

        internal static string TranslateEditorLabel(string text)
        {
            string[] parts = text.Split(new[] { " · " }, StringSplitOptions.None);
            for (int i = 0; i < parts.Length; i++) parts[i] = Translate(parts[i]);
            return String.Join(" · ", parts);
        }

        // Status and marker labels are assigned after the initial language pass.
        // This label translates those later assignments while still allowing
        // ApplyControl to translate the whole tree when the toggle is pressed.
        internal sealed class LocalizedLabel : Label
        {
            private bool translating;
            private string sourceText = String.Empty;
            internal void ReapplyLanguage() { translating=true; try { base.Text=Translate(sourceText); } finally { translating=false; } }
            protected override void OnTextChanged(EventArgs e)
            {
                if (!translating && !IsApplying) sourceText=base.Text;
                if (!translating)
                {
                    string raw = base.Text;
                    string translated = Translate(raw);
                    if (!String.Equals(raw, translated, StringComparison.Ordinal))
                    {
                        translating = true;
                        base.Text = translated;
                        translating = false;
                    }
                }
                base.OnTextChanged(e);
            }
        }

        internal sealed class LocalizedTextBox : TextBox
        {
            private bool translating;
            private string sourceText = String.Empty;
            internal void ReapplyLanguage() { translating=true; try { base.Text=Translate(sourceText); } finally { translating=false; } }
            protected override void OnTextChanged(EventArgs e)
            {
                if (!translating && !IsApplying) sourceText=base.Text;
                if (!translating)
                {
                    string raw = base.Text;
                    string translated = Translate(raw);
                    if (!String.Equals(raw, translated, StringComparison.Ordinal))
                    {
                        translating = true;
                        base.Text = translated;
                        translating = false;
                    }
                }
                base.OnTextChanged(e);
            }
        }

        private sealed class TextSource { internal string Value; }
        private static readonly ConditionalWeakTable<Control, TextSource> Sources = new ConditionalWeakTable<Control, TextSource>();
        private static TextSource OriginalText(Control control)
        {
            return Sources.GetValue(control, delegate(Control c) {
                var source = new TextSource { Value = c.Text };
                c.TextChanged += delegate { if (!IsApplying) source.Value = c.Text; };
                return source;
            });
        }
        private static void ApplyControl(Control control)
        {
            if (control.Tag as string == "ui-language-toggle" || control.Tag as string == "explicit-bilingual") return;
            // Always retranslate the original caption. Reversing a partially
            // translated dynamic string left English fragments in Chinese UI.
            if (control is LocalizedLabel) ((LocalizedLabel)control).ReapplyLanguage();
            else if (control is LocalizedTextBox) ((LocalizedTextBox)control).ReapplyLanguage();
            else if (!(control is TextBoxBase) && !(control is ComboBox))
                control.Text = Translate(OriginalText(control).Value);
            var grid = control as DataGridView;
            if (grid != null)
            {
                foreach (DataGridViewColumn column in grid.Columns) column.HeaderText = Translate(column.HeaderText);
                foreach (DataGridViewRow row in grid.Rows)
                {
                    if (row.IsNewRow || row.Cells.Count < 3) continue;
                    row.Cells[0].Value = TranslateEditorLabel(Convert.ToString(row.Cells[0].Value));
                    if (row.Cells[2].ReadOnly) row.Cells[2].Value = Translate(Convert.ToString(row.Cells[2].Value));
                }
            }
            var combo = control as ComboBox;
            if (combo != null)
            {
                int selected = combo.SelectedIndex;
                for (int i = 0; i < combo.Items.Count; i++)
                {
                    string text = combo.Items[i] as string;
                    if (text != null) combo.Items[i] = Translate(text);
                }
                if (selected >= 0 && selected < combo.Items.Count) combo.SelectedIndex = selected;
            }
            foreach (Control child in control.Controls) ApplyControl(child);
        }
        private static bool InitialEnglish()
        {
            string value = Environment.GetEnvironmentVariable("TOUCHHLE_UI_LANGUAGE");
            if (String.Equals(value, "en", StringComparison.OrdinalIgnoreCase)) return true;
            if (String.Equals(value, "zh", StringComparison.OrdinalIgnoreCase)) return false;
            try {
                value = System.IO.File.ReadAllText(LanguageFile()).Trim();
                if (value == "en") return true;
                if (value == "zh") return false;
            } catch (System.IO.IOException) { }
              catch (UnauthorizedAccessException) { }
            return !String.Equals(CultureInfo.CurrentUICulture.TwoLetterISOLanguageName, "zh", StringComparison.OrdinalIgnoreCase);
        }
        private static string LanguageFile()
        {
            return System.IO.Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "DeadSpaceLegacy", "ui-language.txt");
        }
        private static Dictionary<string, string> Reverse(Dictionary<string, string> source)
        {
            var result = new Dictionary<string, string>(StringComparer.Ordinal);
            foreach (var pair in source) if (!result.ContainsKey(pair.Value)) result.Add(pair.Value, pair.Key);
            return result;
        }
    }
}
