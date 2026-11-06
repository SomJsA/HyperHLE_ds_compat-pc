# R12 经验与整合边界

本轮未提交。用户要求验收后再提交；真实 index 不暂存，候选补丁用私有 index 保存。

- 视觉高亮不是菜单选择：用原版字段和事件核对模式、档案。只有不一致才派发，防原版二次点击直接进入。
- 暂停 HUD 2/3 与选项/帮助 HUD 4/5 分别拥有 handler；原生所有者切换时释放游戏输入。
- 不按函数名字猜语义。UIAnimator 在 elapsed==duration 时 IsFinished 已真，IsAnimating 仍可能为真；原版进入 ready 阶段就停止更新。最终代码依赖原版 layout phase，而非附加错误门控。
- 不按相邻内存猜坐标。WidgetHandler +4/+8 传给 Layout::Draw 的是 width/height。ABI 中 r3 与栈上前一个 float 才是 x/y；只验证数值没验证参数位置会得到错误结论。
- OptionsPanel 控件不在根 map；须按原版 OnEvent 遍历 shared_ptr 向量。父 active rect 为空时查询真实子按钮，套原版滚动原点与裁剪。SlideFill、Minus/Plus、Selector 的继承虚表必须核对完整。
- 私有测试进程可以做只读内存快照，再在 Unicorn 副本执行 GetActiveRect。它不往活进程写内存，不注入，不操作宿主前台；比多轮猜测改代码有效。
- 单帧合成 tap 在 60 FPS 会漏过原生控件处理；专用 UI 点击需保持三帧并避开未释放的上次点击。不能只看下发日志就认定成功。
- 功能回归需验证真正页号和原生数值变化。截图只截 control inbox 内部图，模拟按键不能冒充物理手柄验收。
- 冷却保留最后请求，松开不复活旧键；长按只触发一次，暂停/失焦清空。菜单导航与游戏 Dpad 武器要分上下文。
- 翻译功能合并单独进行：读取 delivery/MAINLINE-MERGE.md 和 LESSONS-2026-10-31.md；不要 merge Snapshot 基线，不整文件覆盖本轮输入修复。

临时失败构建和证据保留供追溯，不是交付：V2/V4/V6/V7/V8。最终候选仅以 human-runtime.json 和内部回归通过记录为准。
