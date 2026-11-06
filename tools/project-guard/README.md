# R15 项目自查

Python3.10+，标准库。默认只读本地文件，不连接ADB、不唤醒设备、不启动游戏、不生成IPA、不修复/覆盖数据。

从任一 R15 仓库运行（路径替换为当前实际工作树）：

```powershell
python tools/project-guard/guard.py --pc-root E:/CodexProjects/DeadSpaceLegacy/work/HyperHLE_ds_compat-r15-integration-20261031 --android-root E:/CodexProjects/DeadSpaceLegacy/work/HyperHLE_ds_compat-r15-android-integration-20261031 --report E:/CodexProjects/DeadSpaceLegacy/report/guard-source-new.json
```

可追加 `--pc-package <解压发布目录>`、`--android-package <含manifest.json与APK的目录>`、`--protected-manifest <路径到SHA256的JSON>`。报告路径必须全新，拒绝覆盖。检查失败返回1；参数/输出冲突返回2。未指定的范围明确 skipped。`scoped-checks-passed` 只表示本次文件检查通过，绝不是全项目验收。

如果项目树中存在必须保留、但不属于构建输入的已知未跟踪备份，可向
`--pc-artifact-exclude` 传入 JSON 对象 `{相对路径: 64位SHA256}`。Guard只对
`git ls-files --others` 返回的同一路径且哈希完全一致时排除；已跟踪改动、
未知新运行时文件或哈希变化仍失败。该参数用于精确冻结备份，不允许通配符。

检查：工作树/submodule清单、SBIN与JSON键值一致、当前资源指纹/文本修正/章节藏头、PC/Android样式与字体副本、Android既有光照实现、有限API21禁用模式、发布清单文件/声明源码快照、编译后内嵌文本、APK字体/样式/native库/原IPA、受保护文件。Git未提交改动作为工作中信息记录；发布检查会拒绝有未提交的声明编译输入或实现提交后改过的运行时代码。新增规则不应依赖源码排版/行号。

契约更新要说明用户要求、资源来源和重验范围。不要为消除失败而抄当前文件的哈希；先查明变化是否授权、是否需要重新构建。`contract.json` 的光照指纹是当前平台专属基线，变化需要有对应场景证据。历史工作台预设不会作为正式默认参与比对。

```powershell
python -m unittest discover -s tools/project-guard -p "test_*.py"
python tools/project-guard/run_save_tests.py --repo <Android工作树> --fixture-documents <只读原始Documents夹具> --output <全新测试输出目录> --javac <javac路径> --java <java路径>
```

第二条编译并执行仓库内真实 Java 存档/IPA保护探针；只在新目录建夹具副本，原始夹具前后逐文件哈希核验。使用只有slot0 checkpoint/level的实验夹具，测试将克隆出另两槽，不能声称三个原生有效进度。测试类使用桌面JDK API，不打包进Android；生产代码API21约束单独检查。这不是设备/SAF/原生换甲动画验收。追加 `--pc-repo <PC工作树> --csc <csc.exe路径>` 可编译执行 `tools/tests/SaveBundleTest.cs` 的PC三槽ZIP/拒绝写入回归。若用户游戏进程正在运行，它会安全拒绝，不应为测试杀掉用户进程。

发布与人工验收流程、证据层级及既知反复问题见 `dev-docs/R15-RELIABILITY.md`。当前实际设备/发布状态由项目最新交接及报告记录，不把机器IP或某次HEAD写成永久默认。

PC QTE 的额外检查会提取提示发布函数，拒绝直接调用旧定位扫描或 framebuffer 读取，并要求 tap/swipe 两条原生内存几何入口。它忽略排版、注释和字符串，保留发布函数以外显式开启的历史兼容代码；这是直接调用检查，不冒充完整调用图或运行时验收。负例覆盖条件分支中重新引入扫描以及丢失原生入口。

## 设备任务结束熄屏

仅在用户已经授权的设备上，显式运行：

```powershell
python tools/project-guard/device_job.py --adb <adb.exe> --serial <当前serial> --output <全新日志目录> --timeout 300 -- python <有限设备任务.py> --serial <同一serial>
```

此入口会实际发送ADB；它不属于离线guard，不自动唤醒或查询设备。成功、失败、超时/中断均在finally请求KEYCODE_SLEEP，保留任务与熄屏返回码。子任务仍须自己负责触点/按键释放、独立包/UID、原数据恢复和有限生命周期；包装器不把“熄屏请求已发送”冒充设备状态读回，也不代替这些清理。不得包装含旧硬编码设备地址、未经核对的历史脚本。对应测试全部mock，不连接ADB。

PC Win64 fastmem异常回调ABI修复使用显式platform_submodule_overrides，仅覆盖已知子模块的完整提交；默认及Android pin不变。变更原因、CDB故障及真实JIT回归见dev-docs/PC-JIT-FASTMEM.md。未知路径/无效提交和非预期实际指针仍拒绝，不自动更新基线。

PC英文布局：Game/manifest.json及运行时/原IPA，Defaults覆盖样式，顶层仅DeadSpace Launcher.exe；guard同时兼容既有flat发布件。Stage-Distribution.py只对全manifest校验的候选复制重排到全新输出，不修改输入、不做运行时验收。参见dev-docs/PC-DISTRIBUTION-LAYOUT.md。


PC EAGL传输缓存：guard拒绝thread_local宏中的已知GL缓存类型/标识，要求实际EAGL对象持有TransferState；CPU像素scratch允许按宿主线程复用。该规则是已复现旧缓存模式的结构检查，不是完整调用图/生命周期证明。真实独立/共享GL上下文的像素、附件及当前上下文释放回归见dev-docs/PC-EAGL-TRANSFER-CONTEXT.md；不能据此关闭玩家10h6x→4x90degree的精确验收。
