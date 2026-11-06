# Dead Space 存档结构实验室

Python 3.10+，只用标准库。在独立分支里解析和编辑 SBIN v2，输出新文件；另提供整组
`Documents` 的存档库、冷切换和备份恢复。本目录仍是命令行研究后端；正式启动器现有图形页面：
PC可编辑已有资源/RIG/武器等字段，Android可改信用点/节点/护甲。两端已有一个ZIP管理三槽及共享依赖、
自动备份与恢复。Android只同步用户改过的标量到已有checkpoint/level字段。
复跑入口见 `tools/project-guard/README.md`；图形功能不代表DLC或未知结构可任意合成。

## 快速开始

在仓库根目录执行，路径替换为自己的**实验副本**：

```powershell
python tools/save-lab/save_lab.py verify C:/SaveTest/00_CheckpointSave.sb
python tools/save-lab/save_lab.py inspect C:/SaveTest/00_CheckpointSave.sb --out C:/SaveTest/structure.json
python tools/save-lab/save_lab.py edit C:/SaveTest/00_CheckpointSave.sb C:/SaveTest/changes.json --out C:/SaveTest/edited.sb
python tools/save-lab/save_lab.py diff C:/SaveTest/00_CheckpointSave.sb C:/SaveTest/edited.sb
```

`changes.json` 示例（JSON 数字保持数字，不加引号）：

```json
{
  "player/inventory/credits": 95275,
  "player/inventory/nodes": 21
}
```

所有 SBIN 编辑命令必须指定新的 `--out` 文件；已存在的输出和原档都会被拒绝覆盖。
`inspect` 输出包含路径、存储类型、DATA 偏移、值、容器摘要和原始 SHA256。
`diff` 忽略布局偏移，只比较字段类型和值。字段名中的 `/`、`~` 按 JSON Pointer 转义。

## 模块导出、移植、插入与移除

```powershell
# 导出整个玩家模块，包含生命、资源、武器、升级及背包
python tools/save-lab/save_lab.py export C:/SaveTest/donor.sb player --out C:/SaveTest/player.module.json
python tools/save-lab/save_lab.py import C:/SaveTest/target.sb player C:/SaveTest/player.module.json --out C:/SaveTest/player-transplanted.sb

# 导出一个已有模块，移除后可以重新插入；不会处理付费交易缓存
python tools/save-lab/save_lab.py export C:/SaveTest/settings.sb dlc_modules/attack --out C:/SaveTest/attack.module.json
python tools/save-lab/save_lab.py detach C:/SaveTest/settings.sb dlc_modules/attack --out C:/SaveTest/detached.sb
python tools/save-lab/save_lab.py attach C:/SaveTest/detached.sb dlc_modules attack C:/SaveTest/attack.module.json --out C:/SaveTest/reattached.sb
```

`import` 替换已存在的同类型字段；`attach` 向对象添加尚不存在的命名字段；`detach` 移除命名字段。
模块 JSON 包含类型化 root 和局部对象图，使用名称而非来源 STRS 数字 ID。移植时重新分配 OHDR
引用，按名称合并 STRS；原字符串 ID 与对象 ID 保持有效。引用共享在所移植的图内保留，
目标图外的别名不会跟着修改。未知标量/列表结构、循环引用和类型不匹配会拒绝。

引用对象采用追加 DATA/OHDR 的方式重建，原对象字节保留为不可达历史；反复结构编辑会增加文件
大小，上限 16 MiB / 65,535 个对象。暂未实现对象垃圾回收。标量编辑保持字段宽度，只有新增
字符串时会扩展 STRS；布尔编辑只改有效低字节，保留填充。

## 背包和字符串列表扩容

```powershell
python tools/save-lab/save_lab.py list-replace C:/SaveTest/input.sb player/inventory/items C:/SaveTest/items.json --out C:/SaveTest/resized.sb
```

`items.json` 是**完整替换列表**，不是增量补丁：

```json
[
  {"type": "AMMO_PLASMA_CUTTER", "count": 10},
  {"type": "PENG", "count": 1}
]
```

字符串 ID 列表同样可替换，例如检查点 `dlc_modules` 的 JSON 是 `["attack", "defense"]`。
它与 settings 中的 `dlc_modules` 对象是不同结构。
列表可以增减到零；游戏对容量、物品名和资源数值的实际约束仍要单独验证。

## 整组冷切换与恢复（PC）

```powershell
# 保存整个 Documents，包含 var 外依赖、Core.sb、未知普通文件和空目录
python tools/save-lab/save_lab.py profile-snapshot C:/SaveTest/ProfileA/Documents C:/SaveTest/Bank chapter2
python tools/save-lab/save_lab.py profile-verify C:/SaveTest/Bank/chapter2

# 游戏完全退出后，切换整个 Documents；先把当前状态保存到 Backups/pre-...
python tools/save-lab/save_lab.py profile-activate C:/SaveTest/Bank/chapter2 C:/SaveTest/Runtime/Documents --backups C:/SaveTest/Backups

# 恢复：将上一步返回的 backup 路径作为 entry，仍先备份当前状态
python tools/save-lab/save_lab.py profile-activate C:/SaveTest/Backups/pre-实际编号 C:/SaveTest/Runtime/Documents --backups C:/SaveTest/RestoreBackups
```

快照包含 SHA256/长度清单，验证全部成员、SBIN 和额外/缺失文件。快照同名不能覆盖。
切换先在同目录暂存，再备份当前树、重新检查源内容和进程状态，最后用目录重命名发布；
切换后的旧残留文件不会混入新档。捕获到的发布/校验异常会还原原树，失败的新树保留为
`.save-lab-failed-*` 供检查。

约束：

- 检测到本机 touchHLE 进程或无法查询进程时拒绝快照/激活。启动器尚未共享此锁，操作期间
  仍须保持游戏退出。此检查不代表 Android/VM 上的游戏已退出；ADB 适配器见下节。
- 拒绝链接、目录联接、路径重叠、清单路径越界、大小写冲突；限定 256 MiB / 10,000 条目。
- 目标必须明确叫 `Documents`，切换以整组文件为单位，槽位与共享设置不会暗中混用。
- 两次目录重命名不是断电原子事务。强制终止/断电可能留下 `.save-lab-Documents.lock`、
  `.save-lab-stage-*` 或 `.save-lab-rollback-*`。恢复前确认没有工具/游戏进程：优先从已验证的
  `Backups/pre-*` 恢复；若 `Documents` 缺失，可将同目录 rollback 改回 `Documents`。
  保留这些目录作证据，确认原档已恢复后再处理遗留锁。自动崩溃恢复日志尚未实现。

## 格式证据与测试

根对象是 `OHDR[0]`，不是 DATA 开头；字段偏移相对于所属对象；校验为 **FNV-1**，
不是 FNV-1a。支持存储类型 `05/06`（u32 位模式）、`09`（布尔）、`0A`（float32）、
`0D`（字符串 ID）、`0F`（OHDR 引用），以及背包已知 `10` 记录列表。
静态类型结构和运行时语义尚未完全解出，遇到未知编码会停止，不能声称支持任意 SBIN。

```powershell
python -m unittest discover -s tools/save-lab -p test_save_lab.py -v
python tools/save-lab/verify_fixtures.py C:/SaveTest/Originals --out C:/SaveTest/report.json
```

测试使用合成文件和用户本地真实副本，不将游戏、账户缓存或存档二进制提交到 Git。
真实文件报告见 `dev-docs/session-2026-10-31/SAVE-LAB-*-VERIFICATION.json`；
游戏加载结果与未完成项见同目录的 `SAVE-STRUCTURE-BRANCH.md`。

## Android ADB 冷插拔与剧情锚点

独立包为 `org.touchhle.android.savelab`，显示名称“死亡空间 · 存档实验室”。安装时不替换
其他包。实验档位于该包内部 `files/save-lab/touchHLE_sandbox/.../Documents`，ADB 以
`run-as` 保持应用 UID 写入，兼容本次平板 Android 14 的目录权限。当前适配器面向 Android
用户 0 和可调试实验包；不承诺所有 ROM、其他用户和生产包可写。

```powershell
python tools/save-lab/save_lab.py adb-pull --adb C:/Android/platform-tools/adb.exe --serial 设备序列号 C:/SaveTest/Bank tablet-copy
python tools/save-lab/save_lab.py adb-activate --adb C:/Android/platform-tools/adb.exe --serial 设备序列号 C:/SaveTest/Bank/edited --backups C:/SaveTest/AndroidBackups
```

`adb-activate` 只允许写这个精确包名，会先停止该包、保留本机整组备份和设备上的 rollback
目录，再上传、核对、切换、复核。启动器和导入器尊重 ADB 操作锁。读取其他包仅用于双次
一致性校验后的只读备份，不会停止或写入它们。传输失败时保留回滚/暂存树，不能保证断网或
断电后自动恢复；可将本机已验证备份重新激活。

剧情位置包含关卡、演员、任务、触发器、计时状态和角色变换。可导出一组剧情锚点，并移植到
兼容档中，保留目标玩家的武器、资源与升级：

```powershell
python tools/save-lab/save_lab.py progress-export C:/SaveTest/donor.sb --out C:/SaveTest/chapter-point.json
python tools/save-lab/save_lab.py progress-import C:/SaveTest/target.sb C:/SaveTest/chapter-point.json --out C:/SaveTest/story-edited.sb
python tools/save-lab/save_lab.py progress-transplant C:/SaveTest/target.sb C:/SaveTest/donor.sb --out C:/SaveTest/transplanted.sb

# 以完整配置为输入，配套复制同槽三个文件，输出新的配置，不动共享设置和成就
python tools/save-lab/save_lab.py slot-copy C:/SaveTest/Bank/target C:/SaveTest/Bank/donor C:/SaveTest/Bank new-slot --target-slot 0 --donor-slot 2
# 只移植配套 Checkpoint + Level 的剧情锚点，保留目标玩家装备
python tools/save-lab/save_lab.py slot-copy C:/SaveTest/Bank/target C:/SaveTest/Bank/donor C:/SaveTest/Bank new-story --target-slot 0 --donor-slot 2 --story-only
```

单文件命令不会暗中同步 Checkpoint/Level；`slot-copy --story-only` 才负责配套处理。
锚点包含 `current_level/actors/objectives/triggers/misc/player/transform`，其中难度和周目在
`misc` 内，也会随剧情锚点移植。当前仅证明这些组合可加载到截图中的场景，未验证完整后续剧情。
不要将“仅改 level_id”或“仅移动 XYZ”当成剧情已完成迁移。

Android 原生 ZIP 导入新增 SBIN v2 区段校验、重复路径拒绝、独立操作标识和导入互斥；
保留 `last_version.txt`、`loggedIn`、`mtxprods`、`.dat1fda.006` 等已知依赖。未知非 SBIN
文件仍被旧式 ZIP 导入过滤；任意普通依赖和空目录的完整保留请使用 ADB 整组快照。

实验与拼图工具：`prepare_story_cases.py`、`android_story_runner.py`、`make_story_montage.py`。
原始 PNG 留在本地证据目录；标注拼图使用小于 400 KB、最大边 1568 px 的 JPEG。

## 可选一周目节点采集（代码准备，默认关闭）

Android 启动器已接入“自动采集存档节点”主开关，默认关闭、下次启动生效。开启后观察实际
SBIN 写入，归档有效文件版本和稳定后的整组 Documents；支持轮次、暂停菜单手动标签、
可选画面和退出后的 ZIP 导出。本轮只做代码与离线验证，没有安装或开启设备采集。

导出库解压后：

```powershell
python tools/save-lab/save_lab.py node-bank-inspect C:/SaveTest/save-node-bank --out C:/SaveTest/node-index.json
```

索引包含节点、原始版本和异常流水；各 `nodes/<id>` 可以作为已有冷恢复入口。
文件观察与轮询不保证捕获任意短间隔覆盖，也不能生成游戏未曾落盘的任意位置。连续试玩前
仍需短程实机覆盖率和性能验收。详见 `dev-docs/session-2026-10-31/SAVE-NODE-MONITOR.md`。
