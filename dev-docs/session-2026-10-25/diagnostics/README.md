# 原始诊断脚本快照

本目录保留本轮离线取证脚本原文，不自动改写其绝对路径。原运行位置、命令与状态见 ../TAKEOVER.md。
inspect-head.py 依赖外层 tools/dsstate.py、tools/arm-dis.py、原始 ARM 二进制；基础解析器副本也已收录。
head-cleanup-targets.py 的地址来自本轮三份快照，不能直接套用新进程。arm-dis.py 需要 Python capstone。
startup-smoke.py 会启动并终止它自己创建的独立无窗口子进程；它不是用户游戏控制脚本。
patch-test-wrapper.rs 引用本机源码路径。不要将这些诊断脚本当作即时存档恢复工具。
