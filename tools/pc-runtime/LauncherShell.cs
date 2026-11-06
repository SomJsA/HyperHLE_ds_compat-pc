using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Security.Cryptography;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private static readonly Color Shell = Color.FromArgb(10, 17, 24);
        private static readonly Color Surface = Color.FromArgb(19, 30, 40);
        private static readonly Color Ink = Color.FromArgb(221, 232, 237);
        private static readonly Color Muted = Color.FromArgb(142, 163, 176);
        private static readonly Color Accent = Color.FromArgb(98, 218, 232);
        private readonly List<Panel> pages = new List<Panel>();
        private readonly List<Button> navigation = new List<Button>();
        private SaveSettings loadedSave;
        private readonly CheckBox attackPlugin = new CheckBox();
        private readonly CheckBox defensePlugin = new CheckBox();
        private readonly CheckBox lootPlugin = new CheckBox();
        private readonly Label saveStatus = new Label();
        private readonly Button writeSave = new Button();

        private void BuildShell()
        {
            BackColor = Shell;
            ForeColor = Ink;
            Controls.Add(new Label { Text = "DEAD SPACE", Font = new Font("Segoe UI", 27, FontStyle.Bold),
                ForeColor = Ink, Location = new Point(28, 24), AutoSize = true });
            Controls.Add(new Label { Text = "IOS 移植  /  PC", ForeColor = Accent,
                Location = new Point(32, 78), AutoSize = true });
            Image poster = LoadEmbeddedPoster();
            if (poster != null)
            {
                Controls.Add(new PictureBox
                {
                    Image = poster,
                    Location = new Point(48, 488),
                    Size = new Size(128, 128),
                    SizeMode = PictureBoxSizeMode.Zoom,
                    BorderStyle = BorderStyle.FixedSingle
                });
            }
            var main = new Panel { Location = new Point(224, 126), Size = new Size(748, 510),
                Anchor = AnchorStyles.Top | AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right };
            Controls.Add(main);
            Panel graphics = Page(main, "01   画质与显示");
            AddHardwareReviewButton(graphics);
            Panel input = Page(main, "02   鼠标与操作");
            Panel bindings = Page(main, "03   键盘绑定");
            BuildKeyBindings(bindings);
            Panel gamepadBindings = Page(main, "04   手柄与摇杆");
            BuildGamepadBindings(gamepadBindings);
            Panel library = Page(main, "05   游戏与存档位置");
            Panel editor = Page(main, "06   存档编辑器");
            BuildSaveEditor(editor);
            Panel saves = Page(main, "07   存档插件");
            Panel compatibility = Page(main, "08   兼容与高级");

            Heading(graphics, "画质与显示", "分辨率按屏幕独立识别；画质档位调整采样和过滤。", 22, 440);
            AddComboRow(graphics, "内部渲染", 96, scaleHack, new[] { AutomaticRenderScale, "1x 性能", "2x 平衡", "3x 清晰", "4x 精细（1920×1280）", "5x 手动（2400×1600）", "6x 高画质（2880×1920）", "7x 手动（3360×2240）", "8x 超采样（3840×2560）" });
            scaleHack.Width = 310;
            AddComboRow(graphics, "输出分辨率", 142, displayResolution,
                new[] { "跟随内部", "1280 × 720", "1600 × 900", "1920 × 1080", "1920 × 1200", "2560 × 1440", "3840 × 2160" });
            AddComboRow(graphics, "窗口模式", 188, windowMode, new[] { "窗口", "无边框全屏", "独占全屏" });
            AddComboRow(graphics, "帧率上限", 234, fpsCap, new[] { "30 FPS", "60 FPS", "70 FPS", "不限" });
            BuildSamplingOptions(graphics);
            BuildTextureOptions(graphics, 370);
            AddComboRow(graphics, "超分算法", 578, fsrMode, new[] { "关闭", "FSR1 EASU + RCAS", "FSR1 EASU（轻量）" });
            AddCheck(graphics, sharpen, 628, "追加 CAS 锐化（可独立于 FSR1 使用）", false);
            AddCheck(graphics, fastPresent, 668, "快速呈现", false);
            AddCheck(graphics, gpuPresentCopy, 704, "3D加速模式（GPU copy，默认开启；关闭为兼容模式 CPU copy）", true);
            LabelText(graphics, "屏幕匹配：1080P用4×、1440P用6×、4K用8×；内部尺寸独立于画质。\r\nMSAA 受驱动上限约束；本管线尚无 TAA，不能承诺消除所有运动闪烁。", 30, 748, 665, 44);

            Heading(input, "鼠标与操作", "鼠标捕获、瞄准与常用操作；手柄摇杆在04页单独调整。", 22);
            AddComboRow(input, "鼠标捕获", 96, mouseLook, new[] { "关闭", "仅瞄准时捕获", "始终捕获" });
            BuildPointerOptions(input);
            AddCheck(input, pcMouse, 232, "右键瞄准，左键射击", true);
            pcMouse.Width = 280;
            mouseAimMode.DropDownStyle = ComboBoxStyle.DropDownList; mouseAimMode.SetBounds(350, 230, 322, 30);
            mouseAimMode.Items.AddRange(new object[] { "右键：按住瞄准，松开收枪", "右键：点击举枪，再点收枪" });
            input.Controls.Add(mouseAimMode); pcMouse.CheckedChanged += delegate { mouseAimMode.Enabled = pcMouse.Checked; };
            LabelText(input, "举枪前移", 30, 284, 135, 24);
            aimedWalk.Text = "举枪禁用前向疾跑"; aimedWalk.SetBounds(174, 278, 225, 30); input.Controls.Add(aimedWalk);
            aimedForwardPercent.Minimum = 5; aimedForwardPercent.Maximum = 100; aimedForwardPercent.Increment = 1; aimedForwardPercent.Value = 100;
            aimedForwardPercent.SetBounds(414, 278, 100, 30); input.Controls.Add(aimedForwardPercent); LabelText(input, "% 步行上限", 528, 284, 160, 24);
            aimedWalk.CheckedChanged += delegate { aimedForwardPercent.Enabled = aimedWalk.Checked; };
            AddTextRow(input, "移动摇杆增益", 324, stickGain);
            AddComboRow(input, "输入设备", 365, inputMode, new[] { "自动（键鼠 + 手柄）", "仅键鼠", "仅手柄" });
            AddComboRow(input, "空格副模式", 410, keyboardAltMode, new[] { "按住空格为副模式", "按空格切换主／副模式" });
            keyboardAltMode.Width = 322;
            LabelText(input, "举枪步行上限共用于键鼠和手柄；100%为最快步行，小拉保持原幅度。\r\nWASD 移动 · E 交互 · Ctrl / G / B 定位 · Z 快速转身\r\nF 念力 · R 装弹 · X 旋转 · F6 检查点 · F7 画面标记", 30, 501, 680, 66);

            Heading(compatibility, "兼容与诊断", "常规游玩保留雾效；实验选项只用于定位问题。", 22);
            AddCheck(compatibility, pcCompat, 96, "启用已验证的 PC 渲染兼容修复", true);
            AddCheck(compatibility, openingMips, 140, "修正开场贴图的远距离采样", true);
            AddCheck(compatibility, globalMips, 184, "实验：重建全部贴图的远距离采样", false);
            AddCheck(compatibility, disableFog, 228, "诊断：关闭场景雾", false);
            AddCheck(compatibility, screenFogMask, 272, "实验：背景层排除雾效", false);
            AddCheck(compatibility, skipFinish, 316, "实验：慢呈现路径跳过同步等待", false);
            AddCheck(compatibility, debugLogs, 360, "详细调试日志", false);
            AddCheck(compatibility, frameDump, 404, "定期保存画面到 frames（影响性能）", false);
            AddCheck(compatibility, nativeHud, 448, "原生切枪、定位与暂停界面", false);
            AddCheck(compatibility, confirmQuit, 492, "关闭游戏窗口前确认退出", true);
            AddCheck(compatibility, nativeInputs, 536, "PC 原生视角与移动", true);
            AddTextRow(compatibility, "触摸回退瞄准点", 580, aimPoint);
            AddCheck(compatibility, overlayEnabled, 624, "高清图文组件（章节 / 任务追踪 / 字幕；样式由外部工作台调整）", true);
            AddCheck(compatibility, superMonitor, 668, "性能监视器与记录（2Hz）", false);
            AddComboRow(compatibility, "纹理上传同步", 712, textureSync,
                new[] { "Finish（保守，默认）", "Flush（A/B）", "关闭（诊断）" });
            AddCheck(compatibility, gpuTextOverlay, 756, "实验：GPU 章节 / 任务 / 字幕（取消勾选恢复 CPU）", false);

            AddComboRow(compatibility,"长句字幕",800,subtitleMode,new[]{"整段三行","大字分段"});


            Heading(library, "游戏文件与存档", "选择游戏文件和存档位置；迁移不会覆盖已有进度。", 22);
            LabelText(library, "已安装游戏数据", 30, 96, 330, 24);
            ipaPath.Location = new Point(30, 134); ipaPath.Size = new Size(650, 28);
            ipaPath.ReadOnly = true;
            library.Controls.Add(ipaPath);
            var browse = new Button { Text = "选择 IPA 安装源", Location = new Point(30, 182), Size = new Size(160, 38) };
            browse.Click += delegate { BrowseIpa(); }; library.Controls.Add(browse);
            BuildMarkerControls(library);
            BuildStorageControls(library);

            Heading(saves, "存档插件", "编辑所选存档位置的 settings.sb，写入前自动备份。", 22);
            AddCheck(saves, attackPlugin, 96, "攻击增强（attack）", false);
            AddCheck(saves, defensePlugin, 140, "防御增强（defense）", false);
            AddCheck(saves, lootPlugin, 184, "资源增强（loot）", false);
            var readSave = new Button { Text = "读取当前存档", Location = new Point(30, 242), Size = new Size(170, 40) };
            readSave.Click += delegate { ReadCurrentSave(); }; saves.Controls.Add(readSave);
            writeSave.Text = "保存插件开关"; writeSave.Location = new Point(218, 242); writeSave.Size = new Size(170, 40); writeSave.Enabled = false;
            writeSave.Click += delegate
            {
                try
                {
                    string backup = loadedSave.Write(attackPlugin.Checked, defensePlugin.Checked, lootPlugin.Checked);
                    ReadCurrentSave(); saveStatus.Text = "保存成功。备份：\r\n" + backup;
                }
                catch (Exception ex) { saveStatus.Text = ex.Message; }
            };
            saves.Controls.Add(writeSave);
            saveStatus.Location = new Point(30, 312); saveStatus.Size = new Size(680, 160);
            saveStatus.Text = "游戏运行时可以读取，保存需先退出游戏。\r\n这里只修改三个插件的解锁状态，不改章节进度或物品。\r\n插件在游戏中的实际效果仍待验证。"; saves.Controls.Add(saveStatus);

            var defaults = new Button { Text = "恢复默认", Location = new Point(224, 662), Size = new Size(122, 42), Anchor = AnchorStyles.Left | AnchorStyles.Bottom };
            defaults.Click += delegate { ApplyDefaults(); status.Text = "已恢复默认；启动时保存。"; }; Controls.Add(defaults);
            launch.Text = "启动游戏  →"; launch.Location = new Point(792, 656); launch.Size = new Size(180, 48); launch.Anchor = AnchorStyles.Right | AnchorStyles.Bottom;
            launch.Click += delegate { LaunchGame(); }; Controls.Add(launch);
            stopGame.Text = "停止游戏"; stopGame.Location = new Point(634, 656); stopGame.Size = new Size(146, 48); stopGame.Anchor = AnchorStyles.Right | AnchorStyles.Bottom; stopGame.Enabled = false;
            stopGame.Click += delegate { StopGame(); }; Controls.Add(stopGame);
            status.Location = new Point(224, 710); status.Size = new Size(748, 24);
            status.Anchor = AnchorStyles.Left | AnchorStyles.Right | AnchorStyles.Bottom; Controls.Add(status);
            ArrangeDistributionPages(graphics, input, compatibility);
            BuildPlayDiagnostics(compatibility);
            Theme(this);
            launch.BackColor = Accent; launch.ForeColor = Shell;
            Controls.Add(new Label
            {
                Text = "让我们把泰坦空间站烧成灰",
                Location = new Point(32, 96),
                Size = new Size(178, 24),
                Font = new Font("Microsoft YaHei UI", 8.5f),
                ForeColor = Accent
            });
            Controls.Add(new Label
            {
                Text = "PC_CONTENT / WINDOWS NATIVE\r\nBUILD " + RuntimeBuildTag() + " · GAME 1.3.34\r\nPORTABLE / WINDOWS",
                Location = new Point(28, 622),
                Size = new Size(178, 38),
                Font = new Font("Consolas", 6.5f),
                ForeColor = Muted
            });
            SelectPage(0);
        }

        private string RuntimeBuildTag()
        {
            string executable = Path.Combine(DistributionLayout.Game(baseDir), "touchHLE.exe");
            if (!File.Exists(executable)) return "UNAVAILABLE";
            using (SHA256 sha = SHA256.Create())
            using (FileStream stream = File.OpenRead(executable))
            {
                return BitConverter.ToString(sha.ComputeHash(stream))
                    .Replace("-", "")
                    .Substring(0, 8);
            }
        }

        private static Image LoadEmbeddedPoster()
        {
            using (Stream stream = typeof(LauncherForm).Assembly.GetManifestResourceStream(
                "DeadSpaceBootstrap.DeadSpace-Poster.png"))
            {
                if (stream == null) return null;
                using (Image image = Image.FromStream(stream))
                {
                    return new Bitmap(image);
                }
            }
        }

        private void ReadCurrentSave()
        {
            try
            {
                string file = Path.Combine(SaveStorage.ProfilePath(SelectedDataRoot()), "Documents", "var", "settings.sb");
                loadedSave = new SaveSettings(file);
                attackPlugin.Checked = loadedSave.Enabled("attack"); defensePlugin.Checked = loadedSave.Enabled("defense"); lootPlugin.Checked = loadedSave.Enabled("loot");
                writeSave.Enabled = true;
                saveStatus.Text = "已读取：\r\n" + file + "\r\n\r\n此设置由该运行目录的存档槽共享；写入需先退出游戏。";
            }
            catch (Exception ex) { loadedSave = null; writeSave.Enabled = false; saveStatus.Text = "无法读取：" + ex.Message; }
        }

        private Panel Page(Panel host, string name)
        {
            int index = pages.Count;
            var page = new Panel { Dock = DockStyle.Fill, AutoScroll = true, Visible = false };
            pages.Add(page); host.Controls.Add(page);
            var button = new Button { Text = name, TextAlign = ContentAlignment.MiddleLeft,
                Location = new Point(24, 128 + index * 43), Size = new Size(180, 38) };
            button.Click += delegate { SelectPage(index); };
            navigation.Add(button); Controls.Add(button); return page;
        }

        private void SelectPage(int index)
        {
            for (int i = 0; i < pages.Count; i++)
            {
                pages[i].Visible = i == index;
                if (i == index) pages[i].BringToFront();
                navigation[i].ForeColor = i == index ? Accent : Muted;
                navigation[i].BackColor = i == index ? Surface : Shell;
            }
        }

        private void BuildPointerOptions(Control input)
        {
            // Keep the four legacy INI values readable; show separate axis switches.
            lookDirection.Items.AddRange(new object[] { "反转水平+垂直", "标准", "只反水平", "只反垂直" });
            lookDirection.SelectedIndexChanged += delegate {
                if (syncingPointerOptions) return;
                syncingPointerOptions = true;
                invertMouseX.Checked = lookDirection.SelectedIndex == 0 || lookDirection.SelectedIndex == 2;
                invertMouseY.Checked = lookDirection.SelectedIndex == 0 || lookDirection.SelectedIndex == 3;
                syncingPointerOptions = false;
            };
            EventHandler changed = delegate {
                if (syncingPointerOptions) return;
                syncingPointerOptions = true;
                lookDirection.SelectedIndex = invertMouseX.Checked ? (invertMouseY.Checked ? 0 : 2) : (invertMouseY.Checked ? 3 : 1);
                syncingPointerOptions = false;
            };
            invertMouseX.CheckedChanged += changed; invertMouseY.CheckedChanged += changed;
            LabelText(input, "视角方向", 30, 148, 135, 24);
            invertMouseX.Text = "反转横向"; invertMouseX.SetBounds(174, 142, 150, 30);
            invertMouseY.Text = "反转纵向"; invertMouseY.SetBounds(350, 142, 150, 30);
            input.Controls.Add(invertMouseX); input.Controls.Add(invertMouseY);
            var reset = new Button { Text = "鼠标默认", Location = new Point(542, 140), Size = new Size(130, 32) };
            reset.Click += delegate { invertMouseX.Checked = invertMouseY.Checked = false;
                releaseF8.Checked = releaseAlt.Checked = releaseWin.Checked = true; lookSensitivity.Value = 100;
                status.Text = "已恢复正常方向、100%灵敏度和默认释放键；启动时保存。"; };
            input.Controls.Add(reset);
            LabelText(input, "镜头灵敏度", 30, 194, 135, 24);
            lookSensitivity.Minimum = 5; lookSensitivity.Maximum = 500; lookSensitivity.Increment = 5;
            lookSensitivity.Value = 100; lookSensitivity.SetBounds(174, 188, 150, 30);
            LabelText(input, "%（100%为默认）", 338, 194, 250, 24); input.Controls.Add(lookSensitivity);
            lookSensitivity.ValueChanged += delegate { lookGain.Text = (lookSensitivity.Value / 100m).ToString("0.00", System.Globalization.CultureInfo.InvariantCulture); };
            lookGain.TextChanged += delegate { decimal value;
                if (Decimal.TryParse(lookGain.Text, System.Globalization.NumberStyles.Float, System.Globalization.CultureInfo.InvariantCulture, out value))
                    lookSensitivity.Value = Math.Min(500, Math.Max(5, value * 100));
            };
            releaseF8.Text = "F8 切换点击/镜头"; releaseAlt.Text = "Alt 按住临时点击"; releaseWin.Text = "Win 临时释放";
            CheckBox[] toggles = { releaseF8, releaseAlt, releaseWin };
            for (int i = 0; i < toggles.Length; i++) { toggles[i].SetBounds(30 + i * 225, 458, 220, 30); input.Controls.Add(toggles[i]); }
        }

        private static void LabelText(Control parent, string text, int x, int y, int width, int height)
        { parent.Controls.Add(new Label { Text = text, Location = new Point(x, y), Size = new Size(width, height) }); }

        private static void Heading(Control page, string title, string description, int y, int titleWidth = 650)
        {
            page.Controls.Add(new Label { Text = title, Font = new Font("Microsoft YaHei UI", 17, FontStyle.Bold),
                Location = new Point(30, y), Size = new Size(titleWidth, 38) });
            LabelText(page, description, 30, y + 42, 680, 26);
        }

        private static void Theme(Control control)
        {
            foreach (Control child in control.Controls)
            {
                child.BackColor = child is Panel ? Surface : control.BackColor;
                child.ForeColor = Ink;
                var button = child as Button;
                if (button != null) { button.FlatStyle = FlatStyle.Flat; button.FlatAppearance.BorderColor = Color.FromArgb(48, 72, 87); }
                if (child is TextBox || child is ComboBox) child.BackColor = Shell;
                Theme(child);
            }
        }
    }
}
