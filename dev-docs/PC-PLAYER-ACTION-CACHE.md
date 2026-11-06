# PC 原生举枪/开火查找缓存

普通 PC 举枪/开火原先每次扫描所有已用 guest 内存，查找 InputForwarderTapToAction；同时无条件执行前后两组原生仪表 getter。现在每个 Environment 仅缓存当前 HUD 玩家 action6 的 forwarder 地址，每次使用仍重新核验当前玩家、对象 vptr、action code、enable 字节和存活分配范围，随后调用原 native SendAction。玩家更换、字段失配或分配已释放即失效；未找到的结果不缓存。内嵌对象保守检查原扫描同一 used ranges。没有新增 guest 状态写入或 GL 查询。

泛用 E/世界交互继续使用原查找，距离、朝向、战斗和模态门槛没有绕过。缓存只减少举枪/开火查找，不声称修复用户报告的“未按 E，靠近或提示出现即卡顿”。不得把 code6 的缓存扩展为任意世界对象缓存而省略寿命和 native enable 核验。

`TOUCHHLE_DS_PLAYER_ACTION_CACHE=0` 可显式关闭缓存用于同二进制 A/B，默认开启。`TOUCHHLE_DS_ACTION_GAUGES=1` 才执行 dispatch 前后昂贵原生 getter，默认关闭；显式控制 `probe` 仍可读取仪表。仅移除自动 getter 的试验未证明速度收益，不能宣称该改动单独提速。

2026-11-03 的私有静态 Plasma 场景使用同一 native SHA eaa97e036e8cff965756a7883ca68a554ef18cda60efb857fcf8520ff1f8cd09，4K 输出、8x native、8x 纹理、MSAA4/AF4、无上限。ON/OFF/ON 三轮每轮 8 发、间隔 1.2 秒，实际 clip20→12、ammo110→102。开枪帧 present 中位数为51.132 /74.103 /52.008ms，缓存两轮均8次命中、0次扫描；关闭轮8次扫描，中位11.6465ms。保留首发、次序和完整原生帧日志，截图/仪表读取均在测量窗外。这是约30%的开枪帧组件改善，不是全局帧率提升或稳定60FPS；开枪 native sender 本身仍约38ms。

验证包括199项相关 Rust 测试通过、1项忽略；另以真实 allocator 核验释放后残留字节、内嵌对象与跨界/溢出。原生 Bench 测试沿正常 E 门槛进入19→20，模态内两次开火不耗弹，退出20→21→0后无延迟幽灵开火，再次正常开火clip20→19。该测试没有购买或替代完整换甲动画验收。每次测试原21文件夹具不变。

证据入口：项目 `report/pc-action-cache-performance-20261103/STATUS.json`。这份文档不更新已交付 ZIP；新的正式发行仍须记录确切源码、native、资源、构建与实际验收。
