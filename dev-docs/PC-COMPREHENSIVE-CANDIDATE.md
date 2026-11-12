# PC 综合优化候选版

这是供人工最终把关的**独立候选启动方式**。稳定启动器默认值不改变，候选通过子进程环境启用：

- 首战斗 JIT 预热；
- 战斗轮询延迟优化；
- CPU 蒙皮 host math + strided 输入；
- `Transform::PostMultiply` host math；
- 原始 ARM 路径保留，候选各自独立做暖机比对和周期回查。

启动：

```powershell
powershell -ExecutionPolicy Bypass -File .\Start-Optimized-Candidate.ps1 -ProgramRoot "C:\你的安装目录"
```

候选不会修改全局注册表或稳定配置。任一候选遇到指纹变化、不支持输入或输出 mismatch，都会自动恢复原始路径并停用本进程候选。

本机组合 smoke 证据：

- 47 次真实开火 dispatch；
- skin：81,408 次 host 输出、382 次原始回查，`mismatch=0`、`unsupported=0`；
- transform：3,786,235 次 host 输出、14,853 次原始回查，`mismatch=0`、`unsupported=0`；
- JIT prewarm 1257 blocks；
- seed/IPA 未变化，进程正常关闭。

这是候选版，不等同于全章节或 i5+5060 的最终验收。若出现画面、动画、长帧或稳定性问题，直接退出候选并用普通 `DeadSpace Launcher.exe` 启动即可。
