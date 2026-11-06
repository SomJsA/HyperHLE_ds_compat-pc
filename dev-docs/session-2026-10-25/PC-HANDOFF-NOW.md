# PC 交接总入口（2026-10-25，用户要求立即交接）

本文优先于旧计划、旧会话推断和历史工具注释。当前停止新增实现，保留运行现场。

## 1. 接手首先遵守

- **不要关闭游戏，不要抢前台，不要替换正在运行的程序，不要修改活动存档。**
- 用户禁止 computer-use；操作测试由用户完成，通过 F5/F7 标记。不要使用输入注入、窗口点击或自动游玩。
- 用户正在测试另一处近距离雾。开局闪黑已经明确回复“已修复”；近距离雾尚未回报。
- 最后指令：“不论做了多少，马上整理并生成交接级文档”。恢复实现前以用户新指令为准。
- 无子代理授权。参考 WII / Android 工程只读。
- 工程有大量已有 Rust 格式化、SDL/vendor、Android 和构建产物变化，**禁止 git reset/clean 或全量暂存**。

仓库：`E:/CodexProjects/DeadSpaceLegacy/repo/HyperHLE_ds_compat`

分支：`codex/android-mainline-merge`。
外层工程：`E:/CodexProjects/DeadSpaceLegacy`。默认工具 cwd 可能是 DEMObile，命令应显式指定仓库。

## 2. 当前运行现场、版本与存档

交接检查时游戏 PID **65144**；OBS PID8120。PID会变化，使用前重新检查。

| 用途 | 路径 / 状态 |
|---|---|
| 正在运行 | `builds/pc-opening-diagnostic-20261025/touchHLE.exe`，开局修复已验收，哈希7dbcc… |
| 正式运行目录 | `run-host/Runtime-PC`，仍是早期已验收E/F/QTE版本，未部署最新雾修复或启动器 |
| 当前带存档测试证据 | `logs/pc-near-fog-saved-20261025`，F5/F7仍可留证 |
| 新启动器暂存 | `builds/pc-launcher-next-20261025/DeadSpace Launcher.exe`，未部署、未给用户验收 |
| 新运行时构建 | 仓库 `target-local-verify/release/touchHLE.exe`，编译成功，未部署、未做新版启动冒烟 |

以上相对路径除特别标明外均相对于外层工程。

哈希：

- 当前已验收开局修复：`7dbcc24cf145ec6f590e16beb7963d0147b41ffce66ef91efd44dc231531864c`，提交 `bd949378`。
- 新运行时（鼠标失焦/改键）：`1914fd651cbfcfa51e8bdede34d563dd2ed4dd1bc19599c03a11c8ab6b01f965`。
- 新启动器：`4e0de51f46a5e55070ce77a2700f94b8f2697665312f0475768fdd366047d626`。
- 正式旧运行时：`dba3e40eb93daa9eb4ceaa4749fbb532c15da8ae6d8c407de87cd29126caa9ff`。

用户指出独立新游戏存档不能进后面关卡后，20:24检查游戏已退出；已将正式 Runtime-PC 的
`touchHLE_sandbox` **复制**到诊断运行目录，核对 var 顶层13文件哈希。
双方原档备份在：
`E:/CodexProjects/DeadSpaceLegacy/builds/save-backups/near-fog-20261025-202434`
（`diagnostic-before` / `production-source` / `manifest.json`）。正式原档未改。
**用户此后的进度在诊断运行目录！未来切版前先保存这份新进度，不要用旧正式档覆盖它。**

OBS最后由我们启动录制，交接时未停止。不要自动停止用户当前雾测试录像。
WS接口 `ws://127.0.0.1:4455`，密码只读本地配置不要输出；外层 `tools/obs_record_clip.py`
有辅助函数。当前录制状态应下次通过只读API确认。输出目录沿用用户OBS配置：
`E:/DS2-Severed-Patch/artifacts/r1889c_npcs_grouphash_cdb/`，不要改全局配置。

## 3. 已验收与未验收，严格区分

**用户明确验收：** F对怪物动态掉落物念力；E大怪/小怪抓取QTE、第二段横切、图1全身附着小怪；运行标题；开局黑一下修复。

**实现但未单独实测：** E不做念力，QTE > 拾取 > 场景交互；拾取失败/背包满对象屏蔽3秒。
这些在当前雾修复运行时中，但背包满挡操作台场景还需验收。

**本次后台完成但未部署：** 新启动器、三个插件解锁编辑、键盘改键、鼠标失焦释放/手柄不抢鼠标。
测试通过不等于用户体验验收，详见下节。

## 4. 后台新增实现与测试

### 启动器

源码 `tools/pc-runtime/`：

- `Program.cs` 保留已验证渲染启动参数和原INI字段；改用partial外壳；支持新改键配置。
- `LauncherShell.cs` 原生WinForms深蓝黑侧栏，六页：画面、键鼠、兼容、游戏文件、插件、改键。
- `KeyBindings.cs` 15动作的字母/数字改键、冲突检查，保存 `DeadSpace-Keys.ini`。
- `SaveSettings.cs` 经校验的SBIN解锁编辑器，见关键纠错。
- `Build-Launcher.ps1 -OutputDirectory <暂存目录>`，使用系统.NET Framework64 csc。
- `Verify-Launcher.cs` 测试入口，**不是生产入口**；用 `/main:DeadSpaceBootstrap.VerifyLauncher`。

后处理三个方案：原始(关闭FSR/CAS)、轻量(EASU)、超分锐化(EASU+RCAS)。按钮设置真实控件，
自定义选项仍可调整，不声称支持PC端FSR3。F5说明已改成诊断快照；普通进度由游戏自身保存。
现有INI保留，未改运行中的配置。新UI无原版封面图；手柄改键、独立控制方式选择尚未完成。

测试：启动器编译成功；六页离屏渲染。仅第0页已人工看图检查布局，其余是成功生成，
**不能写成全部视觉验收通过**。结果 `builds/pc-launcher-next-20261025/verification/page-0..5.jpg`。
离屏测试只创建自己的控件句柄并DrawToBitmap，没有Show/Activate，也没有读取或操作桌面。

### 鼠标与改键

- `src/window.rs` 已有 `SDL_SetRelativeMouseMode`；先前“没找到捕获API”的判断已纠正。
- 新增失焦立即释放、清除视角累积、抬起guest Look触点。返回后F8/Alt重新捕获。
- 手柄视角仍走guest看向手势，但不再因手柄摇杆而抓住主机鼠标。
- **移动和视角目前仍由guest触摸手势实现，并未完成原生Vector2Event路径。**
- `src/ds_key_bindings.rs` 解析action=key，按下/松开同一映射，移除被替换的旧绑定。
  默认15动作含WASD、E/F/C/R/Q/X、1..5；G/M/N/P/V、功能键和修饰键保留。
  `TOUCHHLE_DS_KEY_BINDINGS` 指向配置，启动器会设置；无配置保持旧路径。
- 3个纯Rust测试通过：默认/交换、重复/保留键拒绝、换到新键后旧键失效。
- 两次cargo check通过；最终release构建通过，5m04s，517条已有警告。
  日志 `logs/pc-launcher-input-20261025-build.txt`。
- **未完成：** 失焦时移动键/瞄准/开火统一释放、SDL捕获失败反馈、手柄改键、输入模式切换。
  新版没有执行startup smoke，未覆盖手柄或游戏内改键实测。

### 存档：重要纠错，禁止使用旧DLC工具

`dlc_modules` **不是位掩码**！外层 `SAVE_FORMAT.md` 旧章节、`ds-dlc.cmd`、
`sbobjects.py --set-dlc` 的原写法错误，接手不要运行这些修改命令。

- settings.sb顶层dlc_modules是OHDR引用，指向含`nodes_2`、`nodes_1_registration`、
  `attack`、`defense`、`loot`的对象；每个插件对象有`unlocked`。
- 插件对象声明size13，unlocked位于+12，占**1字节**，后面的CD是填充；只改布尔字节。
- Checkpoint里的dlc_modules是type0x0d字符串ID数组，`2`是数组长度，
  实际条目为nodes_2/nodes_1_registration；旧工具误把数组长度当位掩码。
- `SaveSettings.cs`验证v2、区段界限/FNV-1校验、名称和OHDR；只修改三个已命名布尔及DATA校验。
- 任何touchHLE进程存在时禁止写；两次核对源文件未变；临时文件重新解析；File.Replace原子替换，唯一命名备份。
- 全8种开关组合通过，改回原状态逐字节一致；最多3开关字节+4校验字节变化；损坏文件拒绝。
  使用备份fixture，**没有修改任何活动存档**。
- 编辑的是当前运行目录的settings.sb，所有档位共享。UI明确提示。未声称逐档独立开关。
- UI标签attack攻击增强/defense防御增强/loot资源增强；loot具体游戏文案/效果还需核对。
- **游戏内开/关实际效果尚未验证**，不要改关卡已应用列表来冒充实现。
- 全物品和进度编辑尚未实现。用户要求加载已有进度逐个拍长图，**禁止自行打通流程**。

## 5. 用户新增武器/原生输入要求（最新范围）

用户明确希望：各枪开火手感对齐；切枪、寻路、暂停尽量底层；考虑每把武器主/副模式，
可切换或自动识别当前武器模式后适配按下/持续/松开。
**目前只完成静态逆向调查，没有新增这些动作的运行时代码。不要把下列入口当成已接线功能。**

现状：1..5切枪宏是点击轮盘→等20帧→点武器坐标；P/ESC暂停是两次点击。
左键PC鼠标开火仍按`TOUCHHLE_DS_AIM_XY`注入触点。右键瞄准；RT也有旧触摸开火路径。
不要用统一定时连点替代各枪状态机，不要直接调用OnTryFire作“只读探测”（有副作用）。

### 已确认入口（iOS1.3.34 ARM）

| 原生入口 | 地址 / 语义 |
|---|---|
| GameObjectPlayable::OnEvent | 0x53dc8；Vector2Event id1006，Action id1008 |
| InputForwarderTouchDPad::SendDPadEvent | 0xe5c0c，参数float x/y |
| GameObjectPlayable::ChangeWeapon | 0x501fc；自带武器拥有/状态/禁止切枪检查 |
| CanChangeWeapon / IsWeaponAvailable | 0x3afe8 / 0x3b2a8 |
| Hud::SelectWeapon | 0x18f1f4，尾部0x18f384调用ChangeWeapon |
| Hud::OnStringIdEvent | **0x194158**；优先研究它，保留原HUD状态门控 |
| Hud::CanUseLocator / CanEnterRig | 0x18b998 / 0x18b9ec |
| Hud::ShowPauseMenu / HidePauseMenu | 0x18dd84 / 0x18cfb4；不要绕过上层门控直接硬调 |
| Playable::DoLocatorMeshAnim | 0x4af1c；原BTN_MARKER分支传false，**不是true** |

`ds_current_hud_player`已存在于gles_guest.rs，可从GetHud0x30474→hud+4 world→world+e0 player，
并核对player vptr0xa7a4f8。不要为每帧原生输入再全内存扫指针。

Hud::OnStringIdEvent按hud+0x234状态分发；state1正常状态支持武器、定位和暂停按钮。
事件中+8/+12是eastl wchar字符串begin/end。已解析到游戏只读字符串：

| 字符串 | 地址 |
|---|---|
| BTN_RIG | 0x8ee380 |
| BTN_PLAY_PAUSE | 0x8ee3a0 |
| BTN_MARKER | 0x8ee3dc |
| BTN_PLASMA | 0x8ee68c |
| BTN_RIPPER | 0x8ee6b8 |
| BTN_LINEGUN | 0x8ee6e4 |
| BTN_CORER | 0x8ee714 |
| BTN_BURST | 0x8ee73c |

原生武器序号：Plasma0、Ripper1、LineGun2、Corer3、Burst4；旧宏数字键顺序不同：
1 LineGun、2 Plasma、3 Ripper、4 Corer、5 Burst。接手不要无意换掉用户键位语义。
可用对应按钮事件取代屏幕坐标，仍需验证StringIdEvent完整布局/生命期与返回值，**尚未实现**。
BTN_PLAY_PAUSE在state1→请求state2，state3走恢复路径；BTN_MARKER分支检查hud+240=0、
state1、player非空和CanUseLocator，再调用DoLocatorMeshAnim(false)。保留原游戏禁用条件。

### 武器类型与模式证据

当前武器索引player+0x2ec，对象表player+0x2d4+index*4。vtable公共槽：
`+0x170 OnTryFire`、`+0x190 IsHolding`、`+0x194 CanFire`、`+0x19c IsAltMode`、
`+0x1a0 SetAltMode`、`+0x1d0 GetFireSpeed`、`+0x1d4 GetFireHoldSpeed`。

| 类 | vptr | IsAltMode | SetAltMode |
|---|---|---|---|
| PlasmaCutter | 0xa7ace8 | 0x72940 | 0x73674 |
| Ripper | 0xa7b9f8 | 0xbb230 | 0xb9d54 |
| Corer | 0xa7c1c8 | 0xcef2c | 0xd0608 |
| LineGun | 0xa7dea8 | 0x1243c0 | 0x1261bc |
| BurstRifle | 0xa8a498 | 0x3079d0 | 0x3096a0 |

OnTryFire静态证据：Ripper0xba780主模式进入持续状态并返回1，副模式调用OnFire返回0；
Corer0xd09a0相反（主模式OnFire返回0，副模式持续返回1）；Burst0x3099f8成功返回1。
这表明必须按“枪械+模式”处理。OnTryFire还会做音效/状态改变，绝不可用于探测。
玩家Action6（0x541c0）会检查武器并调虚函数，返回0/1/2分别导入不同状态/装弹；
它也兼有瞄准/念力状态行为，**不能直接把6称为无条件开火**。
Action7=已有旋转/副模式路径；8/9分支涉及收枪、结束持续等，需继续完整逆向，不能猜编号。

证据文件（外层logs）：`pc-native-input-symbols.txt`、`pc-native-player-onevent.arm.txt`、
`pc-native-hud-onevent.arm.txt`。后者从0x1957f0开始的转储扩出了函数范围，真正Hud::OnEvent
结束于0x195a00；勿误将后续代码归它。OnStringIdEvent另用外层tools/arm-dis.py读取。
二进制：`device-app/extracted/Dead Space.app/Dead Space`。分析纯离线，没有操作当前游戏。

## 6. 开局修复的精确结论

不是普遍关闭雾。黑色LINEAR雾正常10..25，在开局长时间零范围后恢复为极小正值
（如.001662.. .004155..），令环境饱和黑；角色雾豁免所以仍可见。
红雾对照由用户确认。最终 `src/gles/ds_opening_fog.rs` 记住正常基线，零范围仅等待，
首个正范围恢复才开始240帧保护，恢复正常/不匹配即结束；GL调用后恢复guest原始设置。
限黑色LINEAR、透视near约.05等条件，不全局抬高雾距离。
第一版650275ef失败，**只有bd949378修订版获用户验收**。
详见 `PC-OPENING-FOG-FIX.md`。不要重跑已经验收的红雾轮次。

重要陷阱：PowerShell `[Environment]::SetEnvironmentVariable(name,$null)` 在本环境产生空串；
Rust var_os().is_some()仍开启诊断。脚本使用 `Remove-Item Env:NAME`，并核对manifest。

## 7. 下位AI建议接续顺序

1. 先等用户近距离雾反馈；游戏仍开着时只做后台，不替换/迁移存档。
2. 读取本文和用户最后指令；确认何时恢复开发。当前交接动作不是“全部目标完成”。
3. 检查原生HUD StringIdEvent构造，先接切枪/定位/暂停；用原生门控，禁用场景不强行执行。
4. 武器识别/模式/按下松开日志先行；明确每把枪的hold/release行为后接原生输入。
5. 补移动/瞄准/开火在失焦和模式切换时的统一释放；完成手柄方案与改键，保护E/F/QTE。
6. 新运行时先独立启动冒烟；用户退出后再建立带当前最新存档的验收版本。
7. 验证三个DLC在游戏里的实际开关效果；再扩展物品/进度编辑，不能用错误位掩码工具。
8. 关卡长图仅加载现有存档，安排用户操作，绝不自行游玩推进。

构建环境：RUSTUP_HOME=`E:/CodexProjects/DeadSpaceLegacy/tools/rust/.rustup`，
CARGO_HOME=`E:/CodexProjects/DeadSpaceLegacy/tools/rust/.cargo`，PATH加cargo/bin。
`cargo build --release --target-dir target-local-verify -j 2`，日志重定向到外层logs。
查阅输出要裁剪，不整段读大日志；图片先量大小，超过400KB先缩小。

其他详记：`PC-LAUNCHER-SAVE-IMPLEMENTATION.md`、`PC-NEXT-PLAN.md`、
`PC-QTE-KINESIS-FOLLOWUP.md`。旧文档按时间保留，本文的状态表优先。
