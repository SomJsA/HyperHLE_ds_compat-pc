# VM 中的逐步动作捕捉通道

2026-10-26。用户确认要继续捕捉快速转身、无重力跳跃、五把武器主副模式、近战/踩踏、背包与菜单等动作，并要求不占用本机前台或键鼠。

## 作用与边界

`TOUCHHLE_DS_CONTROL_INBOX` 指定一个诊断命令文件。默认不设置时不读文件，也不改变正常输入。
运行时至多每200ms检查一次，每次只接收一个命令，并通过既有触摸/动作请求/HUD路径处理。
不发送宿主OS键鼠，不执行shell，不写游戏内存来伪造成功结果。

它是测试通道，不是新的玩家键位方案。`[DScontrol] sequence=N queued ...`只证明请求已入队，
实际动作仍须看 `[DSgauge]`、`[DSpad]`、`[DShud]` 和画面；门控拒绝不是动作成功。

## 格式

```text
<递增正整数序号> <命令>
```

仅允许一行，最多1024字节。重复或过期序号不执行；启动时文件里已有的命令会被忽略，防止旧攻击请求重放。
必须等 `[DScontrol] ready` 后发布新序号。它是单条命令信箱，不是队列；每发一条须先等待日志确认，再发下一条。

| 命令 | 含义 |
| --- | --- |
| `tap x y` / `down x y` / `move x y` / `up x y` | 既有guest触摸通道，坐标为横屏480×320点 |
| `swipe x y x2 y2` | 既有8步划动脚本 |
| `action interact` / `kinesis` / `stasis` / `reload` / `rotate` / `combat` | E/F/C/R/X及手柄上下文近战/踩踏请求 |
| `aim on` / `aim off` | 原生瞄准请求 |
| `fire primary` / `fire secondary` | 原生武器模式与攻击请求，仍受游戏瞄准等门控 |
| `ui up` / `ui down` / `ui left` / `ui right` | 向既有前端导航通道注入一次方向事件，不发送宿主键鼠 |
| `ui confirm` / `ui cancel` | 向既有前端确认/取消通道注入一次事件；不绕过菜单门控 |
| `release` | 既有持续攻击释放路径 |
| `cancel` | 清除待发脚本/输入并释放已送达触点与持续动作 |
| `hud 1`..`hud 5` / `hud P` / `hud G` | 已有原生HUD候选；只有显式启用 `TOUCHHLE_DS_NATIVE_HUD=1` 才入队 |
| `mark` | 请求一张原生present诊断帧 |
| `probe` | 请求现有player gauges及诊断帧 |
| `checkpoint` | 记录序号和当前frame，不截图 |

坐标拒绝NaN、无限、负值或超出480×320的值；拒绝任意动作码和多行脚本。
不会因这条通道自动开放原生切枪/暂停/定位，也没有新增快速转身或跳跃的默认按键。

## VM 发布

`tools/pc-runtime/vm/Send-ControlCommand.ps1`只在指定的VM计算机名上执行，默认`DSH-WINBOX`。
它将单条命令写入同目录临时文件后替换信箱，并最多等待5秒查找对应日志确认。
超时须重新观察，不自动重放。只在guest运行示例：

```powershell
& C:/dsh/pc-action-capture-20261026/Send-ControlCommand.ps1 -Command 'checkpoint'
& C:/dsh/pc-action-capture-20261026/Send-ControlCommand.ps1 -Command 'tap 80 286'
& C:/dsh/pc-action-capture-20261026/Send-ControlCommand.ps1 -Command 'probe'
& C:/dsh/pc-action-capture-20261026/Send-ControlCommand.ps1 -Command 'cancel'
```

方向/按钮坐标必须来自刚观察到的画面，不能把菜单动画过渡帧当作稳定状态盲点。
持续输入后应显式release/cancel；测试结束停止仅本次候选进程。

## 免打扰自动化验收规范

后续 PC/Android 自动化验收默认遵守以下约束，除非用户明确要求外部工具兼容性测试：

- 不捕获宿主鼠标/键盘，不强制把游戏窗口切到前台。
- 不使用 Computer Use 作为常规测试驱动；优先使用 `TOUCHHLE_DS_CONTROL_INBOX`。
- 不使用 OBS/PixPin 作为功能判断依据；画面证据优先使用游戏内 frame dump 或
  `TOUCHHLE_DS_PRESENT_CAPTURE`，性能证据使用 super-monitor JSONL。
- 每条命令必须递增序号并等待 `[DScontrol] sequence=... queued`；超时只重新观察，不自动重放。
- `[DScontrol] queued` 只证明请求进入仿真器，不证明游戏动作成功；必须同时检查
  `[DSfrontendInput]`、`[DSfrontendCards]`、HUD 状态、最终帧和可回退日志。
- 菜单测试优先用 `ui` 命令；触摸坐标只在需要验证真实触摸命中时使用。
- 外部窗口/全屏捕获问题单独标记为“外部捕获兼容性”，不能与游戏输入或内部呈现失败混为一类。

## 离线与真实代码证据

- 新信箱解析器3项测试：去重/过期、坏批次与坐标拒绝、有限语义白名单。
- 真实文件运输检查：启动旧命令不重放、新序号恰执行一次、cancel可送达、默认关闭。
- Windows cargo check通过；整包release及VM实际接线结果见外层报告。
- 原始ARM `CanQuickTurn` 已在WSL Unicorn验证24组条件；`OnEvent` 的jump/action8共17组验证通过。
- 原始`StateTransition(2)`会把当前前向量取反，写入player+0x1f0..1f8；结合Action8空闲分支的
  `0x1000`标志，支持它是转身准备路径。完整动画/更新仍未运行验收。
- Action `0x0c`只在玩家空闲且`IsInZeroGEnvironment`为真时调用`TryJump2(true)`。
  测试截获了外部入口，不代表实际起跳/落地完成。

旧文将Action8仅称为Abort不完整；它还包含空闲时进入state2的路径。
这些发现不推翻已验收的E/F/QTE，也不能替代第二章真实切枪流程。

## VM 存储

旧Windows VM启动时磁盘空间紧张，本轮曾因连续PPM及新纹理缓存耗尽剩余空间。
已停止本轮候选并清理**仅本轮隔离目录**的PPM/缓存和传输包，保存清单及小图。
本轮候选现仅按mark/probe留帧；测试目录的`touchHLE_pvrtc_cache`用普通文件占位，令该实例
安全回退内存解码，避免继续填盘。其他运行目录与缓存未改。
该VM软件OpenGL性能不是宿主Windows/GPU性能的验收结果。
