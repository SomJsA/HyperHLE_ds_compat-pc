# PNG RGBA 资源处理优化（2026-11-03）

来源：已交付PC全章节采集包d33b7796的独立战斗记录。首次加载7张不同8192×8192替换纹理；Image::from_bytes慢调用约1.21–1.61秒，不是同一张图片被重复解码的证明。原生PNG解压、CPU透明度处理、上传/驱动mip必须分开，加载收益不能写成实战FPS收益。

## 实现

src/image_premultiply.rs只处理PNG解码后已有的唯一可写RGBA切片。alpha255的乘数恰为1，保留RGB；alpha0按原实现写RGB零；中间值保持原f32除255、逐色乘法和u8截断。没有改成可能产生舍入差异的整数近似或倒数乘法。

大于等于4MiB的RGBA缓冲分为互不重叠的完整像素块，通过scoped线程纯处理；小图片单线程。沿用TOUCHHLE_DS_TEXTURE_WORKERS，默认PC最多8、Android最多4、硬件可用线程数及32上限；设1可回到同算法单线程。实际requested/spawned/fallback_chunks仅首次并行处理时记录。STB的全局设置和解码仍在原线程，不把Image或Environment强行标Send/Sync，不在worker调用GL、guest内存或原生输入。

线程创建失败时只由调用者处理尚未被worker取走的独立块，不重做整张图片。半透明乘法不具备幂等性，整图回退会二次变暗，故有实际故障注入回归。像素不增加或删除，PNG和IPA字节不改，图片尺寸、alpha-aware mip、过滤/纹理选择都保持。

## 组件证据与限制

桌面release优化设置（opt3、codegen1、thinLTO）及已有STB静态库，读取已交付8x PNG的独立进程内基准，拷贝准备/逐字节比较均在计时窗外，交替测试顺序。实际接入helper在69291a12.png上：旧阶段中位127.7365ms，单线程快速分支17.874ms，8worker6.5195ms；d98fa9a2.png（含0/255/半透明像素）：旧139.3395ms，单线程24.131ms，8worker5.585ms，所有结果逐字节相同。两次STB解码本身为1.826886秒和1.547982秒，不能据阶段数字宣称整次加载快20倍。

穷举256×256颜色/alpha取值、并行混合alpha和非整齐分块、多个线程创建失败位置、透明/不透明、空缓冲/0worker回退均有源代码测试。复测入口tools/tests/PngPremultiplyBench.rs；需用同构建STB库路径链接，命令在文件头，选择只读PNG，输出到新目录。

目前这些是组件及数学等价证据。完整release编译、真实关卡首载/跨关/实战和Android设备收益以对应报告为准。不得当作被动靠近门/电梯卡顿已修复、GPU编译预热已实现或双端稳定60已达成；PNG解压仍是大头。采集包d33b7796 ZIP保持原样，回传分析使用它自己的diagnostic-map和源码提交。
