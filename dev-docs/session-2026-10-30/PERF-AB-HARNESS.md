# PC Performance A/B Harness

Date: 2026-10-30

## Scope

This procedure measures host-side presentation cost. It does not replace visual
acceptance and does not combine VM/llvmpipe, Android and real-PC results.

## Baseline

2026-11-01: the quiet runner now defaults to `--vsync=off`, independently of
`--fps-limit`. Check `[DSvsync] swap interval 0` in the startup log before treating
an arm as uncapped. `--vsync=driver-default` deliberately leaves the driver
policy unspecified and records that choice. Earlier runs without an explicit
swap-interval request are not an uncapped performance certification.

Windows inbox publication tolerates a transient sharing/antivirus lock for at
most two seconds, while the unpublished temporary snapshot still exists. The
sequence number and payload stay fixed during those failed rename attempts.
Successful/uncertain publication and acknowledgement timeouts never replay the
input. The offline transport regressions exercise these distinct failure cases.

- Same binary, IPA, save, scene, camera, viewport and window mode for every arm.
- Current checkpoint PC binary SHA-256:
  `CE3926ED3E4AEE2E0A2B36B3F7A82CCFC21A759EAB7AD99D98F46E3F8AC6B863`.
- Start with no HD texture pack.
- `scale=4`, `MSAA=2`, `ANISO=4`, `FSR=2`, fast present.
- Warm up 20 seconds, then sample 60 seconds.
- Run three repetitions with baseline/treatment interleaved.
- Any scene transition, save load or window-mode change invalidates the sample.

## Arm Matrix

| Group | Change from baseline |
| --- | --- |
| TEX | `TOUCHHLE_DS_TEX_UPLOAD_FINISH=1`, `flush`, `0` |
| GPU copy | `TOUCHHLE_DS_GPU_PRESENT_COPY=0/1` with readback finish on/off |
| Present | fast present versus CPU composition |
| Scale | internal `4x` versus `2x` |
| MSAA | `2` versus `4` samples |
| FSR | off, EASU, EASU+RCAS |
| Overlay | no host text versus chapter/subtitle/objective text |

Do not combine GPU overlay, GPU present copy and disabled texture synchronization
in one arm. Each must have an independent rollback path.

## Required Output

Keep per arm and repetition:

- `super-monitor-*.jsonl`
- stdout/stderr logs
- manifest with binary/IPA/save hashes, HEAD, dirty state, GPU/driver, scene and
  complete environment
- before/after frame captures
- generated `summary.json` and `summary.md`

## Summarizer

```powershell
python tools/pc-runtime/Summarize-PerfRun.py `
  E:\CodexProjects\DeadSpaceLegacy\logs\perf-ab\TEX1\rep1 `
  --out-dir E:\CodexProjects\DeadSpaceLegacy\logs\perf-ab\TEX1\rep1
```

`Start-QuietPerfRun.py` runs one hidden process through the control inbox. For
load-sensitive environments, use its paired mode rather than comparing separate
cold starts:

```powershell
python tools/pc-runtime/Start-QuietPerfRun.py `
  --runtime-dir <runtime> --seed-sandbox <seed> --out-dir <output> `
  --ipa <game.ipa> --run-id paired-gpu `
  --paired-mode gpu-copy --paired-seconds 20 --paired-cycles 3

# The runner generates paired-summary.json/md automatically.
# For an offline reanalysis:
python tools/pc-runtime/Summarize-PairedRun.py <output>
```

Paired mode sends only `perf gpu-copy on|off` or `perf tex-sync
finish|flush|off` through the control inbox. It does not inject host keys and
does not force the window to the foreground.

Whole-run FPS and frame mean are calculated from frame-count delta over
wall-clock time. The monitor's p95/p99 values are rolling-window snapshots, so
the summary reports medians/maxima of those snapshots rather than claiming raw
whole-run percentiles.

## Validity

- `GPU_PRESENT_COPY=1` is valid only if the startup log confirms the GPU copy
  path and frame captures show no black, stale or duplicated frames.
- `TEX_UPLOAD_FINISH=0|flush` is diagnostic until texture-load transitions and
  level loads show no black atlases or stale textures.
- FSR is invalid as an A/B arm when the source area is at least 115% of the
  viewport, because the automatic gate intentionally disables it.
- Snapshot text overlays force CPU composition. Fast-present measurements must
  record whether any host text layer was visible.


## 2026-10-30：隔离存档根目录修复（R5）

TOUCHHLE_USER_DATA_DIR 是数据基目录，运行时还会追加 touchHLE_sandbox/包名。此前 quiet runner 将 sandbox 本身作为基目录，实际生成了 touchHLE_sandbox/touchHLE_sandbox，指定 seed 未被使用，落入新档难度页。R5 改传 sandbox.parent，记录基目录和活动00存档SHA，并按实际原生菜单导航、处理读档提示页。启动器原本使用 SelectedDataRoot，未受影响。

旧隔离结果的 seed_sandbox 元数据不能证明加载了该档；依赖具体章节/任务/加载场景的结论须重跑。同进程 paired 时间窗仍是同一真实进程的对照，但只能归属于日志/内部截图实际出现的场景。R4 的开场CG视觉验收与用户复测仍有效。

## 2026-10-31：报告有效性修复（R8）

明确指定的 A/B 时间窗不再裁掉配置变化，首尾观测须分别在请求边界的
1 秒以内；中间卡顿仍计入耗时。拒绝计数器回退、时间回退和 renderer /
画质配置变化。没有新增调用的阶段不再沿用加载时遗留的最大耗时。

Paired 报告只汇总完整且可比较的 baseline/treatment 周期；一臂无效，
该周期两臂一起退出比较。纹理同步没有实际工作时不得据此判断模式收益。
Off 绕过同步计时器，另需上传活动证据。历史 GPU copy 3 组配对仍通过，
历史纹理同步 4 周期因无同步工作被拒绝，加载态回归仍待完成，默认 Finish。

验证命令：`python tools/pc-runtime/test_perf_reports.py -q`（R10 后为 48 项离线测试）。
完整限制与防回退说明见 [PERF-REPORT-VALIDITY.md](PERF-REPORT-VALIDITY.md)。

## R9：加载态上传活动证据

纹理同步的有效配对还必须有 schema-1 独立提交计数：Finish/Flush/Off 各自记录
带数据和不带数据的调用。Off 不再依赖不存在的同步计时器证明上传；空数据分配
不算加载 payload。两臂都须有请求模式下的 payload 增量，测量窗内不能混入其他
模式；Off 的同步计时调用应为零。旧日志仍可计算 FPS，但不能补造这种活动证据。

计数只覆盖经过同步钩子的 guest 提交，不代表 GPU 完成或全部 HD/mip 上传。
测量应从触发加载之前的已知快照开始，并在加载结束后的快照之后结束，将中间
停顿计入耗时；不能在加载完成的 ACK 后才开始计时。还须证明两臂的实际加载
任务和画面可比。实现与限制见 [PERF-LOADING-ACTIVITY.md](PERF-LOADING-ACTIVITY.md)。

## R10：请求与实际提交路径分开

新日志记录 FSR 请求、冗余门控及最终 EASU 路径提交结果；普通锐化回退不会冒充
FSR。MSAA 按当前 present 接收上下文刷新。GPU copy 和 CPU/GPU 字卡还各有独立
提交计数；GPU 字卡可用并不等于强制 CPU/CA 呈现时真正选择了它。

实际画质字段在配对中须一致，只允许指定 treatment 的模式字段变化。空字幕场景
或已退回 CPU 的场景不能给出 GPU 字卡收益。计数仍不代表 GPU 完成或像素正确，
详见 [PERF-PRESENT-PATH-EVIDENCE.md](PERF-PRESENT-PATH-EVIDENCE.md)。

## R11：quiet runner 判定与文件安全

配对运行自动生成整段、各臂和完整周期报告；退出码采用 `paired-summary.json`。
整段报告保留配置变化的诊断（通常退出码 2），不能把有意切换模式误判成运行失败。
配对中没有实际提交、缺一臂或配置混杂仍会被拒绝。`report-status.json` 记录最终
判定来源及整段诊断退出码；这只判定时序可比性，不代表视觉或驱动验收。

新输出目录复制 seed 时跳过根诊断目录，完全取消递归删除。存档内部链接/junction
和输出重定向会被拒绝；内部截图覆盖/删除前核对其位于私有输出根目录且为普通文件。
只终止本次创建的进程，不触碰已有游戏/启动器。报告子进程显式采用 UTF-8，保留
独立 stdout/stderr；manifest 增加 IPA 和三个 harness 脚本哈希。

离线验证：`python tools/pc-runtime/test_quiet_runner.py -q`（13 项，Windows 无文件
symlink 权限时跳过该项；真实 junction 测试仍执行）。这些测试不会启动游戏。
此 runner 的静态场景 paired 模式不能替代加载态事务；仍需在触发加载前取得边界，
保留完整停顿并证明两臂加载任务一致。
