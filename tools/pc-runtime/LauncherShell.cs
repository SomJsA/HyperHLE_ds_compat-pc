using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Security.Cryptography;
using System.Diagnostics;
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
        private readonly Panel navigationHost = new Panel();
        private readonly Panel mainHost = new Panel();
        private Panel saveEditorPage;
        private DeadSpaceUi.ProjectAboutPanel aboutPanel;
        private int selectedPageIndex;
        private readonly Button restoreDefaults = new Button();
        private SaveSettings loadedSave;
        private readonly CheckBox attackPlugin = new CheckBox();
        private readonly CheckBox defensePlugin = new CheckBox();
        private readonly CheckBox lootPlugin = new CheckBox();
        private readonly Label saveStatus = new DeadSpaceUi.UiLanguage.LocalizedLabel();
        private readonly Button writeSave = new Button();

        private void BuildShell()
        {
            BackColor = Shell;
            ForeColor = Ink;
            Controls.Add(new Label { Text = "DEAD SPACE\r\nMOBILE", Font = new Font("Segoe UI", 15, FontStyle.Bold),
                ForeColor = Ink, Tag = "sidebar-title", Location = new Point(28, 18), Size = new Size(184, 56) });
            Controls.Add(new Label { Text = "IOS HD PORT / PC", ForeColor = Accent, Tag = "sidebar-subtitle",
                Location = new Point(32, 78), AutoSize = true });
            languageToggle.Tag = "ui-language-toggle";
            languageToggle.Text = DeadSpaceUi.UiLanguage.ToggleCaption;
            languageToggle.Size = new Size(92, 30);
            languageToggle.Location = new Point(878, 18);
            languageToggle.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            languageToggle.FlatStyle = FlatStyle.Flat;
            languageToggle.Click += delegate { DeadSpaceUi.UiLanguage.Toggle(); ApplyUiLanguage(); };
            Controls.Add(languageToggle);
            navigationHost.SetBounds(24, 164, 184, 310);
            navigationHost.AutoScroll = true;
            Controls.Add(navigationHost);
            Image poster = LoadEmbeddedPoster();
            if (poster != null)
            {
                Controls.Add(new PictureBox
                {
                    Image = poster, Tag = "sidebar-poster",
                    Location = new Point(48, 488),
                    Size = new Size(128, 128),
                    SizeMode = PictureBoxSizeMode.Zoom,
                    Anchor = AnchorStyles.Left | AnchorStyles.Bottom,
                    BorderStyle = BorderStyle.FixedSingle
                });
            }
            mainHost.Location = new Point(224, 64);
            mainHost.Size = new Size(Math.Max(480, ClientSize.Width - 248), Math.Max(220, ClientSize.Height - 168));
            mainHost.Anchor = AnchorStyles.Top | AnchorStyles.Bottom | AnchorStyles.Left | AnchorStyles.Right;
            Controls.Add(mainHost);
            var main = mainHost;
            Panel about = Page(main, "00 关于");
            aboutPanel = new DeadSpaceUi.ProjectAboutPanel(); about.Controls.Add(aboutPanel);
            Panel graphics = Page(main, "01 画质与显示");
            Panel input = Page(main, "02 鼠标与操作");
            Panel bindings = Page(main, "03 键盘绑定");
            BuildKeyBindings(bindings);
            Panel gamepadBindings = Page(main, "04 手柄与摇杆");
            BuildGamepadBindings(gamepadBindings);
            Panel library = Page(main, "05 游戏与存档位置");
            library.AutoScrollMinSize = new Size(710, 980);
            Panel editorPage = Page(main, "06 存档编辑器"); saveEditorPage = editorPage;
            Panel editor = Section(editorPage, 460, 760);
            BuildSaveEditor(editor);
            Panel quick = Section(editorPage, 0, 440);
            BuildQuickSaveEditor(quick);
            editorPage.AutoScrollMinSize = new Size(710, 1240);
            Panel compatibility = Page(main, "07 调试");
            AddHardwareReviewButton(compatibility);

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
            AddCheck(graphics, safeBlackFloor, 806, "护眼模式（实验性）：将接近黑色的区域替换为暗灰，减少整屏黑闪", false);
            safeBlackFloor.Font = new Font("Microsoft YaHei UI", 10.5f, FontStyle.Bold);
            LabelText(graphics,
                "原游戏含大量高闪烁场景，这是营造氛围的原生行为。游玩时如有不适，请停止游玩并及时休息。\r\n护眼模式默认关闭。开启后会改变黑色、暗部和原有氛围，影响观感；灰度过高可能使黑场发白。\r\n可拖动滑块调整灰度；此开关不会消除所有闪烁。",
                30, 842, 665, 130);
            LabelText(graphics, "黑色替换灰度（默认 10 / 255）", 30, 986, 400, 24);
            safeBlackGray.Minimum = 0;
            safeBlackGray.Maximum = 160;
            safeBlackGray.TickFrequency = 10;
            safeBlackGray.SmallChange = 1;
            safeBlackGray.LargeChange = 10;
            safeBlackGray.Value = 10;
            safeBlackGray.SetBounds(30, 1010, 560, 40);
            graphics.Controls.Add(safeBlackGray);
            safeBlackGrayValue.TextAlign = ContentAlignment.MiddleLeft;
            safeBlackGrayValue.SetBounds(604, 1014, 100, 28);
            graphics.Controls.Add(safeBlackGrayValue);
            EventHandler updateSafeGray = delegate { safeBlackGrayValue.Text = safeBlackGray.Value + " / 255"; };
            safeBlackGray.ValueChanged += updateSafeGray;
            safeBlackFloor.CheckedChanged += delegate { safeBlackGray.Enabled = safeBlackFloor.Checked; safeBlackGrayValue.Enabled = safeBlackFloor.Checked; };
            updateSafeGray(null, EventArgs.Empty);
            safeBlackFloor.Checked = false;
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
            LabelText(input, "举枪步行上限共用于键鼠和手柄；100%为最快步行，小拉保持原幅度。\r\nWASD 移动 · E 交互 · Ctrl 无重力跳跃 · G / B 定位 · Z 快速转身\r\nF 念力 · R 装弹 · X 旋转 · F6 检查点 · F7 画面标记", 30, 501, 680, 66);

            Heading(compatibility, "兼容与诊断", "常规游玩保留雾效；实验选项只用于定位问题。", 22, 430);
            AddCheck(compatibility, pcCompat, 96, "启用已验证的 PC 渲染兼容修复", true);
            AddCheck(compatibility, openingMips, 140, "修正开场贴图的远距离采样", true);
            AddCheck(compatibility, globalMips, 184, "实验：重建全部贴图的远距离采样", false);
            AddCheck(compatibility, disableFog, 228, "诊断：关闭场景雾", false);
            AddCheck(compatibility, screenFogMask, 272, "实验：背景层排除雾效", false);
            AddCheck(compatibility, skipFinish, 316, "实验：慢呈现路径跳过同步等待", false);
            AddCheck(compatibility, debugLogs, 360, "详细调试日志", false);
            AddCheck(compatibility, frameDump, 404, "定期保存画面到 frames（影响性能）", false);
            AddCheck(compatibility, nativeHud, 448, "原生切枪、定位与暂停界面", true);
            AddCheck(compatibility, confirmQuit, 492, "关闭游戏窗口前确认退出", true);
            AddCheck(compatibility, nativeInputs, 536, "PC 原生视角与移动", true);
            AddTextRow(compatibility, "触摸回退瞄准点", 580, aimPoint);
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

            var defaults = restoreDefaults; defaults.Text = "恢复默认"; defaults.Size = new Size(106, 42);
            defaults.Click += delegate { if (IsSaveEditorPage) { UndoSaveChanges(); return; } ApplyDefaults(); status.Text = "已恢复默认；启动时保存。"; }; Controls.Add(defaults);
            launch.Text = "启动游戏 →"; launch.Location = new Point(812, 656); launch.Size = new Size(160, 48); launch.Anchor = AnchorStyles.Right | AnchorStyles.Bottom;
            launch.Click += delegate { LaunchGame(); }; Controls.Add(launch);
            stopGame.Text = "停止游戏"; stopGame.Location = new Point(674, 656); stopGame.Size = new Size(126, 48); stopGame.Anchor = AnchorStyles.Right | AnchorStyles.Bottom; stopGame.Enabled = false;
            stopGame.Click += delegate { StopGame(); }; Controls.Add(stopGame);
            stopGame.Visible = stopGame.Enabled;
            stopGame.EnabledChanged += delegate { stopGame.Visible = stopGame.Enabled; };
            status.Location = new Point(224, 710); status.Size = new Size(Math.Max(480, ClientSize.Width - 248), 24);
            status.AutoEllipsis = true;
            var statusTip = new ToolTip();
            status.TextChanged += delegate { statusTip.SetToolTip(status, status.Text); };
            Disposed += delegate { statusTip.Dispose(); };
            status.Anchor = AnchorStyles.Left | AnchorStyles.Right | AnchorStyles.Bottom; Controls.Add(status);
            ArrangeDistributionPages(graphics, input, compatibility);
            // Insert primary controls after the existing resolution-summary and
            // row arrangement, so later layout passes cannot cover them.
            int graphicsBottom=0;
            foreach(Control control in graphics.Controls) {
                if(control.Top>=96) control.Top+=112;
                graphicsBottom=Math.Max(graphicsBottom,control.Bottom);
            }
            graphics.AutoScrollMinSize=new Size(710,graphicsBottom+24);
            AddCheck(graphics,overlayEnabled,112,"高清中文文字组件（章节 / 任务 / 字幕）",true);
            AddCheck(graphics,advancedLighting,156,"高级光影（默认开启）",true);
            BuildPlayDiagnostics(compatibility);
            Theme(this);
            navigationHost.BackColor = Shell;
            launch.BackColor = Accent; launch.ForeColor = Shell;
            Controls.Add(new Label
            {
                Text = "快行动，破坏者！\r\n让我们把泰坦空间站烧成灰！",
                Location = new Point(32, 96),
                Size = new Size(184, 58),
                Font = new Font("Microsoft YaHei UI", 8.5f),
                ForeColor = Accent, Tag = "sidebar-egg"
            });
            Controls.Add(new Label
            {
                Text = "WINDOWS · DEAD SPACE MOBILE\r\nVERSION " + DeadSpaceUi.ReleaseChannel.CurrentTag + "\r\nBUILD " + RuntimeBuildTag(),
                Location = new Point(28, 622),
                Size = new Size(178, 58),
                Font = new Font("Consolas", 8f),
                Anchor = AnchorStyles.Left | AnchorStyles.Bottom,
                ForeColor = Muted
                , Tag = "sidebar-build"
            });
            SelectPage(0);

        }

        private static LinkLabel CreditsLink(string caption, string url)
        {
            var link = new LinkLabel { Text = caption, AutoSize = true, LinkColor = Accent, ActiveLinkColor = Color.White, Margin = new Padding(0, 2, 16, 2), Cursor = Cursors.Hand };
            link.Links.Add(0, caption.Length, url);
            link.LinkClicked += delegate(object sender, LinkLabelLinkClickedEventArgs e)
            {
                try { Process.Start(new ProcessStartInfo { FileName = Convert.ToString(e.Link.LinkData), UseShellExecute = true }); }
                catch (Exception) { }
            };
            return link;
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

        private string RuntimeRevision()
        {
            try
            {
                string path = Path.Combine(DistributionLayout.Game(baseDir), "build-info.json");
                if (File.Exists(path))
                {
                    var map = new System.Web.Script.Serialization.JavaScriptSerializer().DeserializeObject(File.ReadAllText(path)) as System.Collections.Generic.Dictionary<string, object>;
                    string revision = map == null ? null : Convert.ToString(map.ContainsKey("revision") ? map["revision"] : null);
                    if (!String.IsNullOrWhiteSpace(revision)) return revision;
                }
            }
            catch (Exception) { }
            return "R15";
        }

        private static System.Drawing.Icon LoadEmbeddedIcon()
        {
            try
            {
                using (Stream stream = typeof(LauncherForm).Assembly.GetManifestResourceStream(
                    "DeadSpaceBootstrap.DeadSpace.ico"))
                {
                    return stream == null ? null : new System.Drawing.Icon(stream);
                }
            }
            catch (Exception) { return null; }
        }

        private Image LoadEmbeddedPoster()
        {
            try
            {
                using (Stream stream = typeof(LauncherForm).Assembly.GetManifestResourceStream(
                    "DeadSpaceBootstrap.DeadSpace-Poster.png"))
                {
                    if (stream != null)
                    {
                        using (Image image = Image.FromStream(stream))
                        {
                            return new Bitmap(image);
                        }
                    }
                }
            }
            catch (Exception) { }
            string[] fallback = {
                Path.Combine(baseDir, "Assets", "launcher", "DeadSpace-Poster.png"),
                Path.Combine(baseDir, "assets", "DeadSpace-Poster.png")
            };
            foreach (string path in fallback)
            {
                try
                {
                    if (!File.Exists(path)) continue;
                    using (Image image = Image.FromFile(path)) return new Bitmap(image);
                }
                catch (Exception) { }
            }
            return null;
        }

        private void ReadCurrentSave()
        {
            try
            {
                string file = Path.Combine(SaveStorage.ProfilePath(SelectedDataRoot()), "Documents", "var", "settings.sb");
                loadedSave = new SaveSettings(file);
                bool complete = loadedSave.Available("attack") && loadedSave.Available("defense") && loadedSave.Available("loot");
                attackPlugin.Enabled = defensePlugin.Enabled = lootPlugin.Enabled = true;
                attackPlugin.Checked = loadedSave.Enabled("attack"); defensePlugin.Checked = loadedSave.Enabled("defense"); lootPlugin.Checked = loadedSave.Enabled("loot");
                writeSave.Enabled = true;
                saveStatus.Text = complete
                    ? "已读取：\r\n" + file + "\r\n\r\n内购三件套使用共享 settings.sb；写入需先退出游戏。"
                    : "已读取：\r\n" + file + "\r\n\r\n当前存档尚未初始化完整的内购插件字段；点击保存会补齐 attack / defense / loot，并写入三个开关。\r\n缺少：" + loadedSave.MissingFields() + "；发现的 DLC 字段：" + loadedSave.AvailablePluginFields();
            }
            catch (Exception ex) { loadedSave = null; writeSave.Enabled = false; saveStatus.Text = "无法读取：" + ex.Message; }
        }

        private void BuildQuickSaveEditor(Control page)
        {
            Heading(page, "专项快速编辑（上）", "先读取共享设置；下方自由字段表可直接修改金钱 / 信用点、能量节点和大脉冲。这里集中处理三个内购插件开关。", 22);
            LabelText(page, "金钱 / 信用点、大脉冲持有、弹匣和副模式都保留原字段路径；按钮只负责快速筛选，下半区保存时会自动备份并校验。", 30, 88, 680, 42);
            var readArchive = new Button { Text = "读取当前存档", Location = new Point(30, 140), Size = new Size(180, 38) };
            readArchive.Click += delegate { ReadEditorSave(); };
            page.Controls.Add(readArchive);
            var importArchive = new Button { Text = "从备份创建副本", Location = new Point(224, 140), Size = new Size(190, 38) };
            importArchive.Click += delegate { ImportEditorCopy(); };
            page.Controls.Add(importArchive);
            var resources = new Button { Text = "快速定位：金钱 / 信用点与节点", Location = new Point(30, 190), Size = new Size(260, 38) };
            resources.Click += delegate { editorGroup.SelectedIndex = 0; status.Text = "已定位资源与升级字段；请在下方自由字段表修改。"; };
            page.Controls.Add(resources);
            var weapon = new Button { Text = "快速定位：大脉冲", Location = new Point(306, 190), Size = new Size(180, 38) };
            weapon.Click += delegate { editorGroup.SelectedIndex = 1; status.Text = "已定位武器字段；请在下方自由字段表修改大脉冲。"; };
            page.Controls.Add(weapon);
            AddCheck(page, attackPlugin, 246, "攻击增强（attack）", false); attackPlugin.SetBounds(30, 246, 210, 32);
            AddCheck(page, defensePlugin, 246, "防御增强（defense）", false); defensePlugin.SetBounds(250, 246, 210, 32);
            AddCheck(page, lootPlugin, 246, "资源增强（loot）", false); lootPlugin.SetBounds(470, 246, 210, 32);
            var readSave = new Button { Text = "读取共享 settings.sb", Location = new Point(30, 292), Size = new Size(210, 38) };
            readSave.Click += delegate { ReadCurrentSave(); };
            page.Controls.Add(readSave);
            writeSave.Text = "应用全部存档修改"; writeSave.Location = new Point(256, 292); writeSave.Size = new Size(210, 38); writeSave.Enabled = false; writeSave.Visible = false;
            writeSave.Click += delegate { ApplyAllSaveChanges(); };
            page.Controls.Add(writeSave);
            saveStatus.Location = new Point(30, 338); saveStatus.Size = new Size(680, 42);
            saveStatus.Text = "游戏运行时可以读取；请在下方统一应用所有已读取的插件勾选和字段修改，保存前会自动备份。"; page.Controls.Add(saveStatus);
            foreach (Control control in page.Controls)
            {
                if (control is Label && control.Top == 64) control.Height = 44;
                else if (control.Top >= 88) control.Top += 28;
            }
        }

        private Panel Page(Panel host, string name)
        {
            int index = pages.Count;
            var page = new Panel { Dock = DockStyle.Fill, AutoScroll = true, Visible = false };
            page.AutoScrollMinSize = new Size(710, 0);
            pages.Add(page); host.Controls.Add(page);
            var button = new Button { Text = name, TextAlign = ContentAlignment.MiddleLeft,
                Location = new Point(0, index * 43), Size = new Size(184, 38) };
            button.Click += delegate { SelectPage(index); };
            navigation.Add(button); navigationHost.Controls.Add(button); return page;
        }

        private Panel Section(Control host, int top, int height)
        {
            var section = new Panel { Location = new Point(0, top), Size = new Size(710, height), AutoScroll = true, BackColor = Surface };
            host.Controls.Add(section); return section;
        }

        private void SelectPage(int index)
        {
            selectedPageIndex = index;
            for (int i = 0; i < pages.Count; i++)
            {
                pages[i].Visible = i == index;
                if (i == index) pages[i].BringToFront();
                navigation[i].ForeColor = i == index ? Accent : Muted;
                navigation[i].BackColor = i == index ? Surface : Shell;
            }
            UpdateFooterContext();
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
        {
            var label = new Label {
                Text = text, Location = new Point(x, y), Size = new Size(width, height),
                AutoEllipsis = false, AutoSize = false, UseCompatibleTextRendering = false
            };
            Size measured = TextRenderer.MeasureText(text ?? String.Empty, label.Font,
                new Size(width, Int32.MaxValue), TextFormatFlags.WordBreak | TextFormatFlags.NoPadding);
            label.Height = Math.Max(height, measured.Height + 4);
            parent.Controls.Add(label);
        }

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
                if (!(child is Label) || child.ForeColor == SystemColors.ControlText) child.ForeColor = Ink;
                var button = child as Button;
                if (button != null) { button.FlatStyle = FlatStyle.Flat; button.FlatAppearance.BorderColor = Color.FromArgb(48, 72, 87); }
                if (child is TextBox || child is ComboBox || child is NumericUpDown) child.BackColor = Shell;
                Theme(child);
            }
        }
    }
}
