# PC 持续单核压力候选：CPU 蒙皮

这是一个**可回退、默认不启用**的候选。它不改变正式启动器默认值，也不覆盖原始 ARM 蒙皮函数。

启用方式（在已安装的程序目录旁运行 PowerShell）：

```powershell
powershell -ExecutionPolicy Bypass -File .\Start-Skin-Candidate.ps1 -ProgramRoot "C:\你的安装目录"
```

候选会通过子进程环境启用：

- host skin math；
- strided 输入，减少四组输入复制/分配；
- 前 64 次真实原始结果比对；
- 之后每 256 次回查一次原始结果；
- 输入不支持、输出不一致或指纹变化时自动恢复原始路径并永久停用本进程候选。

本机受控 smoke 证据：

- 46 次真实开火 dispatch；
- 91,392 次 host 输出；
- 421 次原始结果回查；
- `mismatch=0`、`unsupported=0`；
- seed/IPA 未变化，进程正常关闭。

这不是正式默认性能承诺。最终是否采用由人工验收决定；若关闭候选，只需正常启动原启动器，或删除 `%LOCALAPPDATA%\\DeadSpaceLegacy\\skin-candidate.trigger`。
