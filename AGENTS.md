# DeadSpace Legacy / HyperHLE_ds_compat

## 可选矩阵追踪的默认开销

- 热路径矩阵追踪须传递同步fmt::Arguments，在输出路径、动态marker及文件打开成功后才格式化；不要在关闭的日志调用前构造format!字符串。marker存在状态保持动态检查，参数不跨线程/不延长guest指针生命周期。默认关闭、trigger移除重建及失败输出须用实际格式化计数/文件输出验证；组件测试或采样中的floatformatter不能冒充FPS或单核问题已解决。见`dev-docs/PC-LAZY-MATRIX-TRACE.md`。
- 仅显式私有control-inbox的`perf matrix-format eager/lazy`可复现旧格式化时机进行同进程对照；普通启动保持lazy。旧字面量调用不得在eager校准中凭空增加分配。等待实际formatted路径的effective回报后在窗口外预热，不把请求ACK当生效，不把跨进程不利结果或构图差异丢弃。

## PC正常关闭互斥锁

- Windows Dead Space仅在窗口关闭进入原生生命周期回调后启用完全释放且有等待者的mutex调度让出；保留原生resign/terminate/通知/存档路径、递归层级及调度器正常relock。显式`TOUCHHLE_DS_EXIT_MUTEX_HANDOFF=0`保留旧行为对照。不得用强制退出、跳过回调或代替线程解锁来消除超时。见`dev-docs/PC-EXIT-TRACE.md`；关闭成功不关闭性能/长测验收范围。

## PC EAGL传输缓存维护

- 直接呈现的ES1纹理/源FBO、ES2程序/对象/尺寸及附件记录也必须归实际backing EAGL上下文持有；与GL driver使用同一个有效线程/主线程fallback解析。为辅助呈现生成独立对象，不按sharegroup复用附件/尺寸记录。EAGL释放时先使owner当前，释放私有传输/后处理/卡片资源，再恢复已有线程上下文映射。CPU-onlyRGBA scratch可继续按宿主线程复用。见`dev-docs/PC-EAGL-TRANSFER-CONTEXT.md`。独立纹理0x501复现、共享附件及当前上下文释放回归不等于玩家10h6x→4x90degree精确复现。

## PC原生字体角色维护

- 标题/前端/章节使用用户指定MStiffHei，正文字幕及游戏内原生UI使用Swei Regular，精确资源指纹由guard固定。两种FontCGFont/FontUIFont都须保留经过原指令验证的GetID路径，覆盖三个GlyphBuffer调用者；通过原生计数器保留新ID，不能猜高位标签或改写guest cmap。字体归属必须跨暂停输入层HUD UNKNOWN保持，直到真实前端状态切换；不能把暂时没有玩家输入所有权当成返回前端。原生章节退出会先回菜单12，等待各页真实按钮/动画就绪再继续，不能仅凭state日志判定。见`dev-docs/PC-FONT-ROLES.md`；窗口退出超时的既有样本与完整R15验收仍保留。

## 开局一次性注册询问

- 注册询问走WidgetHandler的Renderer OnDraw重载，HUD弹窗可走SpriteGraphics重载；两条原指令验证过的观察入口均须保留。不能用首次空档难度页替代第一存档点后重启的page2回归。控件/动画就绪和原生按钮回调仍为准，不写注册标记或用扫屏代替。虚拟手柄先挂接/释放再按下，避免把SDL未发出DOWN的样本算通过。见`dev-docs/PC-STARTUP-REGISTRATION.md`。

## 呈现上下文缓存维护

- 呈现shader/program/uniform、FSR texture/FBO和失败标记必须属于实际GL context；同线程或sharegroup不代表FBO可复用。Window内部合成、每个EAGLContext和Android私有shader context各持有PresentState。不得恢复成thread_local全局GL句柄；资源释放必须在所属context当前时执行。真实双context、6x/4x、FSR0/1/2像素回归见`dev-docs/PC-PRESENT-CONTEXT-CACHE.md`。这处缺陷已复现/修复，不把短测或白屏复现冒充玩家10h后90度旋转的精确复现，也不算单核问题已解决。

## 开始工作

- 用户最新指令优先。完整性能、原生输入/UI、PC/Android release 和最终验收目标不能因打包成功而缩小。
- 先读 `dev-docs/R15-RELIABILITY.md`、`tools/translation/PATCH-MODEL.md`；运行 `tools/project-guard/guard.py`，参见其 README。历史经验、旧截图和旧状态不是当前配置来源。
- 先复核分支、HEAD、status、submodule。只显式暂存本次文件；禁止 `git add -A`、reset 或覆盖未知改动。不要把实验工作区整个纳入 Git。
- PC 与 Android 是不同实现基线。共享数据/协议，逐段移植；不得用 PC 整个文件覆盖 Android，尤其是 `gles_guest.rs`、`gles/util.rs` 和输入/窗口代码。

## 不可回退的契约

- 原版/已验收 IPA、存档和发布件不可覆盖。原 IPA + 运行时只读 SBIN 挂载是正式模型。生成派生 IPA 仍需用户明确确认；普通构建、检查不得调用旧 IPA 重打包脚本。
- 当前默认样式源是 `src/ds_overlay_default.json`，Android asset 副本必须匹配。`tools/overlay-studio/default-style.json` 是历史工作台预设，不可直接部署作正式默认。字号以 PC 为准：**“目标：”标题18，正文独立适配**；章节和字幕的布局、字体度量也须比对，不能只比较字号数值。保留用户显式设置。
- 台账导入后同时核验996键的 native SBIN/JSON、占位符、章节藏头、字体、文本修正清单。`CREDITS` 是信用点，`credits_list` 是制作名单。不得只改 JSON。
- 默认键鼠 Ctrl/G/B 定位，手柄 L3 转身、R3 定位。教程与重绑定来自实际动作语义；不要把 canonical dispatch 表误当物理默认。至少验证一次非默认绑定。
- E 交互沿用原生距离/朝向/战斗门槛。按键释放、站点进入、移动/疾跑状态及模态/换甲动画必须有明确所有者；回归覆盖连续进入/退出及完整换甲动画，不能以屏蔽原生门槛掩盖故障。
- 按住瞄准必须使用独立的物理按键边沿：鼠标右键/手柄 LT 的逻辑输入即使被换弹、UI 或取消路径重置，真实松开仍必须发送 native aim release。toggle 模式保持点击语义；回归必须在换弹进行中释放并确认完成后 `aiming=0`。
- PC QTE 按键提示也必须读取原生 HUD 布局/渲染参数；E 主路径改为内存不代表提示路径已清理。禁止在被抓等状态轮询全分辨率 framebuffer 找提示。未知原生代码保留原提示，不自动退回抓帧。运行 project-guard 的 QTE 发布函数检查，不能只核对资源哈希。性能窗口覆盖原生 state26 前的完整被抓阶段，不能仅截取最后一次低频存活探针之前的轻负载部分。
- 新PC分发包配置/存档/编辑器备份默认系统文档目录DeadSpaceLegacy；旧中文叶目录复制校验后保留，不自动合并。游戏运行时程序与已安装 GameData 分离：安装器默认程序和 GameData 位于 LocalAppData 用户可选目录，Documents 只放配置、三槽存档与编辑备份。用户交付为标准安装 EXE + 只读 marker；程序升级不得覆盖 GameData 或 Documents。诊断默认程序目录diagnostics，原IPA由 marker 安装进 GameData。布局 v1 见 `dev-docs/STORAGE-COMPATIBILITY.md`，安装路径契约见 `dev-docs/PC-INSTALLER-DISTRIBUTION.md`。保留旧根 INI 与已有配置，未知布局拒绝写入；不能为改名丢进度。
- 外测自动诊断遵守 `dev-docs/PC-PLAY-DIAGNOSTICS.md`：前30秒关键留存、内存有界/磁盘不固定封顶、后台I/O及自保护，缺口明确；默认无抓帧、逐键输入、个人路径或上传。未正常结束不等于崩溃，压力与真实开关A/B须分别核验。
- PC首次读取物理显示器预置并保存：1080P4×、1440P6×、2160P8×，纹理/MSAA/AF默认4×。后续直接读取已保存配置，不询问/自动运行短测，不根据旧benchmark缓存改设置；自检只显示当前运行实际特性。显式自动匹配仍可选，质量档不得改分辨率；原生长帧完整保留。
- 三槽 ZIP 包含共享 Documents 依赖和合法零字节依赖；空 SBIN/损坏/越界/重复/路径穿越拒绝且无写入。编辑仅改用户选择的已有字段；Android checkpoint/level 同步时也不能连带覆盖未改的信用点/节点。导入/编辑前自动备份，恢复须字节核验。克隆槽位夹具不等于三个原生有效进度。
- HD PNG 的实际上传路径、尺寸、alpha-aware mip 必须在运行时观察。自动原图 mip 修复不能抢在选定替换图之前。8x限制与显存估算如实标注，估算不是实测峰值。
- 多核先用于纯资源处理：RGBA/mip/预解码任务只能读取输入和写入独立输出；GL、guest内存/游戏状态及原生输入继续由所属线程管理。不得用 unsafe Send/Sync 强行共享 Environment。并行输出须逐字节对照；加载收益不能冒充实战主循环多核或稳定帧率。
- Android minSdk21；新增 API 必须检查可用版本。主 release 必须正常完整构建；复用 `.so` 仅限隔离 SaveLab，并核验其与发布件逐库一致。

## 运行与证据

- 正式PC新配置默认全屏，后续尊重已保存模式；自动化用隔离窗口/隐藏后台覆盖，仅写测试副本。PC 优先 `TOUCHHLE_DS_CONTROL_INBOX` 和隐藏测试进程；不注入宿主键鼠、不强制前台。不终止未知进程/VM。
- ADB/VM 授权与用户偏好沿会话持续，不重复询问。ADB 显式 serial/package/UID，测试用独立包和副本；升级前新鲜备份当前 APK/数据，不能用最早备份推定当前版本。**ADB不用时熄屏，测试 finally 中 KEYCODE_SLEEP**；离线 guard 不查询设备。
- 有限设备任务优先用 `tools/project-guard/device_job.py` 包装（README示例），其finally在失败/超时/中断后仍请求熄屏；子任务仍须自行恢复数据/释放输入，不能把包装器当完整清理。包装器和子任务必须指向同一明确serial。
- 隔离前写可恢复日志；恢复校验完整路径和目标不存在，保留测试结果，恢复原目录/设置/APK并核验哈希。恢复脚本不得盲目重跑，禁止 clear/uninstall 原应用。
- 串行管理同一设备/私有 runtime 的 worker。开始新测试前确认上一 worker 已结束；测试 finally 释放触点/按键和休眠，不让后台自动化与用户冲突。
- 先辨认真实菜单和游戏状态，不写死“前12项就是章节”。当前17项交错菜单映射见 guard contract；更改时从实际菜单重新审计。探针含义先看平台实现，Android F6 不是 PC 检查点。
- 超时先分清导航/观测器失败与游戏失败；状态变化日志不是持续快照。通过结论须关联原生事件、时间推进和实际画面；预览不能代替原生章节/目标/字幕验收。
- `frame 300 reached`只证明present仍活跃，不证明主菜单准备好；输入ACK也不证明菜单已切换。档案/模式/章节各页分别确认，等待实际UI和动画结束。玩家对象已创建仍可能是过场（已确认state29）；性能入窗必须另有可操作状态和实画面，不能把过场算移动/战斗。
- AF日志中的`actual`若仅来自请求值钳制，不能当作驱动读回；HD上传必须核验GL错误，过滤设置与MSAA回退单列。不能把失败设置回退到原图后的帧率记为高档资源性能。
- 源码、构建资源、产物、实际安装四层分别核对。发布清单记录 implementation commit、编译输入/文件 SHA、样式/字体/文本及 native库；文档 HEAD 不冒充构建来源。APK安装成功不等于装对版本。
- 性能固定场景/配置/预热，记录 native present interval；截图、探针、加载/暂停和温漂单列。动态场景样本不能当静止AB，封顶FPS不能证明余量。请求开启不等于实际启用：MSAA/FSR/FG回退必须检查。VM llvmpipe不算第二硬件GPU。
- 人工验收和未完成门槛保留在最新状态文件，不能由脚本批量改成通过。新状态明确指向上一记录与实际证据；历史记录保留并标识失效原因。

## 输出与检索

- 先限定目录和文件类型，排除 vendor/target/build/cache；大目录状态先摘要，不把整个未跟踪文件列表带入上下文。
- Windows 文本显式 UTF-8；控制台 JSON 用 ASCII 转义或设置 UTF-8，检查每步退出码再执行依赖步骤。
- 看图前量字节，>400KB先缩至<=1568px，优先局部；不重复看同图，结论落盘。长日志定位后摘要。
- 请求体字节才决定413；tokens不是体积。rollout 有 compact 时文件大小会虚高，不能据此叫停。沿用用户全局上下文卫生规则。

## 全章节采集包维护

- PC诊断退出采用会话级准入关闭/有界排空：Frame/Event/Feature与F7 Problem/Picture统一处理，关闭前获准的生产者在排空预算内仍可提交，关闭后的新请求拒绝。后台150ms排空、主线程最多200ms退出等待；队尾、在途、拒绝、超时和最终flush错误分别记录，不把“正常退出”当完整落盘。真实worker/并发生产者/小PNG/超时回归必须覆盖；不得仅改queued_tail计数或延长游戏帧等待。见`dev-docs/PC-PLAY-DIAGNOSTICS.md`。

- PC增强/超级指标位于 `src/ds_scene_diagnostics.rs`；包内诊断映射由 `tools/pc-runtime/Build-DiagnosticMap.py` 在干净实现提交上只读生成，build-info绑定SHA。不得用旧源码行/IPA索引映射分析新二进制。
- 全流程、分次会话与异常前30秒均保留；导出默认全部会话，附件仍明确选择。新指标必须写单位、线程/时间/缺口及边界，不把JIT代码读取、部分render key、嵌套HLE wall span当作编译耗时/完整着色器键/独占函数成本。
- 采集包数据优先可承受开销，但仍有界/后台I/O和自保护；最终真实性能验收另做同场景A/B。采集包交付不关闭双端最终目标，参见 `dev-docs/PC-CHAPTER-COLLECTOR.md`。

## PNG纯资源处理维护

- `src/image_premultiply.rs`保持原f32中间alpha舍入；opaque/zero快路径必须逐字节等价。worker仅借用不重叠RGBA切片，不跨线程传Image、STB全局、GL或Environment。线程创建失败只处理未被取走的块，不能重算整图使半透明像素二次变暗。
- 组件基准/5项等价恢复测试见 `dev-docs/PNG-RESOURCE-PERFORMANCE.md` 和 `tools/tests/PngPremultiplyBench.rs`；加载分量收益不能标成实战FPS或门电梯被动卡顿已修复。已交付采集包仍按自身implementation commit/map分析，不能更新旧ZIP。

## PC屏幕原生尺寸维护

- 用户明确规则：物理屏幕1080P→4×、1440P→6×、2160P→8×；横屏游戏底分480×320，其他高度向上取偶数倍率。首次预置物理屏幕后保存，后续启动读取配置（仅显式自动模式重新匹配），基础区显示内部与输出尺寸，内部分辨率移出高级画质区，质量档和硬件画质建议均不得改动倍率。默认PC纹理/MSAA/AF都是4×，旧显式配置保留；见dev-docs/PC-RENDER-RESOLUTION.md。此条替代e2093b05的最小整数算法。

## PC Windows JIT fastmem维护

- 当前候选为显式TOUCHHLE_DS_CPU_FASTMEM=1，默认关闭；原页表/兼容回调保留。空地址区须按原先MEM_RESERVE或已提交READWRITE分别保护/恢复，先销毁JIT，再恢复页面。懒提交VEH只处理MEM_RESERVE，不能接管已提交页面的保护异常。真实空地址load/store/load与恢复回归必须通过。
- Windows Dynarmic异常回调需32字节shadow space加8字节对齐；PC专项子模块pin与原因在guard契约的platform_submodule_overrides，Android仍沿原pin。禁止为消除失败自动采样新hash或跨平台整树覆盖。见dev-docs/PC-JIT-FASTMEM.md；组件收益不等于整机FPS，实际候选要正常完整构建及同场景A/B。

## PC GL hotpath候选维护

- `TOUCHHLE_DS_GL_STACK_DRIVER=1`、`TOUCHHLE_DS_GL_ENV_CACHE=1`经两组原生ABBA/64发射击/完整被抓QTE死亡回归后，由Windows x64 DeadSpace profile默认启用；显式0保留，其他app/平台的backend默认仍关闭。保留原boxed/live-env对照；借用句柄不跨线程、不改变GL上下文所有权。配置缓存必须在游戏app默认值/选项应用后、Environment创建前初始化，app picker提前绘制仍读实时环境；Windows环境变量名大小写兼容。动态文件触发器/guest与GL状态/原生输入不缓存。
- 不应为未启用的texcoord诊断扫描或MapBuffer读取索引；矩阵调色板蒙皮、GL_FIXED转换所需范围计算仍保留。完整说明与回归见`dev-docs/PC-GL-DRIVER-HANDLE.md`。后续变更仍须同二进制高画质开关A/B和完整被抓/QTE/死亡阶段。当前12%左右收益只属所测场景，最大帧未稳定改善，不能宣称门电梯被动卡顿、全章性能或结构性单核瓶颈已解决；JIT fastmem仍默认关闭。

- 跨平台默认值须独立检查外层cfg；不能把Windows分支嵌入Android-only块。配置默认化后的正常release必须在不传该开关时读回实际effective标记。306836fa的错误被原生默认值测试捕获，保留失败，不把成功编译/源码存在当启用证据。

## PC可选渲染分段测量

- `TOUCHHLE_DS_GL_PROFILE=1`仅测量候选、默认关闭。DrawElements/DrawArrays包括嵌套IndexRange/Skin/SkinSubmit的主线程elapsed wall span，不能相加冒充独占CPU/GPU或着色器编译时间。Skin的query/map、math+输出分配、post/unmap分别计时。格式键最多16种并显式计溢出，约1Hz记录，不读像素/guest内容、不新增GL调用、不跨线程传Environment。最后不足1秒的tail在终止时可能无分段报告；原生帧journal仍保留。见dev-docs/PC-GL-PROFILING.md。
- v5后的固定举枪fastmem ABBA没有整机收益（FPS约-0.77%、CPU/frame约+2.44%），继续默认关闭。不得把组件12%收益当成实战优化，也不得改写已交付v5来更新实验计时。

- `TOUCHHLE_DS_GUEST_SKIN_PROBE=1`还需显式trigger文件，默认关闭。完整指纹通过后仅重放原ARM首条PUSH，原函数计算不替换，最多64次/入口后恢复。生产JIT必须比较ARM/Thumb返回的GPR/CPSR/栈。未知代码不覆盖；SP/PC已消费后不得返回unhandled走旧fake-return。不得仅凭两个已知函数地址就宣称实际热点，先检查原生调用证据。见dev-docs/PC-GUEST-SKIN-PROBE.md。

## PC SIMD 蒙皮候选维护

- `TOUCHHLE_DS_PC_SKIN_SHADOW=1`继续用于纯影子比较；`TOUCHHLE_DS_PC_SKIN_MATH=host`是独立opt-in，默认关闭。host必须先做至少64次真实原ARM逐字节/完整FPSCR对照，之后才允许输出，并每256次重新执行原函数复核；不支持布局、别名、非有限值和任何差异都立即回退/停用。`perf skin-host on/off`仅用于隔离A/B，会完整移除或恢复入口断点。不能凭组件或单一场景宣布全章FPS收益。DAZ可能影响浮点比较，输入分类用bits；未知代码不覆盖，与入口频率探针互斥。纯CPU数据不跨线程传guest/GL/Environment。见`dev-docs/PC-SKIN-MATH.md`。

## PC JIT 热点归因维护

- PC蒙皮输入`TOUCHHLE_DS_PC_SKIN_INPUT=strided`为独立opt-in，默认packed且host默认关闭。有界只读步幅视图只在所属线程同步使用，借用必须在原ARM执行/输出提交前结束；输出独立，原别名/边界/普通浮点检查保留。`perf skin-input packed/strided`每次请求重置64次真实原版校验，每256次实际host输出复核；模式切换不能绕过差异后进程内永久停用。原版ARM/步幅/对齐/完整FPSCR、实时匹配和无观察器AB分别证明；不把省分配当FPS。`accelerated`从该实现起仅计真实host输出，旧报告语义保留。见`dev-docs/PC-SKIN-MATH.md`。

- `TOUCHHLE_DS_JIT_MAP` 仅在首个 CPU 创建前显式指向新文件时启用，默认关闭；16MiB 上限、已有文件拒绝覆盖、QPC/缓存 epoch/地址重用和 I/O 停用均保留。独立 PSS 观察器只采样本控制器拥有的 PID+创建时间+exe，限时并核验快照释放。saved-IP 计数含等待/抢占和观察开销，不是独占 CPU 占比；缓存转换/未知映射不强行归因，采样运行不能认证 FPS。见 `dev-docs/PC-JIT-PROFILING.md`。

- `TOUCHHLE_DS_PC_TRANSFORM_MATH=shadow/host` 是独立、默认关闭的矩阵实验，另需显式 trigger。108字节完整函数指纹及首条 `ADD r2,r0,#16` 必须匹配，不能套用 PUSH 重放。64次真实原版逐字节/完整FPSCR比较后才可输出，每256次实际host调用复核；不支持则原版，差异后进程内永久停用。保留对象头、合法别名、所有GPR/FPR副作用和返回状态；组件、实时影子和无观察器A/B分别报告。见 `dev-docs/PC-TRANSFORM-MATH.md`。

## 长时间原生战斗覆盖

- 用户要求锁血、自动QTE与持续攻击辅助，默认关闭，仅在全新私有存档副本中通过marker+inbox5秒租约启用；超时释放输入和血量干预。不得写原IPA/用户进度/已交付包。不能把短开局或死亡样本算后期敌人/Boss覆盖；至少记录较长无尽模式的原生计时、敌人类型/数量、峰值和长帧，按实际出现确认。辅助请求计数不等于命中/击杀/QTE成功；锁血改变工作负载，报告须标记。剧情导航/机关另外验收。具体入口、边界和回归见`dev-docs/PC-TEST-ASSIST.md`。

## 通用问题热键与诊断真实性

- F7是所有问题的自动编号标记，QTE仅是上下文之一。每会话issue编号和前后窗口文件后台写入，成功提示必须在实际文件写入后；不可把按键排队当保存成功。用户最新澄清F7使用一次诊断dump的大停顿并自动继续，不使用原生暂停/手动恢复。显式按键触发单帧读回，后台PNG编码/写入，短暂有界等待实际保存；编号关联截图与诊断。dump/读回/等待耗时单列，不算游戏卡顿。不得做周期截图/扫屏，也不得改原IPA/进度。导出包含全部issue JSON。启动器“正在记录”需要有效帧/事件实际落盘计数和新鲜健康状态；仅配置开关或文件夹存在不算。未知测试玩家包版本不能由本机结果代替。见`dev-docs/PC-PROBLEM-MARKS.md`。
