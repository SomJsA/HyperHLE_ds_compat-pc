# 历史工作台预设说明（2026-11-02）

本目录的 `default-style.json` 是历史设计预设，不是当前发布默认。
正式默认来自仓库 `src/ds_overlay_default.json`，Android asset须与之匹配。
不要将本目录预设直接部署来“恢复默认”；当前大小以PC运行时为准。
部署脚本含历史设备配置，使用前须按当前交接核对serial/package，禁止盲跑。

---

# 图文层工作台

用真实游戏截图调整章节、任务追踪和对白字幕。支持全局样式、按游戏文本 ID 单条特调、JSON 导入导出和应用到双端。原始文本共 995 条，尚未进行翻译审校。

本项目当前交付布局为 `DeadSpaceLegacy/repo/HyperHLE_ds_compat`。运行 `Deploy-Local.ps1 -Open`，将源码部署到项目的 `tools/overlay-studio` 并打开 <http://127.0.0.1:8768/>。既有 PC 启动器读取这个部署目录里的 `pc-style.json`；部署保留现有配置。正在运行的 Python 服务不会自动重启，源码更新后须自行退出旧服务再打开。

部署目录内也可以运行 `Start-Studio.ps1`。Python 3 标准库即可启动服务，不需要安装前端依赖。桥接仅监听 loopback；平板序列号、ADB 和测试包名见 `server.py`。它只截屏和写样式，不操作游戏和存档。

默认背景是开场过后的角色站稳截图，历史色彩仅作构图参考。图片与 Noto Sans SC 字体供本地验收使用，字体授权见 `assets/LICENSE.noto`。

“全局默认”修改图层样式；“单条特调”按原始 ID 覆盖位置、样式和文案。预览中的全局样本文案不会替换游戏内容。总开关保留在 PC 与安卓启动器，详细字幕排版在此调整。导入游戏后约 750ms 读取，换行和最终位置仍应实机核对。

校验：`node --check studio.mjs`、`node --check layout.mjs`、`python -m py_compile server.py`。已检查单条继承、参数边界、保存/清除、双端写入以及构建加载日志。
