using System;
using System.Collections;
using System.Collections.Generic;
using System.ComponentModel;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Diagnostics;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DeadSpaceInstaller
{
    internal sealed class InstallerForm : Form
    {
        private readonly TextBox programRoot = new TextBox();
        private readonly TextBox gameDataRoot = new TextBox();
        private readonly TextBox resourcePath = new TextBox();
        private readonly CheckBox[] tiers = { new CheckBox(), new CheckBox(), new CheckBox() };
        private readonly CheckBox desktopShortcut = new CheckBox();
        private readonly CheckBox startMenuShortcut = new CheckBox();
        private readonly CheckBox launchAfter = new CheckBox();
        private readonly CheckBox hdChinesePack = new CheckBox();
        private readonly Button languageInfo = new Button();
        private readonly Label space = new Label();
        private readonly Label sourceHint = new Label();
        private readonly TextBox details = new TextBox();
        private readonly Button install = new Button();
        private readonly Button close = new Button();
        private readonly Button languageToggle = new Button();
        private readonly ProgressBar progressBar = new ProgressBar();
        private readonly Panel settings = new Panel();
        private readonly BackgroundWorker worker = new BackgroundWorker();
        private readonly Dictionary<string, long> resourceBytes = new Dictionary<string, long>();
        private readonly TabControl sections = new TabControl();
        private readonly DeadSpaceUi.ProjectAboutPanel aboutPanel = new DeadSpaceUi.ProjectAboutPanel();
        private bool busy;
        private bool markerSource;
        private DeadSpaceResources.IpaProfile sourceProfile;
        private string inspectedSource;
        private long inspectedLength, inspectedWriteTime;
        private readonly Button profileInfo = new Button();
        private InstallOptions activeOptions;

        internal InstallerForm()
        {
            AutoScaleDimensions = new SizeF(96, 96);
            AutoScaleMode = AutoScaleMode.None;
            SuspendLayout();
            Font = new Font("Microsoft YaHei UI", 9.5f);
            Text = I("死亡空间移动版（IOS端高清移植） · 安装与更新");
            Icon = Icon.ExtractAssociatedIcon(System.Reflection.Assembly.GetExecutingAssembly().Location);
            // Keep the primary install flow on one screen. The previous
            // 860x790 window pushed the resource and shortcut choices below
            // an inner scroll area, which made first install look incomplete.
            ClientSize = new Size(1000, 900);
            MinimumSize = new Size(900, 760);
            StartPosition = FormStartPosition.CenterScreen;
            BackColor = Color.FromArgb(10, 17, 24);
            ForeColor = Color.FromArgb(221, 232, 237);

            var shell = new TableLayoutPanel { Dock = DockStyle.Fill, ColumnCount = 1, RowCount = 3, Padding = new Padding(26, 18, 26, 18), BackColor = BackColor, ForeColor = ForeColor };
            shell.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            shell.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            shell.RowStyles.Add(new RowStyle(SizeType.Percent, 100));
            shell.RowStyles.Add(new RowStyle(SizeType.AutoSize));
            Controls.Add(shell);
            var headerRow = new TableLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, MinimumSize = new Size(0,100), ColumnCount = 2, BackColor = BackColor, ForeColor = ForeColor };
            headerRow.ColumnStyles.Add(new ColumnStyle(SizeType.Percent,100)); headerRow.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            var header = new Label { Text = "DEAD SPACE MOBILE", AutoSize = true, Font = new Font("Segoe UI",20,FontStyle.Bold), ForeColor = Color.FromArgb(98,218,232), Margin = new Padding(0,0,18,12) };
            headerRow.Controls.Add(header,0,0);
            var languageRow = Flow(); languageRow.Dock = DockStyle.None; languageRow.Anchor = AnchorStyles.Top | AnchorStyles.Right;
            languageRow.FlowDirection = FlowDirection.TopDown; languageRow.WrapContents = false;
            languageRow.AutoSizeMode = AutoSizeMode.GrowAndShrink;
            languageRow.Controls.Add(new Label { Text = "界面语言 / UI language", AutoSize = true, Margin = new Padding(0,8,8,0) });
            languageToggle.Tag = "ui-language-toggle"; languageToggle.AutoSize = true; languageToggle.MinimumSize = new Size(90,32); languageToggle.FlatStyle = FlatStyle.Flat;
            languageToggle.Click += delegate { DeadSpaceUi.UiLanguage.Toggle(); ApplyUiLanguage(); };
            languageRow.Controls.Add(languageToggle); headerRow.Controls.Add(languageRow,1,0); shell.Controls.Add(headerRow,0,0);
            sections.Dock = DockStyle.Fill; sections.BackColor = BackColor; sections.ForeColor = ForeColor;
            var about = new TabPage(I("00 关于")) { UseVisualStyleBackColor = false, BackColor = BackColor, ForeColor = ForeColor }; about.Controls.Add(aboutPanel); sections.TabPages.Add(about);
            var installPage = new TabPage(I("01 安装")) { UseVisualStyleBackColor = false, BackColor = BackColor, ForeColor = ForeColor }; sections.TabPages.Add(installPage);
            shell.Controls.Add(sections,0,1);
            sections.SelectedIndexChanged += delegate { RefreshPrimaryAction(); };
            settings.Dock = DockStyle.Fill; settings.BackColor = BackColor; settings.ForeColor = ForeColor;
            // Compatibility details can grow for legacy / unknown packages.
            // Keep the install action fixed while the options can scroll.
            settings.AutoScroll = true;
            installPage.Controls.Add(settings);
            var content = new TableLayoutPanel { Dock = DockStyle.Top, AutoSize = true, ColumnCount = 1, Padding = new Padding(0, 0, 18, 12), BackColor = BackColor, ForeColor = ForeColor };
            content.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            settings.Controls.Add(content);
            Note(content, I("安装程序与游戏资源"), true);
            PathRow(content, I("程序位置"), programRoot, delegate { BrowseFolder(programRoot); });
            PathRow(content, I("游戏资源位置"), gameDataRoot, delegate { BrowseFolder(gameDataRoot); });
            Note(content, I("配置与三槽存档保存在文档目录；程序更新保留已有游戏资源和进度。"), false);
            PathRow(content, I("资源来源 · marker / IPA"), resourcePath, BrowseResource);
            sourceHint.AutoSize = true; sourceHint.Dock = DockStyle.Fill; sourceHint.Margin = new Padding(0, 0, 0, 8);
            content.Controls.Add(sourceHint);
            profileInfo.AutoSize = true; profileInfo.FlatStyle = FlatStyle.Flat;
            profileInfo.Click += delegate {
                if (sourceProfile != null) MessageBox.Show(this, sourceProfile.Describe(DeadSpaceUi.UiLanguage.English), "IPA compatibility / IPA 适配", MessageBoxButtons.OK, MessageBoxIcon.Information);
            };
            content.Controls.Add(profileInfo);

            Note(content, I("语言组件 / Language mode"), true);
            var languageChoices = Flow();
            hdChinesePack.Text = I("高清中文组件（对话字幕 + 章节头）");
            hdChinesePack.Checked = true;
            hdChinesePack.AutoSize = true;
            hdChinesePack.Font = new Font(Font, FontStyle.Bold);
            hdChinesePack.ForeColor = Color.FromArgb(98, 218, 232);
            hdChinesePack.BackColor = Color.FromArgb(20, 53, 64);
            hdChinesePack.Padding = new Padding(8, 5, 8, 5);
            hdChinesePack.Margin = new Padding(0, 4, 14, 4);
            languageInfo.Text = I("ⓘ 说明");
            languageInfo.AutoSize = true;
            languageInfo.FlatStyle = FlatStyle.Flat;
            languageInfo.ForeColor = Color.FromArgb(221, 232, 237);
            languageInfo.Margin = new Padding(0, 4, 14, 4);
            languageInfo.Click += delegate { ShowLanguageInfo(); };
            languageChoices.Controls.Add(hdChinesePack);
            languageChoices.Controls.Add(languageInfo);
            content.Controls.Add(languageChoices);
            Note(content, I("关闭后回归原生语言行为：安装版本不启用高清中文对话字幕和章节头；重新运行安装器即可切换。其他语言将在后续开放，欢迎通过 GitHub issue 投稿。"), false);

            Note(content, I("高清纹理 · 可多选"), true);
            var choices = Flow();
            for (int i = 0; i < tiers.Length; i++)
            {
                int index = i;
                tiers[i].Text = (2 + i * 2) + "×" + (i == 1 ? I("  推荐") : "");
                tiers[i].AutoSize = true;
                tiers[i].ForeColor = ForeColor;
                tiers[i].Margin = new Padding(0, 4, 30, 4);
                tiers[i].CheckedChanged += delegate {
                    if (markerSource && tiers[index].Checked && !resourceBytes.ContainsKey((2 + index * 2).ToString())) {
                        tiers[index].Checked = false;
                        sourceHint.Text = I("此 marker 不包含 ") + (2 + index * 2) + I("× 纹理，请选择提供的档位。");
                    }
                    UpdateSpace();
                };
                choices.Controls.Add(tiers[i]);
            }
            tiers[1].Checked = true;
            var selectAll = new Button { Text = I("全选"), AutoSize = true, FlatStyle = FlatStyle.Flat, BackColor = Color.FromArgb(31, 50, 62), ForeColor = ForeColor }; selectAll.FlatAppearance.BorderColor = Color.FromArgb(98, 218, 232);
            selectAll.Click += delegate { for (int i = 0; i < tiers.Length; i++) if (tiers[i].Enabled && (!markerSource || resourceBytes.ContainsKey((2 + i * 2).ToString()))) tiers[i].Checked = true; };
            choices.Controls.Add(selectAll);
            content.Controls.Add(choices);
            Note(content, I("可同时安装多个档位，在启动器中切换使用。全部取消则只装原版资源；已有纹理不会删除。"), false);
            space.AutoSize = true; space.Dock = DockStyle.Fill; space.Margin = new Padding(0, 2, 0, 10);
            content.Controls.Add(space);

            Note(content, I("快捷方式与启动"), true);
            var shortcuts = Flow();
            foreach (var item in new[] { desktopShortcut, startMenuShortcut, launchAfter }) { item.AutoSize = true; item.Margin = new Padding(0, 4, 22, 4); shortcuts.Controls.Add(item); }
            desktopShortcut.Text = I("桌面快捷方式"); desktopShortcut.Checked = true;
            startMenuShortcut.Text = I("开始菜单"); startMenuShortcut.Checked = true;
            launchAfter.Text = I("完成后启动"); launchAfter.Checked = true;
            content.Controls.Add(shortcuts);

            var footer = new TableLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, ColumnCount = 3, Margin = new Padding(0, 12, 0, 0), BackColor = BackColor, ForeColor = ForeColor };
            footer.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100));
            footer.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            footer.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            details.Multiline = true; details.ReadOnly = true; details.ScrollBars = ScrollBars.Vertical;
            details.Dock = DockStyle.Fill; details.Height = 60; details.BackColor = Color.FromArgb(19, 30, 40); details.ForeColor = ForeColor;
            details.Text = I("选择安装位置和资源后，点击开始安装。");
            footer.Controls.Add(details, 0, 0); footer.SetColumnSpan(details, 3);
            progressBar.Dock = DockStyle.Fill; progressBar.Height = 10; progressBar.Margin = new Padding(0, 14, 18, 14);
            footer.Controls.Add(progressBar, 0, 1);
            install.Text = I("开始安装"); install.AutoSize = true; install.MinimumSize = new Size(154, 40); install.FlatStyle = FlatStyle.Flat;
            install.BackColor = Color.FromArgb(98, 218, 232); install.ForeColor = Color.FromArgb(7, 11, 17); install.FlatAppearance.BorderColor = Color.FromArgb(190, 250, 255);
            install.Margin = new Padding(0, 8, 0, 0); install.Click += delegate { if (sections.SelectedIndex==0) { sections.SelectedIndex=1; return; } BeginInstall(); };
            footer.Controls.Add(install, 1, 1);
            close.Text = I("退出"); close.AutoSize = true; close.MinimumSize = new Size(110, 40); close.FlatStyle = FlatStyle.Flat;
            close.BackColor = Color.FromArgb(31, 50, 62); close.ForeColor = ForeColor; close.FlatAppearance.BorderColor = Color.FromArgb(98, 218, 232); close.Margin = new Padding(10, 8, 0, 0); close.DialogResult = DialogResult.Cancel;
            close.Click += delegate { Close(); };
            footer.Controls.Add(close, 2, 1);
            shell.Controls.Add(footer, 0, 2);
            AcceptButton = install;
            CancelButton = close;

            worker.WorkerReportsProgress = true;
            worker.DoWork += delegate(object sender, DoWorkEventArgs e) {
                e.Result = InstallerEngine.Install((InstallOptions)e.Argument, delegate(string message) { worker.ReportProgress(0, message); });
            };
            worker.ProgressChanged += delegate(object sender, ProgressChangedEventArgs e) { details.AppendText(Environment.NewLine + Convert.ToString(e.UserState)); };
            worker.RunWorkerCompleted += delegate(object sender, RunWorkerCompletedEventArgs e) {
                busy = false; sections.Enabled = true; languageToggle.Enabled = true; settings.Enabled = true; install.Enabled = true; Cursor = Cursors.Default;
                progressBar.Style = ProgressBarStyle.Blocks; progressBar.Value = e.Error == null ? 100 : 0;
                if (e.Error != null) { details.AppendText(Environment.NewLine + I("失败：") + e.Error.Message); install.Text = I("重试安装"); return; }
                details.AppendText(Environment.NewLine + (DeadSpaceUi.UiLanguage.English ? "Completed. You can open DeadSpace Launcher.exe." : I("已完成，可打开 DeadSpace Launcher.exe。")) + Environment.NewLine + (DeadSpaceUi.UiLanguage.English ? "Program: " : I("程序：")) + activeOptions.ProgramRoot + Environment.NewLine + (DeadSpaceUi.UiLanguage.English ? "Game data: " : I("资源：")) + activeOptions.GameDataRoot);
                install.Text = I("更新 / 更改安装");
                MessageBox.Show(this,
                    DeadSpaceUi.UiLanguage.English ? "Installation completed. You can now open DeadSpace Launcher.exe." : I("安装完成。现在可以打开 DeadSpace Launcher.exe。"),
                    DeadSpaceUi.UiLanguage.English ? "Installation complete" : I("安装完成"),
                    MessageBoxButtons.OK, MessageBoxIcon.Information);
                if (activeOptions.LaunchAfterInstall) InstallerEngine.Launch(activeOptions.ProgramRoot);
            };
            InstallOptions previous = InstallerEngine.LoadPrevious();
            programRoot.Text = previous.ProgramRoot; gameDataRoot.Text = previous.GameDataRoot;
            for (int i = 0; i < tiers.Length; i++) tiers[i].Checked = Array.IndexOf(previous.Tier.Split(','), (2 + i * 2).ToString()) >= 0;
            resourcePath.TextChanged += delegate { InspectSource(); };
            gameDataRoot.TextChanged += delegate { UpdateSpace(); };
            resourcePath.Text = previous.ResourcePath;
            InspectSource();
            if (InstallerEngine.HasProgramInstallation(previous.ProgramRoot)) { install.Text = I("更新 / 更改安装"); details.Text = I("检测到已有安装。资源来源留空时仅更新程序；选择 marker 可补装其他纹理档。"); }
            ApplyUiLanguage();
            using (var dpiGraphics=CreateGraphics()) { float factor=Math.Max(1f,dpiGraphics.DpiX/96f); if(Math.Abs(factor-1)>0.001f) Scale(new SizeF(factor,factor)); }
            ResumeLayout(true);
            FormClosing += delegate(object sender, FormClosingEventArgs e) { if (busy) { e.Cancel = true; details.AppendText(Environment.NewLine + I("安装仍在进行，请等待校验完成后关闭。")); } };
            Shown += delegate { sections.SelectedIndex=0; RefreshPrimaryAction(); aboutPanel.StartAutomaticCheck(); Rectangle area = Screen.FromControl(this).WorkingArea; MinimumSize = new Size(Math.Min(MinimumSize.Width, area.Width), Math.Min(MinimumSize.Height, area.Height)); if (Height > area.Height) Height = area.Height; if (Width > area.Width) Width = area.Width; };
        }

        private void ApplyUiLanguage()
        {
            InstallerLanguage.Apply(this);
            aboutPanel.ApplyLanguage();
            InspectSource(); UpdateSpace(); RefreshPrimaryAction();
            languageToggle.Text = DeadSpaceUi.UiLanguage.ToggleCaption;
        }

        private void RefreshPrimaryAction()
        {
            if (busy) return;
            install.Text = sections.SelectedIndex==0 ? I("安装选项 →") : I(InstallerEngine.HasProgramInstallation(programRoot.Text) ? "更新 / 更改安装" : "开始安装");
        }
        private static string I(string value) { return InstallerLanguage.Translate(value); }
        private static FlowLayoutPanel Flow() { return new FlowLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, WrapContents = true, Margin = new Padding(0), BackColor = Color.FromArgb(10, 17, 24), ForeColor = Color.FromArgb(221, 232, 237) }; }
        private static LinkLabel CreditLink(string caption, string url) {
            var link = new LinkLabel { Text = caption, AutoSize = true, LinkColor = Color.FromArgb(98, 218, 232), ActiveLinkColor = Color.White, Margin = new Padding(0, 2, 16, 2), Cursor = Cursors.Hand };
            link.Links.Add(0, caption.Length, url);
            link.LinkClicked += delegate(object sender, LinkLabelLinkClickedEventArgs e) {
                try { Process.Start(new ProcessStartInfo { FileName = Convert.ToString(e.Link.LinkData), UseShellExecute = true }); } catch (Exception) { }
            };
            return link;
        }
        private void Note(TableLayoutPanel parent, string text, bool title) {
            var label = new Label { Text = text, Dock = DockStyle.Fill, AutoSize = true, Margin = new Padding(0, title ? 12 : 2, 0, 8) };
            label.ForeColor = title ? ForeColor : Color.FromArgb(142, 163, 176);
            if (title) label.Font = new Font(Font, FontStyle.Bold);
            parent.Controls.Add(label);
        }
        private void PathRow(TableLayoutPanel parent, string title, TextBox field, EventHandler browse) {
            Note(parent, title, false);
            var row = new TableLayoutPanel { Dock = DockStyle.Fill, AutoSize = true, ColumnCount = 2, Margin = new Padding(0, 0, 0, 6), BackColor = BackColor, ForeColor = ForeColor };
            row.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 100)); row.ColumnStyles.Add(new ColumnStyle(SizeType.AutoSize));
            field.Dock = DockStyle.Fill; field.BackColor = Color.FromArgb(19, 30, 40); field.ForeColor = ForeColor; field.Margin = new Padding(0, 4, 12, 0);
            var button = new Button { Text = I("浏览…"), AutoSize = true, FlatStyle = FlatStyle.Flat, BackColor = Color.FromArgb(31, 50, 62), ForeColor = ForeColor, Margin = new Padding(0) }; button.FlatAppearance.BorderColor = Color.FromArgb(98, 218, 232); button.Click += browse;
            row.Controls.Add(field, 0, 0); row.Controls.Add(button, 1, 0); parent.Controls.Add(row);
        }
        private void BrowseFolder(TextBox target) { using (var dialog = new FolderBrowserDialog()) { if (Directory.Exists(target.Text)) dialog.SelectedPath = target.Text; if (dialog.ShowDialog(this) == DialogResult.OK) target.Text = dialog.SelectedPath; } }
        private void BrowseResource(object sender, EventArgs e) { using (var dialog = new OpenFileDialog { Title = I("选择游戏资源"), Filter = I("游戏资源 (*.marker;*.ipa)|*.marker;*.ipa|所有文件 (*.*)|*.*") }) { if (dialog.ShowDialog(this) == DialogResult.OK) resourcePath.Text = dialog.FileName; } }
        private string Selection() { var selected = new List<string>(); for (int i = 0; i < tiers.Length; i++) if (tiers[i].Enabled && tiers[i].Checked && (!markerSource || resourceBytes.ContainsKey((2 + i * 2).ToString()))) selected.Add((2 + i * 2).ToString()); return selected.Count == 0 ? "none" : String.Join(",", selected); }
        private void InspectSource() {
            resourceBytes.Clear(); markerSource = false;
            string path = resourcePath.Text.Trim();
            try {
                if (path.Length == 0) sourceHint.Text = I("已有资源时可留空，仅更新程序。首次安装请选择 marker 或 IPA。");
                else if (!File.Exists(path)) sourceHint.Text = I("找不到此资源文件，请检查路径。");
                else if (Path.GetExtension(path).Equals(".ipa", StringComparison.OrdinalIgnoreCase)) { resourceBytes["ipa"] = new FileInfo(path).Length; sourceHint.Text = I("IPA 仅安装原版资源，不包含高清纹理包。"); }
                else {
                    using (var archive = ZipFile.OpenRead(path)) {
                        var entry = archive.GetEntry("vandal.json"); if (entry == null) throw new InvalidDataException(I("缺少 vandal.json"));
                        if (entry.Length > 16 * 1024 * 1024) throw new InvalidDataException(I("资源清单过大"));
                        Dictionary<string, object> manifest;
                        using (var reader = new StreamReader(entry.Open())) manifest = new JavaScriptSerializer { MaxJsonLength = 16 * 1024 * 1024 }.DeserializeObject(reader.ReadToEnd()) as Dictionary<string, object>;
                        foreach (object value in (IEnumerable)manifest["entries"]) {
                            var row = (Dictionary<string, object>)value; string name = Convert.ToString(row["path"]); string key = Convert.ToString(row["role"]) == "ipa" ? "ipa" : name.StartsWith("payload/texture-packs/") ? name.Split('/')[2] : "other";
                            if (!resourceBytes.ContainsKey(key)) resourceBytes[key] = 0; resourceBytes[key] += Convert.ToInt64(row["size"]);
                        }
                    }
                    markerSource = true; sourceHint.Text = I("marker 只读安装。请选择需要保留在电脑上的纹理档位。");
                }
                if (File.Exists(path)) {
                    var info = new FileInfo(path);
                    if (sourceProfile == null || inspectedSource != info.FullName || inspectedLength != info.Length || inspectedWriteTime != info.LastWriteTimeUtc.Ticks) {
                        sourceProfile = DeadSpaceResources.IpaProfiles.Inspect(path);
                        inspectedSource = info.FullName; inspectedLength = info.Length; inspectedWriteTime = info.LastWriteTimeUtc.Ticks;
                    }
                    string[] summary = sourceProfile.Describe(DeadSpaceUi.UiLanguage.English).Split(new[] { "\r\n" }, StringSplitOptions.None);
                    sourceHint.Text += "\r\n" + String.Join("\r\n", summary, 0, Math.Min(4, summary.Length));
                    if (sourceProfile.Id != "current") sourceHint.Text += "\r\n" + (DeadSpaceUi.UiLanguage.English ? "Developer recommendation: 1.3.34. Other IPA reports are welcome on Community issues." : "开发者推荐 1.3.34；欢迎携带其他 IPA 的版本 / 指纹来 Community 提 issue。");
                    profileInfo.Text = DeadSpaceUi.UiLanguage.English ? "IPA compatibility and graphics limits / issue link" : "查看 IPA 适配、画质范围与 issue 入口";
                    profileInfo.Visible = true;
                    hdChinesePack.Enabled = sourceProfile.HdText;
                    if (!sourceProfile.HdText) hdChinesePack.Checked = false;
                } else { sourceProfile = null; profileInfo.Visible = false; hdChinesePack.Enabled = true; }
            } catch (Exception error) { sourceProfile = null; profileInfo.Visible = false; sourceHint.Text = I("无法读取资源清单：") + error.Message; }
            for (int i = 0; i < tiers.Length; i++) {
                bool missing = resourcePath.Text.Trim().Length > 0 && (!markerSource || !resourceBytes.ContainsKey((2 + i * 2).ToString()) || sourceProfile == null || !sourceProfile.Textures);
                tiers[i].Enabled = !missing;
                tiers[i].Text = (2 + i * 2) + "×" + (missing ? I("（未提供）") : i == 1 ? I("  推荐") : "");
                tiers[i].ForeColor = missing ? Color.FromArgb(142, 163, 176) : ForeColor;
                if (missing) tiers[i].Checked = false;
            }
            UpdateSpace();
        }
        private void UpdateSpace() {
            long bytes = resourceBytes.ContainsKey("ipa") ? resourceBytes["ipa"] : 0;
            foreach (string tier in Selection().Split(',')) if (resourceBytes.ContainsKey(tier)) bytes += resourceBytes[tier];
            string hint = resourcePath.Text.Trim().Length == 0 ? I("仅更新程序；保留已有资源。") : I("所选资源展开约 ") + (bytes / 1073741824.0).ToString("0.00") + I(" GiB（不含约 0.15 GiB 程序；升级所需空间按已有文件变化）。");
            try { var drive = new DriveInfo(Path.GetPathRoot(Path.GetFullPath(gameDataRoot.Text))); hint += I("\r\n资源磁盘可用 ") + (drive.AvailableFreeSpace / 1073741824.0).ToString("0.0") + " GiB。"; } catch (Exception) { }
            space.Text = hint;
        }
        private void BeginInstall() {
            if (busy) return;
            activeOptions = new InstallOptions { ProgramRoot = programRoot.Text.Trim(), GameDataRoot = gameDataRoot.Text.Trim(), ResourcePath = resourcePath.Text.Trim(), Tier = Selection(), CreateShortcut = startMenuShortcut.Checked, CreateDesktopShortcut = desktopShortcut.Checked, RegisterInstallation = true, LaunchAfterInstall = launchAfter.Checked, InstallHdChinese = hdChinesePack.Checked };
            busy = true; sections.Enabled = false; languageToggle.Enabled = false; settings.Enabled = false; install.Enabled = false; Cursor = Cursors.WaitCursor;
            progressBar.Style = ProgressBarStyle.Marquee; details.AppendText(Environment.NewLine + I("开始安装…"));
            worker.RunWorkerAsync(activeOptions);
        }

        private void ShowLanguageInfo()
        {
            string title = DeadSpaceUi.UiLanguage.English ? "Language mode" : I("语言模式说明");
            using (var dialog = new Form { Text = title, ClientSize = new Size(760, 430), MinimumSize = new Size(680, 380), StartPosition = FormStartPosition.CenterParent, FormBorderStyle = FormBorderStyle.FixedDialog, MaximizeBox = false, MinimizeBox = false, BackColor = BackColor, ForeColor = ForeColor })
            {
                var shell = new TableLayoutPanel { Dock = DockStyle.Fill, Padding = new Padding(22), ColumnCount = 2, RowCount = 5 };
                shell.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50)); shell.ColumnStyles.Add(new ColumnStyle(SizeType.Percent, 50));
                shell.RowStyles.Add(new RowStyle(SizeType.AutoSize)); shell.RowStyles.Add(new RowStyle(SizeType.AutoSize)); shell.RowStyles.Add(new RowStyle(SizeType.Percent, 100)); shell.RowStyles.Add(new RowStyle(SizeType.AutoSize)); shell.RowStyles.Add(new RowStyle(SizeType.AutoSize));
                var heading = new Label { Text = DeadSpaceUi.UiLanguage.English ? "Choose the language layer installed with the game" : I("选择随游戏安装的语言层"), AutoSize = true, Font = new Font(dialog.Font, FontStyle.Bold), ForeColor = Color.FromArgb(98, 218, 232), Margin = new Padding(0, 0, 0, 14) };
                shell.Controls.Add(heading, 0, 0); shell.SetColumnSpan(heading, 2);
                var hd = new Label { Text = DeadSpaceUi.UiLanguage.English ? "HD Chinese component" : I("高清中文组件"), AutoSize = true, Font = new Font(dialog.Font, FontStyle.Bold), ForeColor = Color.FromArgb(98, 218, 232), Margin = new Padding(0, 0, 0, 8) };
                var native = new Label { Text = DeadSpaceUi.UiLanguage.English ? "Native behavior" : I("原生行为"), AutoSize = true, Font = new Font(dialog.Font, FontStyle.Bold), ForeColor = Color.FromArgb(221, 232, 237), Margin = new Padding(12, 0, 0, 8) };
                shell.Controls.Add(hd, 0, 1); shell.Controls.Add(native, 1, 1);
                var hdBody = new Label { Text = DeadSpaceUi.UiLanguage.English ? "Chapter 01\r\nHonorable Intentions\r\n\"Watch the locator. It knows this place better than anyone.\"\r\n\r\nGPU-cached Chinese dialogue, objective and chapter cards." : I("第 01 章\r\n当初善意\r\n“注意你的定位器。它比任何人都了解这个地方。”\r\n\r\nGPU 缓存的中文对话、目标和章节卡片。"), Dock = DockStyle.Fill, AutoSize = false, Padding = new Padding(12), BackColor = Color.FromArgb(20, 53, 64), ForeColor = ForeColor, Margin = new Padding(0, 0, 12, 12) };
                var nativeBody = new Label { Text = DeadSpaceUi.UiLanguage.English ? "Chapter 01 / original game text\r\n\r\nThe HD Chinese subtitle and chapter layer is omitted. Re-run this installer to switch back." : I("Chapter 01 / 使用游戏原生文字\r\n\r\n不启用高清中文对话字幕和章节层。重新运行安装器即可切换。"), Dock = DockStyle.Fill, AutoSize = false, Padding = new Padding(12), BackColor = Color.FromArgb(19, 30, 40), ForeColor = ForeColor, Margin = new Padding(0, 0, 0, 12) };
                shell.Controls.Add(hdBody, 0, 2); shell.Controls.Add(nativeBody, 1, 2);
                var note = new Label { Text = DeadSpaceUi.UiLanguage.English ? "More language options will be added later. Please submit requests through GitHub Issues." : I("其他语言选项将在后续开放，欢迎通过 GitHub issue 投稿。"), AutoSize = true, ForeColor = Color.FromArgb(142, 163, 176), Margin = new Padding(0, 6, 0, 10) };
                shell.Controls.Add(note, 0, 3); shell.SetColumnSpan(note, 2);
                var closeInfo = new Button { Text = DeadSpaceUi.UiLanguage.English ? "Close" : I("关闭"), AutoSize = true, DialogResult = DialogResult.OK, Anchor = AnchorStyles.Right, FlatStyle = FlatStyle.Flat, Margin = new Padding(0, 0, 0, 0) };
                shell.Controls.Add(closeInfo, 1, 4); shell.SetColumnSpan(closeInfo, 1);
                dialog.Controls.Add(shell); dialog.AcceptButton = closeInfo; dialog.ShowDialog(this);
            }
        }
    }
}
