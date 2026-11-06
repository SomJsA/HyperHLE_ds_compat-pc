# 会话全量审计记录

日期：2026-10-25

## 范围

本轮要求全量读取指定 PC 与 Android 会话。由于单条 rollout 可达数百 MB，
且 Codex 线程读取接口会截断长工具输出，不能只依赖分页摘要。接手时使用
`tools/audit_codex_sessions.py` 流式逐行解析原始 JSONL，校验每条记录并保存
原始字节 SHA-256。

审计覆盖：

1. 用户指定的 PC 线程 `01a16eb0-d99d-7bf1-9089-e94bb62e831a`；
2. 用户指定的 Android 线程 `01a16756-9874-7a42-ba34-9a575108d2f3`；
3. Android 续会话 `01a167f7-bb99-77d0-9aee-be199cf689ed`；
4. PC 线程引用的上游会话 `01a163b3-7b47-7973-a48e-366965f442f9`。

## 统计

| 会话 | 原始字节 | JSONL 行 | 用户/助手消息 | 工具记录 |
| --- | ---: | ---: | ---: | ---: |
| PC `01a16eb0` | 21,919,299 | 2,429 | 110 | 604 |
| Android `01a16756` | 14,625,800 | 1,282 | 32 | 472 |
| Android 续会话 `01a167f7` | 68,728,541 | 15,189 | 330 | 4,830 |
| PC 上游 `01a163b3` | 272,848,702 | 50,118 | 661 | 15,675 |
| **合计** | **378,122,342** | **69,018** | **1,133** | **21,581** |

四个文件均为 `invalid_json_lines = 0`。

## 原始 rollout SHA-256

| ID | SHA-256 |
| --- | --- |
| `01a16eb0` | `ad33fac63d3b79f601d41f359b08b92cad2b3b47cbbb7aa91441775a1bae4b88` |
| `01a16756` | `ac2fceecf005f64c2603d021442c03b4fa6a6e7adf51b2328033567fe28f5742` |
| `01a167f7` | `a5c0fbbc3200bd9718b243cd27879d80f8c2833536a5d6ecba4d3e113caf7909` |
| `01a163b3` | `eaaafc2f3d31e0820390688d8e497e775b08425655c460dddaf231a21cace280` |

机器可读清单：

`E:\CodexProjects\DeadSpaceLegacy\audit\session-2026-10-25\session-audit.json`

该目录还生成完整 `.dialogue.md`、`.user-requests.md` 和
`.assistant-messages.md`。这些文件包含大量重复工具输出，且体积明显大于仓库
策略允许的文档，保留为本地审计证据，不提交 Git。

## 复现审计

```powershell
$files = @(
  'C:\Users\somjs\.codex\sessions\2026\10\25\rollout-2026-10-25T00-14-06-01a16eb0-d99d-7bf1-9089-e94bb62e831a.jsonl',
  'C:\Users\somjs\.codex\sessions\2026\10\23\rollout-2026-10-23T13-58-11-01a16756-9874-7a42-ba34-9a575108d2f3.jsonl',
  'C:\Users\somjs\.codex\sessions\2026\10\23\rollout-2026-10-23T16-54-11-01a16756-9874-7a42-ba34-9a575108d2f3_01a167f7-bb99-77d0-9aee-be199cf689ed.jsonl',
  'C:\Users\somjs\.codex\sessions\2026\10\22\rollout-2026-10-22T21-01-09-01a163b3-7b47-7973-a48e-366965f442f9.jsonl'
)
python .\tools\audit_codex_sessions.py @files `
  --output-dir E:\CodexProjects\DeadSpaceLegacy\audit\session-2026-10-25
```

## 会话提炼出的当前裁决

- 测试灯带和 E/F 均由用户实机确认修复。
- 开场局部黑场仍未解决；全帧黑屏检测为零不代表通过。
- F5 只是诊断快照，F9 不可用于宝贵现场。
- Android FSR3 仅是真实 D16 的 FSR3.1 超分探针，缺 motion/jitter/history/HUD。
- Android 帧生成 NO-GO。
- Android 推荐档是原生 3× + FXAA + CAS 0.5。
- Android 设备当前不可连接，新的实机验收暂停。
