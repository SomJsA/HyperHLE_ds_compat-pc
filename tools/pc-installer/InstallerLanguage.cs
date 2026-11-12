using System;
using System.Collections.Generic;
using System.Windows.Forms;
namespace DeadSpaceInstaller {
internal static class InstallerLanguage {
private static readonly Dictionary<string,string> Messages = new Dictionary<string,string>(StringComparer.Ordinal) {
{"死亡空间移动版（IOS端高清移植） · 安装与更新","DEAD SPACE MOBILE · Install & Update"},
{"安装程序与游戏资源","Installer and Game Resources"},
{"程序位置","Program Location"},
{"游戏资源位置","Game Data Location"},
{"配置与三槽存档保存在文档目录；程序更新保留已有游戏资源和进度。","Configuration and all three save slots stay in Documents. Program updates preserve game resources and progress."},
{"资源来源 · marker / IPA","Resource Source · marker / IPA"},
{"语言组件 / Language mode","Game language components"},
{"高清中文组件（对话字幕 + 章节头）","HD Chinese component (dialogue subtitles + chapter headings)"},
{"ⓘ 说明","ⓘ Details"},
{"关闭后回归原生语言行为：安装版本不启用高清中文对话字幕和章节头；重新运行安装器即可切换。其他语言将在后续开放，欢迎通过 GitHub issue 投稿。","Disable this component for the original game language behavior. Re-run the installer to switch. More game-language components will be available later; requests are welcome on GitHub Issues."},
{"项目、上游与贡献 / Project, upstream and contributors","Project, upstream and contributors"},
{"SomJs：项目主要开发、手柄游玩测试、骁龙 Android 设备游玩测试\r\n","SomJs: lead development, gamepad and Snapdragon Android playtesting\r\n"},
{"lxyun_2（腹黑小喵）：超级校对、超级测试\r\n","lxyun_2 (腹黑小喵): extensive proofreading and testing\r\n"},
{"MisakaCharlotte（是的呀啊）：超级校对、超级测试\r\n","MisakaCharlotte (是的呀啊): extensive proofreading and testing\r\n"},
{"特洛克·洛克莱斯：超级检查测试\r\n","特洛克·洛克莱斯: extensive verification and testing\r\n"},
{"天雷本雷：键鼠游玩测试、天玑 Android 设备游玩测试\r\n","天雷本雷: keyboard/mouse and Dimensity Android playtesting\r\n"},
{"豆沙包里有磁：键鼠测试（顶着极低的全转储帧率）\r\n","豆沙包里有磁: keyboard/mouse testing at very low frame rates with full dumps\r\n"},
{"项目链接当前可能暂时处于私有状态；原版 credits_list 制作名单继续保留。","The project repository may currently be private. The original credits_list is retained."},
{"死亡空间贴吧汉化组","Dead Space Tieba Chinese translation team"},
{"高清纹理 · 可多选","HD Textures · Multi-select"},
{"  推荐","  Recommended"},
{"此 marker 不包含 ","This marker does not include "},
{"× 纹理，请选择提供的档位。","× textures. Choose an available tier."},
{"全选","Select all"},
{"可同时安装多个档位，在启动器中切换使用。全部取消则只装原版资源；已有纹理不会删除。","Install multiple tiers and switch them in the launcher. Deselect all for original resources only. Existing textures are preserved."},
{"快捷方式与启动","Shortcuts and Launch"},
{"桌面快捷方式","Desktop shortcut"},
{"开始菜单","Start Menu"},
{"完成后启动","Launch when complete"},
{"选择安装位置和资源后，点击开始安装。","Choose installation locations and resources, then click Install."},
{"开始安装","Install"},
{"退出","Exit"},
{"失败：","Failed: "},
{"重试安装","Retry installation"},
{"已完成，可打开 DeadSpace Launcher.exe。","Completed. You can open DeadSpace Launcher.exe."},
{"程序：","Program: "},
{"资源：","Resources: "},
{"更新 / 更改安装","Update / Change installation"},
{"安装完成。现在可以打开 DeadSpace Launcher.exe。","Installation complete. You can now open DeadSpace Launcher.exe."},
{"安装完成","Installation complete"},
{"检测到已有安装。资源来源留空时仅更新程序；选择 marker 可补装其他纹理档。","Existing installation detected. Leave the resource source empty to update only the program, or select a marker to add texture tiers."},
{"安装仍在进行，请等待校验完成后关闭。","Installation is still running. Wait for verification before closing."},
{"浏览…","Browse…"},
{"选择游戏资源","Choose game resources"},
{"游戏资源 (*.marker;*.ipa)|*.marker;*.ipa|所有文件 (*.*)|*.*","Game resources (*.marker;*.ipa)|*.marker;*.ipa|All files (*.*)|*.*"},
{"已有资源时可留空，仅更新程序。首次安装请选择 marker 或 IPA。","Leave empty to preserve existing resources and update only the program. First installation requires a marker or IPA."},
{"找不到此资源文件，请检查路径。","Resource file not found. Check the path."},
{"IPA 仅安装原版资源，不包含高清纹理包。","IPA installs original resources only; it contains no HD texture packs."},
{"缺少 vandal.json","Missing vandal.json"},
{"资源清单过大","Resource manifest is too large"},
{"marker 只读安装。请选择需要保留在电脑上的纹理档位。","Read-only marker installation. Select texture tiers to keep on this computer."},
{"无法读取资源清单：","Cannot read resource manifest: "},
{"（未提供）"," (Unavailable)"},
{"仅更新程序；保留已有资源。","Update program only; preserve existing resources."},
{"所选资源展开约 ","Selected resources expand to about "},
{" GiB（不含约 0.15 GiB 程序；升级所需空间按已有文件变化）。"," GiB (plus about 0.15 GiB for the program; upgrade space depends on existing files)."},
{"\r\n资源磁盘可用 ","\r\nAvailable on the resource disk: "},
{"开始安装…","Installing…"},
{"语言模式说明","Game language components"},
{"选择随游戏安装的语言层","Choose the language component installed with the game"},
{"高清中文组件","HD Chinese component"},
{"原生行为","Original language behavior"},
{"第 01 章\r\n当初善意\r\n“注意你的定位器。它比任何人都了解这个地方。”\r\n\r\nGPU 缓存的中文对话、目标和章节卡片。","Chapter 01\r\nHonorable Intentions\r\n“Watch the locator. It knows this place better than anyone.”\r\n\r\nGPU-cached Chinese dialogue, objective and chapter cards."},
{"Chapter 01 / 使用游戏原生文字\r\n\r\n不启用高清中文对话字幕和章节层。重新运行安装器即可切换。","Chapter 01 / original game text\r\n\r\nHD Chinese dialogue and chapter overlays are disabled. Re-run this installer to switch."},
{"其他语言选项将在后续开放，欢迎通过 GitHub issue 投稿。","More game-language components will be available later. Requests are welcome on GitHub Issues."},
{"关闭","Close"},
{"找不到 marker 或 IPA 安装源。","Marker or IPA installation source not found."},
{"首次安装必须选择 vandal.marker 或 IPA。","First installation requires vandal.marker or an IPA."},
{"正在解包程序文件...","Extracting program files..."},
{"安装包缺少 DeadSpace Launcher.exe。","The package is missing DeadSpace Launcher.exe."},
{"正在校验并安装游戏资源，请勿关闭安装器...","Verifying and installing game resources. Keep the installer open..."},
{"保留现有 GameData，正在更新程序...","Preserving existing GameData and updating program files..."},
{"安装完成。","Installation complete."},
{"启动器不存在。","Launcher not found."},
{"程序资源不存在。","Program resources not found."},
{"程序资源文件过少。","Too few program resource files."},
{"缺少参数值：","Missing argument value: "},
{"未知参数：","Unknown argument: "},
{"纹理档位必须是 2、4、6 的组合，或 none。","Texture tiers must be a combination of 2, 4 and 6, or none."},
{"程序目录和 GameData 目录不能互相包含。","The program and GameData directories must not contain each other."},
{"GameData 不能放在 Documents；Documents 只保存配置和三槽存档。","GameData cannot be inside Documents. Documents stores configuration and save slots only."},
{"检测到游戏或启动器正在运行，请先正常退出。","The game or launcher is running. Exit it normally before continuing."},
{"安装器内缺少程序资源。","The installer has no program payload."},
{"程序包缺少启动器或原生运行文件。","The program payload is missing its launcher or native runtime."},
{"程序包包含不安全路径。","The program payload contains an unsafe path."},
{"程序包路径越界。","The program payload path escapes its directory."},
{"资源安装失败（退出码 ","Resource installation failed (exit code "},
{"安装器内缺少卸载程序。","The installer has no uninstaller."},
{"Windows 未提供快捷方式目录。","Windows did not provide a shortcut directory."},
{"WScript.Shell 不可用。","WScript.Shell is unavailable."},
{"程序已安装，但创建","The program is installed, but creating the "},
{"桌面","desktop"},
{"快捷方式失败："," shortcut failed: "},
{"安装选项 →","Installation options →"},
{"00 关于","00 About"},
{"01 安装","01 Install"},
};

internal static string Translate(string text) {
 if(String.IsNullOrEmpty(text)) return text;
 if(DeadSpaceUi.UiLanguage.English) { string value; if(Messages.TryGetValue(text,out value))return value; }
 else foreach(var item in Messages)if(item.Value==text)return item.Key;
 string[] parts=text.Split(new[]{"\r\n"},StringSplitOptions.None);
 if(parts.Length>1) {
  for(int i=0;i<parts.Length;i++)foreach(var item in Messages) {
   string source=(DeadSpaceUi.UiLanguage.English?item.Key:item.Value).TrimEnd('\r','\n');
   if(parts[i]==source){parts[i]=(DeadSpaceUi.UiLanguage.English?item.Value:item.Key).TrimEnd('\r','\n');break;}
  }
  return String.Join("\r\n",parts);
 }
 return text;
}
internal static void Apply(Control root) {
 if(Convert.ToString(root.Tag)=="explicit-bilingual" || Convert.ToString(root.Tag)=="ui-language-toggle")return;
 if(!(root is TextBoxBase))root.Text=Translate(root.Text);
 foreach(Control child in root.Controls)Apply(child);
}
} }
