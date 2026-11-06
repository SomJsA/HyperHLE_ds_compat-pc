# 靠近交互物时的长帧定位

本轮用户确认：未按E，靠近或出现提示时卡顿。不能直接归因按E触发的扫描。
既有实际会话捕获6份自动dump，关键长帧约107–221ms；附近无新的HD替换提交，采集回调约0.08–0.15ms/秒。
这只缩小范围，不能证明具体阻塞点，也不能直接排除驱动/系统调度。

`TOUCHHLE_DS_STUTTER_PROBE=1` 显式启用私有定位；默认关闭。仅输出≥2ms的函数墙钟区间到既有诊断队列。
不会抓帧、查询GL错误、改变输入或写guest状态；区间可能嵌套重叠，不能相加。定位开销与正式性能样本分开。
`runtime_wall_span` values为 `[kind, elapsed_us, last_present_frame, tag]`，kind映射如下：

- 1: GLES context sync + dispatched API; tag is exact gles_guest.rs caller line
- 2: glClear native callbacks
- 3: PC native movement input relay
- 4: QTE/native action prompt observation
- 5: native station actor discovery
- 6: action forwarder refresh
- 7: host overlay prepare/style check
- 8: host text/card construction
- 9: GPU text card pass including construction
- 10: GPU text upload; tag width<<32|height

`native_prompt_kind`为原生提示类型变化：0无、1点击、2滑动、3切割、4踩踏、5继续、6返回；不记录实际按键。
已交付包和用户原始存档不可覆盖。定位用私有副本，不把未复现或缺少跨度记录写成修复通过。

Probe revision 2 adds span11 for the complete presentRenderbuffer host callback and span12 for guest vtable sweeps. Explicit probe mode also samples read-only player position float bits and native hint flags at most5Hz to verify actual movement. These fields are absent with the flag off; no framebuffer fallback or actor mutation is introduced. Navigation/captures, loading and playable windows must remain separate.

Probe revision3 closes two observation gaps: span13 surrounds an existing guest-to-host SVC dispatch (tag=guest SVC address), span14 surrounds Dynarmic run/step (tag=entry guest PC). Both use the same explicit opt-in and>=2ms threshold. Host callbacks can recursively execute guest code or wait; these wall spans are not CPU utilization and must not be summed with their children. No JIT configuration or timing budget changes.

## 本轮证据边界与回放固化

- 实际用户会话的6份严重长帧仍保留；不能由工作台未复现关闭问题。
- 工作台夹具已实际移动6轮，约70秒/6650帧，最大24.387ms，没有原生切割/踩踏提示；编译曾同时进行，不能当作FPS认证。
- 用户进度副本在瞄准/开火教程：无E的正常瞄准/开火和走动回放已完成，原16文件不变。43–111ms的5个移动计划内长帧集中在瞄准/开火开始，随后步行没有同级长帧；没有复现用户指定的“靠近新交互提示”场景。截图仅是显式私有场景证据，在测量窗外。
- HLE跨度会跨协程/guest线程等待。实测 `_sem_wait` 跨度可达61秒而游戏持续present；`_usleep` 可嵌套执行其他线程。绝不可把最长inclusive跨度直接判为阻塞当前游戏帧，不能求和。单帧必须按单调时间交集、guest CPU片段、渲染/输入所有者和对应原生事件判断。
- `Start-QuietPerfRun.py` 对观察到的原生注册page2只取消一次，再逐页核对widget；未知页拒绝触点猜测。控制inbox只有一个序号所有者，不能临时旁写高序号打断控制器。导航失败不输出有效性能结论。
- opt-in高频跨度可能缩短2048条事件环的时间覆盖。分析全程持久化timeline并报告缺口；不要承诺每份probe模式incident都包含完整30秒所有细节。默认非probe的30秒关键摘要契约不变。

新状态入口为项目报告 `report/pc-interaction-stutter-20261103/STATUS.json`。当前不是游戏逻辑修复或新发布包；下一步覆盖首次提示、原生切割/踩踏和用户明确的交互物。

Probe revision4 adds span15 for the original native action sender (including explicit gauge getters only when opted in), and span16 for player raise/fire lookup plus sender. `probe_player_action_cache` values are `[hit, last_present_frame, 0, 0]`; emitted only in explicit stutter-probe mode. These spans overlap glClear, guest-to-host dispatch and Dynarmic execution. Details and scoped component evidence: `PC-PLAYER-ACTION-CACHE.md`. A span14 entry PC often points to a return instruction inside an import stub; it does not attribute the entire execution interval to that imported function.
