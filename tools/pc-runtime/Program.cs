using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.Text;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal static class Program
    {
        [STAThread]
        private static void Main(string[] args)
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            if (args.Length > 0 && args[0] == "--install-resource")
            {
                Environment.ExitCode = LauncherForm.InstallResourceCommand(args);
                return;
            }
            int processId;
            if (args.Length == 2 && args[0] == "--confirm-quit" && int.TryParse(args[1], out processId)) {
                Environment.ExitCode = QuitConfirmation.Run(processId);
                return;
            }
            try { Application.Run(new LauncherForm()); }
            catch (IOException error) { MessageBox.Show("配置或资料目录暂不可用："+error.Message+"\n原有文件已保留。请检查文档目录权限或资料版本。", "资料目录", MessageBoxButtons.OK, MessageBoxIcon.Error); }
            catch (UnauthorizedAccessException error) { MessageBox.Show("文档目录没有写入权限："+error.Message, "资料目录", MessageBoxButtons.OK, MessageBoxIcon.Error); }
        }
    }

    internal sealed partial class LauncherForm : Form
    {
        private readonly string baseDir = AppDomain.CurrentDomain.BaseDirectory;
        private readonly string iniPath;
        private readonly string documentsDirectory;

        private readonly TextBox ipaPath = new TextBox();
        private readonly ComboBox scaleHack = new NoWheelComboBox();
        private readonly ComboBox displayResolution = new NoWheelComboBox();
        private readonly ComboBox windowMode = new NoWheelComboBox();
        private readonly ComboBox fpsCap = new NoWheelComboBox();
        private readonly ComboBox fsrMode = new NoWheelComboBox();
        private readonly ComboBox mouseLook = new NoWheelComboBox();
        private readonly ComboBox lookDirection = new NoWheelComboBox();
        private readonly TextBox aimPoint = new TextBox();
        private readonly TextBox stickGain = new TextBox();
        private readonly TextBox lookGain = new TextBox();
        private readonly ComboBox mouseAimMode = new NoWheelComboBox();
        private readonly ComboBox padAimMode = new NoWheelComboBox();
        private readonly ComboBox padFireMode = new NoWheelComboBox();
        private readonly ComboBox keyboardAltMode = new NoWheelComboBox();
        private readonly CheckBox nativeInputs = new CheckBox();
        private readonly CheckBox aimedWalk = new CheckBox();
        private readonly NumericUpDown aimedForwardPercent = new NumericUpDown();
        private readonly NumericUpDown lookSensitivity = new NumericUpDown();
        private readonly CheckBox invertMouseX = new CheckBox();
        private readonly CheckBox invertMouseY = new CheckBox();
        private readonly CheckBox releaseF8 = new CheckBox();
        private readonly CheckBox releaseAlt = new CheckBox();
        private readonly CheckBox releaseWin = new CheckBox();
        private bool syncingPointerOptions;

        private readonly CheckBox fastPresent = new CheckBox();
        private readonly CheckBox safeBlackFloor = new CheckBox();
        private readonly TrackBar safeBlackGray = new TrackBar();
        private readonly Label safeBlackGrayValue = new Label();
        private readonly CheckBox skipFinish = new CheckBox();
        private readonly ComboBox textureSync = new NoWheelComboBox();
        private readonly CheckBox gpuPresentCopy = new CheckBox();
        private readonly CheckBox gpuTextOverlay = new CheckBox();
        private readonly CheckBox pcCompat = new CheckBox();
        private readonly CheckBox openingMips = new CheckBox();
        private readonly CheckBox globalMips = new CheckBox();
        private readonly CheckBox disableFog = new CheckBox();
        private readonly CheckBox screenFogMask = new CheckBox();
        private readonly CheckBox sharpen = new CheckBox();
        private readonly CheckBox pcMouse = new CheckBox();
        private readonly CheckBox debugLogs = new CheckBox();
        private readonly CheckBox frameDump = new CheckBox();
        private readonly CheckBox nativeHud = new CheckBox();
        private readonly CheckBox overlayEnabled = new CheckBox();
        private readonly ComboBox subtitleMode = new NoWheelComboBox();
        private readonly CheckBox confirmQuit = new CheckBox();
        private readonly CheckBox superMonitor = new CheckBox();

        private readonly Label status = new DeadSpaceUi.UiLanguage.LocalizedLabel();
        private readonly Button languageToggle = new Button();

        public LauncherForm() : this(AppDomain.CurrentDomain.BaseDirectory,
            Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), DisplayHardware.Detect()) { }
        internal LauncherForm(string packageRoot,string documents,DisplayHardware hardware)
        {
            baseDir=Path.GetFullPath(packageRoot);documentsDirectory=Path.GetFullPath(documents);detectedHardware=hardware;
            bool firstConfiguration;
            iniPath = DocumentsSettings.Resolve(baseDir, documentsDirectory, out firstConfiguration);
            Text = "死亡空间移动版（IOS端高清移植） · 启动器";
            Icon embeddedIcon = LoadEmbeddedIcon();
            Icon = embeddedIcon ?? Icon.ExtractAssociatedIcon(System.Reflection.Assembly.GetExecutingAssembly().Location);
            StartPosition = FormStartPosition.CenterScreen;
            AutoScaleDimensions = new SizeF(96, 96);
            AutoScaleMode = AutoScaleMode.None;
            SuspendLayout();
            ClientSize = new Size(1000, 740);
            // The page controls are intentionally designed for a 1000px
            // content lane. Allowing the old 760px minimum clipped the right
            // column, language toggle and footer actions instead of reflowing
            // them. Pages still scroll vertically for short displays.
            MinimumSize = new Size(1000, 620);
            Font = new Font("Microsoft YaHei UI", 9.5f);
            BackColor = Color.FromArgb(236, 241, 245);

            BuildShell();
            BuildLauncherExitButton();
            textureSync.SelectedIndex = 0;
            gpuPresentCopy.Checked = true;
            LoadSettings();
            LoadKeyBindings();
            LoadGamepadBindings();
            if(firstConfiguration){ApplyDetectedGraphicsDefaults();SaveSettings();}
            WatchGame();
            ApplyUiLanguage();
            InitializeResponsiveLayout();
            ResumeLayout(true);
            Shown += delegate {
                Rectangle area = Screen.FromControl(this).WorkingArea;
                MinimumSize = new Size(Math.Min(MinimumSize.Width, area.Width), Math.Min(MinimumSize.Height, area.Height));
                if (Height > area.Height) Height = area.Height;
                if (Width > area.Width) Width = area.Width;
                aboutPanel.StartAutomaticCheck();
                EnsureMarkerReady(false);
            };
        }

        private void ApplyUiLanguage()
        {
            DeadSpaceUi.UiLanguage.Apply(this);
            languageToggle.Text = DeadSpaceUi.UiLanguage.ToggleCaption;
            if (aboutPanel != null) aboutPanel.ApplyLanguage();
            if (resourceProfile != null && File.Exists(loadedResourceIpa)) ApplyResourceCompatibility(loadedResourceIpa, false);
            LayoutLauncher();
        }

        private static void AddComboRow(Control parent, string label, int y, ComboBox combo, string[] items)
        {
            parent.Controls.Add(new Label { Text = label, Location = new Point(30, y + 4), Size = new Size(160, 28), AutoEllipsis = false });
            combo.DropDownStyle = ComboBoxStyle.DropDownList;
            combo.Items.AddRange(items);
            combo.Location = new Point(194, y);
            combo.Size = new Size(210, 28);
            parent.Controls.Add(combo);
        }

        private static void AddTextRow(Control parent, string label, int y, TextBox textBox)
        {
            parent.Controls.Add(new Label { Text = label, Location = new Point(30, y + 4), Size = new Size(160, 28), AutoEllipsis = false });
            textBox.Location = new Point(194, y);
            textBox.Size = new Size(210, 28);
            parent.Controls.Add(textBox);
        }

        private static void AddCheck(Control parent, CheckBox checkBox, int y, string text, bool defaultValue)
        {
            checkBox.Text = text;
            checkBox.Location = new Point(30, y);
            checkBox.Size = new Size(665, 32);
            checkBox.AutoEllipsis = false;
            checkBox.AutoSize = false;
            checkBox.TextAlign = ContentAlignment.MiddleLeft;
            checkBox.Checked = defaultValue;
            parent.Controls.Add(checkBox);
        }

        private void BrowseIpa()
        {
            using (var dialog = new OpenFileDialog())
            {
                dialog.Title = "选择 Dead Space IPA";
                dialog.Filter = "iOS 应用包 (*.ipa)|*.ipa|所有文件 (*.*)|*.*";
                string current = ipaPath.Text.Trim();
                if (current.Length > 0)
                {
                    string directory = Path.GetDirectoryName(current);
                    if (!string.IsNullOrEmpty(directory) && Directory.Exists(directory))
                    {
                        dialog.InitialDirectory = directory;
                    }
                }
                if (dialog.ShowDialog(this) == DialogResult.OK)
                {
                    directIpaSource = dialog.FileName;
                    markerSourcePath = String.Empty;
                    UpdateMarkerStatus();
                    status.Text = "已选择 IPA；点击“安装游戏资源”继续。";
                }
            }
        }

        internal static int InstallResourceCommand(string[] args)
        {
            try
            {
                string marker = null;
                string ipa = null;
                string root = null;
                string tier = "4";
                string result = null;
                for (int index = 1; index < args.Length; index += 2)
                {
                    if (index + 1 >= args.Length)
                        throw new ArgumentException("missing option value");
                    switch (args[index])
                    {
                        case "--marker": marker = args[index + 1]; break;
                        case "--ipa": ipa = args[index + 1]; break;
                        case "--install-root": root = args[index + 1]; break;
                        case "--tier":
                        case "--tiers": tier = args[index + 1]; break;
                        case "--result": result = args[index + 1]; break;
                        default: throw new ArgumentException("unknown option");
                    }
                }
                if (String.IsNullOrWhiteSpace(root))
                    throw new ArgumentException("missing install root");
                if ((marker == null) == (ipa == null))
                    throw new ArgumentException("select exactly one source");
                string installed = marker != null
                    ? VandalMarker.InstallSelected(
                        VandalMarker.Read(marker), root, VandalMarker.ParseTiers(tier))
                    : VandalMarker.InstallIpa(ipa, root);
                DeadSpaceResources.ResourceSelection.Activate(root, installed);
                string report = result ?? Path.Combine(
                    installed, "installer-result.json");
                Directory.CreateDirectory(Path.GetDirectoryName(
                    Path.GetFullPath(report)));
                File.WriteAllText(report,
                    "{\"status\":\"installed\",\"installed_root\":"
                        + new System.Web.Script.Serialization.JavaScriptSerializer()
                            .Serialize(installed)
                        + ",\"source\":"
                        + new System.Web.Script.Serialization.JavaScriptSerializer()
                            .Serialize(marker ?? ipa)
                        + "}");
                return 0;
            }
            catch (Exception error)
            {
                Console.Error.WriteLine(error.Message);
                return 2;
            }
        }

        private string FindDefaultIpa()
        {
            string[] candidates =
            {
                Path.Combine(DistributionLayout.Game(baseDir), "DeadSpace.ipa"),
                Path.Combine(baseDir, "DeadSpace.ipa"),
                Path.Combine(baseDir, "Dead Space-v1.3.34.ipa"),
                Path.Combine(baseDir, "touchHLE_apps", "DeadSpace.ipa"),
                Path.Combine(baseDir, "touchHLE_apps", "DeadSpace-ipad2.ipa")
            };
            foreach (string candidate in candidates)
            {
                if (File.Exists(candidate))
                {
                    return candidate;
                }
            }
            return "";
        }

        private void ApplyDefaults()
        {
            ResetKeyBindings();
            ResetGamepadBindings();
            inputMode.SelectedIndex = 0;
            saveMode.SelectedIndex = 1;
            string existing = ipaPath.Text.Trim();
            SetCombo(scaleHack, AutomaticRenderScale);
            ResetTextureSettings();
            displayResolution.SelectedIndex = 0;
            windowMode.SelectedIndex = 1;
            fpsCap.SelectedIndex = 3;
            fsrMode.SelectedIndex = 1;
            mouseLook.SelectedIndex = 2;
            lookDirection.SelectedIndex = 1;
            releaseF8.Checked = releaseAlt.Checked = releaseWin.Checked = true;
            padInvertX.Checked = padInvertY.Checked = false; padLookSensitivity.Value = 100; padLookDeadzone.Value = 14;
            lookGain.Text = "1.0";
            aimPoint.Text = "360,200";
            stickGain.Text = "2.0";
            installRoot.Text = DefaultInstallRoot();
            fastPresent.Checked = true;
            safeBlackFloor.Checked = false;
            safeBlackGray.Value = 10;
            skipFinish.Checked = false;
            gpuPresentCopy.Checked = true;
            pcCompat.Checked = true;
            openingMips.Checked = true;
            globalMips.Checked = false;
            disableFog.Checked = false;
            screenFogMask.Checked = false;
            sharpen.Checked = true;
            pcMouse.Checked = true;
            debugLogs.Checked = false;
            frameDump.Checked = false;
            nativeHud.Checked = true;
            overlayEnabled.Checked = true;
            advancedLighting.Checked = true;
            subtitleMode.SelectedIndex=0;
            confirmQuit.Checked = true;
            superMonitor.Checked = false;
            mouseAimMode.SelectedIndex = padAimMode.SelectedIndex = 0;
            padFireMode.SelectedIndex = 1; keyboardAltMode.SelectedIndex = 0;
            nativeInputs.Checked = aimedWalk.Checked = true; aimedForwardPercent.Value = 100;
            ipaPath.Text = existing.Length > 0 ? existing : FindDefaultIpa();
            ApplyDetectedGraphicsDefaults();
        }

        private void LoadSettings()
        {
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            if (File.Exists(iniPath))
            {
                foreach (string raw in File.ReadAllLines(iniPath, Encoding.UTF8))
                {
                    int split = raw.IndexOf('=');
                    if (split > 0)
                    {
                        values[raw.Substring(0, split).Trim()] = raw.Substring(split + 1).Trim();
                    }
                }
            }

            ipaPath.Text = Get(values, "ipa", FindDefaultIpa());
            LoadTextureSettings(values);
            LoadSamplingSettings(values);
            SetCombo(scaleHack, Get(values, "scale", AutomaticRenderScale));
            EnsureResolutionOption(AutomaticResolution);
            string savedResolution = Get(values, "display_resolution", AutomaticResolution);
            if (!DeadSpaceUi.UiLanguage.EquivalentLabel(savedResolution,"跟随内部") && !DeadSpaceUi.UiLanguage.EquivalentLabel(savedResolution,AutomaticResolution)) EnsureResolutionOption(savedResolution);
            SetCombo(displayResolution, savedResolution);
            SetCombo(windowMode, Get(values, "window_mode", "无边框全屏"));
            SetCombo(saveMode, Get(values, "save_mode", "文档（当前用户）"));
            bool manualDiagnosticPolicy=Get(values,"diagnostic_defaults_policy","")=="manual-f7-20261107";
            playDiagnostics.Checked=manualDiagnosticPolicy && Get(values,"play_diagnostics","0")=="1";
            int detail;diagnosticDetail.SelectedIndex=Int32.TryParse(Get(values,"diagnostic_detail","2"),out detail)&&detail>=0&&detail<=2?detail:2;
            SetCombo(inputMode, Get(values, "input_mode", "自动（键鼠 + 手柄）"));
            SetCombo(fpsCap, Get(values, "fps", "不限"));
            SetCombo(fsrMode, Get(values, "fsr", "FSR1 EASU + RCAS"));
            SetCombo(mouseLook, Get(values, "mouse_look", "始终捕获"));
            SetCombo(lookDirection, Get(values, "look_direction", "标准"));
            invertMouseX.Checked = GetBool(values, "look_invert_x", invertMouseX.Checked);
            invertMouseY.Checked = GetBool(values, "look_invert_y", invertMouseY.Checked);
            releaseF8.Checked = GetBool(values, "release_f8", true);
            releaseAlt.Checked = GetBool(values, "release_alt", true);
            releaseWin.Checked = GetBool(values, "release_win", true);
            padInvertX.Checked = GetBool(values, "pad_look_invert_x", false);
            padInvertY.Checked = GetBool(values, "pad_look_invert_y", false);
            int padGain, padDeadzone;
            if (!Int32.TryParse(Get(values, "pad_look_sensitivity", "100"), out padGain)) padGain = 100;
            if (!Int32.TryParse(Get(values, "pad_look_deadzone", "14"), out padDeadzone)) padDeadzone = 14;
            padLookSensitivity.Value = Math.Min(500, Math.Max(5, padGain));
            padLookDeadzone.Value = Math.Min(50, Math.Max(0, padDeadzone));
            lookGain.Text = Get(values, "look_gain", "1.0");
            aimPoint.Text = Get(values, "aim_xy", "360,200");
            stickGain.Text = Get(values, "stick_gain", "2.0");
            installRoot.Text = Get(values, "install_root", DefaultInstallRoot());
            fastPresent.Checked = GetBool(values, "fast_present", true);
            safeBlackFloor.Checked = GetBool(values, "safe_black_floor", false);
            int safeGray;
            if (!Int32.TryParse(Get(values, "safe_black_gray", "10"), out safeGray)) safeGray = 10;
            safeBlackGray.Value = Math.Min(safeBlackGray.Maximum, Math.Max(safeBlackGray.Minimum, safeGray));
            skipFinish.Checked = GetBool(values, "skip_finish", false);
            int textureSyncIndex;
            if (!Int32.TryParse(Get(values, "texture_sync_index", "0"), out textureSyncIndex))
                textureSyncIndex = 0;
            textureSync.SelectedIndex = Math.Min(2, Math.Max(0, textureSyncIndex));
            gpuPresentCopy.Checked = GetBool(values, "gpu_present_copy", true);
            // Missing legacy keys use the same GPU default as first setup.
            // An explicit 0 still selects the CPU compatibility renderer.
            gpuTextOverlay.Checked = GetBool(values, "gpu_text_overlay", true);
            pcCompat.Checked = GetBool(values, "pc_compat", true);
            openingMips.Checked = GetBool(values, "opening_mips", true);
            globalMips.Checked = GetBool(values, "global_mips", false);
            disableFog.Checked = GetBool(values, "disable_fog", false);
            screenFogMask.Checked = GetBool(values, "screen_fog_mask", false);
            sharpen.Checked = GetBool(values, "sharpen", true);
            pcMouse.Checked = GetBool(values, "pc_mouse", true);
            debugLogs.Checked = GetBool(values, "debug_logs", false);
            frameDump.Checked = GetBool(values, "frame_dump", false);
            nativeHud.Checked = GetBool(values, "native_hud", true);
            overlayEnabled.Checked = GetBool(values, "overlay_enabled", true);
            advancedLighting.Checked = GetBool(values, "advanced_lighting", true);
            launchReviewCompleted = GetBool(values, "launch_review_r4_completed", false);
            SetCombo(subtitleMode,Get(values,"subtitle_mode","整段三行"));
            confirmQuit.Checked = GetBool(values, "confirm_quit", true);
            superMonitor.Checked = manualDiagnosticPolicy && GetBool(values, "super_monitor", false);
            mouseAimMode.SelectedIndex = Get(values, "mouse_aim_mode", "hold") == "toggle" ? 1 : 0;
            padAimMode.SelectedIndex = Get(values, "pad_aim_mode", "hold") == "toggle" ? 1 : 0;
            padFireMode.SelectedIndex = Get(values, "pad_fire_mode", "toggle") == "split" ? 0 : 1;
            keyboardAltMode.SelectedIndex = Get(values, "keyboard_alt_mode", "hold") == "toggle" ? 1 : 0;
            nativeInputs.Checked = GetBool(values, "native_inputs", true);
            aimedWalk.Checked = GetBool(values, "aimed_walk", true);
            int walkPercent; if (!Int32.TryParse(Get(values, "aimed_forward_percent", "100"), out walkPercent)) walkPercent = 100;
            aimedForwardPercent.Value = Math.Min(100, Math.Max(5, walkPercent));
            if (values.Count == 0) ApplyDetectedGraphicsDefaults();
            string resolvedIpa = DistributionLayout.ResolveIpa(baseDir, ipaPath.Text);
            if (File.Exists(resolvedIpa)) ApplyResourceCompatibility(resolvedIpa, false);
            RefreshRenderResolutionSummary();
        }

        private static string Get(Dictionary<string, string> values, string key, string fallback)
        {
            string value;
            return values.TryGetValue(key, out value) ? value : fallback;
        }

        private static bool GetBool(Dictionary<string, string> values, string key, bool fallback)
        {
            string value;
            if (!values.TryGetValue(key, out value))
            {
                return fallback;
            }
            return value == "1" || value.Equals("true", StringComparison.OrdinalIgnoreCase);
        }

        private static void SetCombo(ComboBox combo, string value)
        {
            int index=-1;
            for(int i=0;i<combo.Items.Count;i++)
                if(DeadSpaceUi.UiLanguage.EquivalentLabel(Convert.ToString(combo.Items[i]),value)){index=i;break;}
            combo.SelectedIndex = index >= 0 ? index : 0;
        }

        private void SaveSettings()
        {
            var lines = new[]
            {
                "ipa=" + SaveIpaReference().Trim(),
                "hardware_defaults_version=4",
                "scale=" + scaleHack.SelectedItem,
                "display_resolution=" + displayResolution.SelectedItem,
                "window_mode=" + windowMode.SelectedItem,
                "save_mode=" + saveMode.SelectedItem,
                "input_mode=" + inputMode.SelectedItem,
                "fps=" + fpsCap.SelectedItem,
                "fsr=" + fsrMode.SelectedItem,
                "mouse_look=" + mouseLook.SelectedItem,
                "look_direction=" + lookDirection.SelectedItem,
                "look_invert_x=" + (invertMouseX.Checked ? "1" : "0"),
                "look_invert_y=" + (invertMouseY.Checked ? "1" : "0"),
                "release_f8=" + (releaseF8.Checked ? "1" : "0"),
                "release_alt=" + (releaseAlt.Checked ? "1" : "0"),
                "release_win=" + (releaseWin.Checked ? "1" : "0"),
                "pad_look_invert_x=" + (padInvertX.Checked ? "1" : "0"),
                "pad_look_invert_y=" + (padInvertY.Checked ? "1" : "0"),
                "pad_look_sensitivity=" + padLookSensitivity.Value.ToString(System.Globalization.CultureInfo.InvariantCulture),
                "pad_look_deadzone=" + padLookDeadzone.Value.ToString(System.Globalization.CultureInfo.InvariantCulture),
                "look_gain=" + lookGain.Text.Trim(),
                "aim_xy=" + aimPoint.Text.Trim(),
                "stick_gain=" + stickGain.Text.Trim(),
                "install_root=" + installRoot.Text.Trim(),
                "fast_present=" + (fastPresent.Checked ? "1" : "0"),
                "safe_black_floor=" + (safeBlackFloor.Checked ? "1" : "0"),
                "safe_black_gray=" + safeBlackGray.Value,
                "skip_finish=" + (skipFinish.Checked ? "1" : "0"),
                "texture_sync_index=" + textureSync.SelectedIndex,
                "gpu_present_copy=" + (gpuPresentCopy.Checked ? "1" : "0"),
                "gpu_text_overlay=" + (gpuTextOverlay.Checked ? "1" : "0"),
                "pc_compat=" + (pcCompat.Checked ? "1" : "0"),
                "opening_mips=" + (openingMips.Checked ? "1" : "0"),
                "global_mips=" + (globalMips.Checked ? "1" : "0"),
                "disable_fog=" + (disableFog.Checked ? "1" : "0"),
                "screen_fog_mask=" + (screenFogMask.Checked ? "1" : "0"),
                "sharpen=" + (sharpen.Checked ? "1" : "0"),
                "pc_mouse=" + (pcMouse.Checked ? "1" : "0"),
                "debug_logs=" + (debugLogs.Checked ? "1" : "0"),
                "frame_dump=" + (frameDump.Checked ? "1" : "0"),
                "native_hud=" + (nativeHud.Checked ? "1" : "0"),
                "overlay_enabled=" + (overlayEnabled.Checked ? "1" : "0"),
                "advanced_lighting=" + (advancedLighting.Checked ? "1" : "0"),
                "launch_review_r4_completed=" + (launchReviewCompleted ? "1" : "0"),
                "subtitle_mode=" + subtitleMode.SelectedItem,
                "confirm_quit=" + (confirmQuit.Checked ? "1" : "0"),
                "super_monitor=" + (superMonitor.Checked ? "1" : "0"),
                "mouse_aim_mode=" + (mouseAimMode.SelectedIndex == 1 ? "toggle" : "hold"),
                "pad_aim_mode=" + (padAimMode.SelectedIndex == 1 ? "toggle" : "hold"),
                "pad_fire_mode=" + (padFireMode.SelectedIndex == 0 ? "split" : "toggle"),
                "keyboard_alt_mode=" + (keyboardAltMode.SelectedIndex == 1 ? "toggle" : "hold"),
                "native_inputs=" + (nativeInputs.Checked ? "1" : "0"),
                "aimed_walk=" + (aimedWalk.Checked ? "1" : "0"),
                "aimed_forward_percent=" + aimedForwardPercent.Value.ToString(System.Globalization.CultureInfo.InvariantCulture)
            };
            var allLines = new List<string>(lines);
            SaveTextureSettings(allLines);
            SaveSamplingSettings(allLines);
            allLines.Add("play_diagnostics="+(playDiagnostics.Checked?"1":"0"));
            allLines.Add("diagnostic_defaults_policy=manual-f7-20261107");
            allLines.Add("diagnostic_detail="+diagnosticDetail.SelectedIndex.ToString(System.Globalization.CultureInfo.InvariantCulture));
            DocumentsSettings.Save(iniPath, allLines.ToArray());
        }

        private void ApplyFireModeEnvironment(ProcessStartInfo info)
        {
            info.EnvironmentVariables["TOUCHHLE_DS_CONTROLLER_FIRE_MODE"] = padFireMode.SelectedIndex == 0 ? "split" : "toggle";
            info.EnvironmentVariables["TOUCHHLE_DS_KEYBOARD_ALT_MODE"] = keyboardAltMode.SelectedIndex == 1 ? "toggle" : "hold";
        }

        private void PrepareDefaultOptimizationEnvironment(ProcessStartInfo info)
        {
            // The combined PC package ships the verified/candidate paths enabled
            // by default. Low-hit Keyframe/Matrix4 inverse caches remain explicit
            // script candidates because their miss-side JIT invalidation regressed
            // sustained play. Keep trigger files outside the program directory so
            // upgrades and rollbacks never rewrite the installed package.
            string stateRoot = Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "DeadSpaceLegacy");
            Directory.CreateDirectory(stateRoot);
            string skinTrigger = Path.Combine(stateRoot, "all-pending-skin.trigger");
            string transformTrigger = Path.Combine(stateRoot, "all-pending-transform.trigger");
            string positionTrigger = Path.Combine(stateRoot, "all-pending-animation-position.trigger");
            foreach (string trigger in new[] { skinTrigger, transformTrigger, positionTrigger })
                File.WriteAllText(trigger, "all-pending", Encoding.ASCII);

            info.EnvironmentVariables["TOUCHHLE_DS_PC_JIT_PREWARM"] = "1";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_COMBAT_LATENCY"] = "1";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_SKIN_SHADOW"] = "1";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_SKIN_MATH"] = "host";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_SKIN_INPUT"] = "strided";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_SKIN_TRIGGER"] = skinTrigger;
            info.EnvironmentVariables["TOUCHHLE_DS_PC_TRANSFORM_MATH"] = "host";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_TRANSFORM_TRIGGER"] = transformTrigger;
            info.EnvironmentVariables["TOUCHHLE_DS_PC_TRANSFORM_CACHE"] = "1";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_ANIMATION_POSITION_CACHE"] = "1";
            info.EnvironmentVariables["TOUCHHLE_DS_PC_ANIMATION_POSITION_TRIGGER"] = positionTrigger;
            // These are already the Windows Dead Space defaults, but setting them
            // in the child process makes the combined package deterministic even
            // when the parent shell inherited an explicit rollback value.
            info.EnvironmentVariables["TOUCHHLE_DS_GL_STACK_DRIVER"] = "1";
            info.EnvironmentVariables["TOUCHHLE_DS_GL_ENV_CACHE"] = "1";
        }

        private void LaunchGame()
        {
            if (diagnosticExporting || RefreshGameState()) return;
            detectedHardware.RefreshDisplayResolution();
            RefreshRenderResolutionSummary();
            string ipa = ipaPath.Text.Trim();
            try { ipa = DistributionLayout.ResolveIpa(baseDir, ipa); }
            catch (Exception error) { status.Text = "游戏文件路径无效：" + error.Message; return; }
            string gameExe = Path.Combine(DistributionLayout.Game(baseDir), "touchHLE.exe");
            if (!File.Exists(gameExe))
            {
                status.Text = "缺少游戏运行文件：" + gameExe;
                return;
            }
            if (!File.Exists(ipa))
            {
                status.Text = "请选择有效的 Dead Space IPA";
                return;
            }

            string texturePath;
            try { ApplyResourceCompatibility(ipa, false); }
            catch (Exception error) { status.Text = error.Message; return; }
            try { texturePath = SelectedTextureRoot(); }
            catch (Exception error) { status.Text = error.Message; return; }
            if (!ReviewFirstLaunch()) return;
            using (var warning = new FlickerWarningForm())
                if (warning.ShowDialog(this) != DialogResult.OK) return;
            try { SaveKeyBindings(); SaveGamepadBindings(); }
            catch (Exception error) { status.Text = error.Message; return; }
            try { SaveStorage.RequireStopped(); SaveStorage.EnsureWritable(SelectedDataRoot()); if(saveMode.SelectedIndex==1)DocumentsSettings.EnsureLayout(SelectedDataRoot()); }
            catch (Exception error) { status.Text = "存档目录不可用：" + error.Message; return; }
            SaveSettings();
            // Visual repairs must accompany every launch, including direct EXE
            // starts. Do not depend on the environment of a verification script.
            VisualCompatibility.Apply();
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_NATIVE_EFFECTS",advancedLighting.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_SUPER_MONITOR",superMonitor.Checked ? "1" : null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_PERF_TRACE",superMonitor.Checked ? "1" : null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_RUN_ID",Path.GetFileName(baseDir.TrimEnd(Path.DirectorySeparatorChar)) + ":" + RuntimeBuildTag());
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_KEY_BINDINGS", ConfigurationFile("DeadSpace-Keys.ini"));
            int renderScale; string outputSize;
            try { renderScale = SelectedRenderScale(); outputSize = SelectedOutputSize(); }
            catch (InvalidOperationException error) { status.Text = error.Message; return; }
            int cap = fpsCap.SelectedIndex == 0 ? 30 : (fpsCap.SelectedIndex == 1 ? 60 : (fpsCap.SelectedIndex == 2 ? 70 : 0));
            string requestedWindowMode = windowMode.SelectedIndex == 1 ? "borderless"
                : windowMode.SelectedIndex == 2 ? "exclusive" : "window";

            Environment.SetEnvironmentVariable("TOUCHHLE_DS_SCREEN_SCALE", "2");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_PRESENT_SCALE", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_WINDOW_MODE", requestedWindowMode);
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_CAPTURE_SAFE_FULLSCREEN",
                requestedWindowMode == "window" ? null : "1");
            if (outputSize == null)
            {
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_OUTPUT_WIDTH", null);
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_OUTPUT_HEIGHT", null);
            }
            else
            {
                string[] parts = outputSize.Split('x');
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_OUTPUT_WIDTH", parts[0]);
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_OUTPUT_HEIGHT", parts[1]);
            }
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_DEPTH_SIZE", "24");
            // AA is a user quality choice, not an implicit depth-precision fix.
            // VisualCompatibility's PC_DEPTH24 also covers MSAA=off: the guest
            // may replace the drawable depth with its own D16 renderbuffer.
            ApplySamplingSettings();
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MSAA_CLEAR_DEPTH", "1");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MSAA_FBO_DEPTH", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MSAA_DRAWABLE_ONLY", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_HOLD_LAST_ON_BLACK", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_FORCE_NO_CULL", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_FORCE_CULL_FACE", null);
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_NO_CULL_HASH_COUNT",
                "02d3486c:405");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_DEPTH_ALWAYS_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CLEAR_DEPTH_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_DEPTH_ALWAYS_TRI_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_FLIP_WINDING_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_NO_CULL_TRI_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_UNLIT_BACKFACE_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_TEXMAT_IDENTITY_HASH_COUNT", null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CLEAR_OPENING_DEPTH", null);
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_DST_FILL_HASH_COUNT",
                null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CG_GLYPH_SCALE", "0.68");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CG_GLYPH_DY", "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_ALPHA_AS_WHITE", "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_PRESENT_SHARPEN", sharpen.Checked ? "1.5" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_PRESENT_FSR",
                fsrMode.SelectedIndex > 0 ? fsrMode.SelectedIndex.ToString() : null);

            if (pcCompat.Checked)
            {
                // The asteroid d1feb687:600 must retain depth writes and GL_LESS.
                // Keep gameplay on the guest's GL_LESS. Only the full-screen
                // opening background atlases need the later-layer tie break.
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_DEPTH_LEQUAL", null);
                string depthLequalHash =
                    "25a08a82,cee43546,36497762,1d5cccb8,8c12a704,87127d0f,e70efc97,2131afd5,18c949fb,f4d178fa";
                if (Environment.GetEnvironmentVariable(
                    "TOUCHHLE_DS_DIAG_PRESERVE_DEPTH_HASH") == "1")
                {
                    string diagnosticDepthHash = Environment.GetEnvironmentVariable(
                        "TOUCHHLE_DS_DEPTH_LEQUAL_HASH");
                    if (!string.IsNullOrWhiteSpace(diagnosticDepthHash))
                    {
                        depthLequalHash = diagnosticDepthHash;
                    }
                }
                Environment.SetEnvironmentVariable(
                    "TOUCHHLE_DS_DEPTH_LEQUAL_HASH",
                    depthLequalHash);
                // The background stack must not write depth after winning a
                // tie, or its black layers cut triangular holes through the
                // cockpit and distant safety posters.
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_DEPTH_LEQUAL_NOWRITE", "1");
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_PROJ_Z_SCALE", null);
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_NO_PROJ_Z_FIX", null);
            }
            else
            {
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_DEPTH_LEQUAL", null);
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_NO_PROJ_Z_FIX", "1");
            }
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_NO_FOG", disableFog.Checked ? "1" : "0");
            // Keep atmospheric fog on 3D perspective scene geometry, but never
            // let eye-space fog leak into orthographic/2D overlays.
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_NO_FOG_ORTHO", "1");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_NO_FOG_SCREEN", screenFogMask.Checked ? "1" : "0");
            // The opening station's lit window/porthole uses this atlas. Its
            // transparent overlay collapses to almost black when eye-space fog
            // is applied, while the rest of the scene still needs the original
            // distance fade.
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_NO_FOG_HASH",
                "7027023c,56c62e8a");
            string texNoDepthHash = null;
            if (Environment.GetEnvironmentVariable(
                "TOUCHHLE_DS_DIAG_PRESERVE_TEXNODEPTH_HASH") == "1")
            {
                texNoDepthHash = Environment.GetEnvironmentVariable(
                    "TOUCHHLE_DS_TEXNODEPTH_HASH");
            }
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_TEXNODEPTH_HASH",
                texNoDepthHash);
            // The opening/porthole/poster atlas is made of overlapping cover
            // quads. During gameplay-camera shots, depth rejects some of those
            // later transparent layers and leaves black walls / a diagonally
            // split poster. Scope by the complete draw count; the runtime
            // scene-signature gate disables this during the opening fly-in.
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_TEXNODEPTH_HASH_COUNT",
                "56c62e8a:6,56c62e8a:36,7027023c:6,7027023c:36");
            // The game uploads textures from one guest thread and renders from
            // another. Complete each upload before the render thread can draw
            // it, otherwise the opening cockpit intermittently samples a black
            // atlas for one or more frames.
            if (Environment.GetEnvironmentVariable(
                "TOUCHHLE_DS_TEX_UPLOAD_FINISH") == null)
            {
                Environment.SetEnvironmentVariable(
                    "TOUCHHLE_DS_TEX_UPLOAD_FINISH", "1");
            }
            // The opening station uses this atlas for the small safety-poster
            // quads. Keep linear filtering without changing its wall batches.
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_TEXFILTER_LINEAR_HASH", "25a08a82");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_TEXFILTER_LINEAR_MAX_COUNT", "6");
            // Leave the experimental texture-coordinate pre-transform off until
            // it is scoped to the weapon batch without affecting the porthole.
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_PRETRANSFORM_TEX_HASH", null);
            // The cinematic's character/environment batch contains two bad
            // triangles that cover the right half of the frame with black.
            // Remove only those triangles; skipping the whole batch also erases
            // the corridor behind them.
            // The fog fix removes the former black weapon/corridor block
            // without deleting geometry from these shared batches.
            string skipTriangles = null;
            if (Environment.GetEnvironmentVariable(
                "TOUCHHLE_DS_DIAG_PRESERVE_SKIP_TRI") == "1")
            {
                string diagnosticSkip = Environment.GetEnvironmentVariable(
                    "TOUCHHLE_DS_SKIP_TRI_HASH_COUNT");
                if (!string.IsNullOrWhiteSpace(diagnosticSkip))
                {
                    skipTriangles = diagnosticSkip;
                }
            }
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_SKIP_TRI_HASH_COUNT",
                skipTriangles);

            foreach (string name in new[] { "TOUCHHLE_DS_CHAPTER_OVERLAY", "TOUCHHLE_DS_SUBTITLE_OVERLAY", "TOUCHHLE_DS_OBJECTIVE_OVERLAY", "TOUCHHLE_DS_TUTORIAL_OVERLAY", "TOUCHHLE_DS_QTE_OVERLAY" })
                Environment.SetEnvironmentVariable(name, overlayEnabled.Checked ? "1" : null);
            string localOverlayStyle=ConfigurationFile("overlay-style.json");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_OVERLAY_STYLE",File.Exists(localOverlayStyle)?localOverlayStyle:null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_SUBTITLE_MODE",subtitleMode.SelectedIndex==1?"paged":"three");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_GPU_TEXT_OVERLAY", overlayEnabled.Checked && gpuTextOverlay.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_AUTO_OPENING_MIPS", openingMips.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_AUTO_MIPS_ALL", globalMips.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_SKIP_READBACK_FINISH", skipFinish.Checked ? "1" : null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_SAFE_BLACK_FLOOR", safeBlackFloor.Checked ? "1" : null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_SAFE_BLACK_GRAY", safeBlackFloor.Checked ? safeBlackGray.Value.ToString(System.Globalization.CultureInfo.InvariantCulture) : null);
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_TEX_UPLOAD_FINISH",
                textureSync.SelectedIndex == 1 ? "flush"
                    : textureSync.SelectedIndex == 2 ? "0" : "1");
            Environment.SetEnvironmentVariable(
                "TOUCHHLE_DS_GPU_PRESENT_COPY",
                gpuPresentCopy.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MOUSE_LOOK",
                mouseLook.SelectedIndex == 1 ? "aim" : (mouseLook.SelectedIndex == 2 ? "all" : null));
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MOUSE_LOOK_INVERT",
                lookDirection.SelectedIndex == 0 ? "both"
                    : lookDirection.SelectedIndex == 1 ? "none"
                    : lookDirection.SelectedIndex == 2 ? "x" : "y");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MOUSE_LOOK_GAIN", lookGain.Text.Trim());
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_RELEASE_F8", releaseF8.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_RELEASE_ALT", releaseAlt.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_RELEASE_WIN", releaseWin.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CONTROLLER_LOOK_INVERT", padInvertX.Checked ? (padInvertY.Checked ? "both" : "x") : (padInvertY.Checked ? "y" : "none"));
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CONTROLLER_LOOK_GAIN", (padLookSensitivity.Value * 0.18m).ToString(System.Globalization.CultureInfo.InvariantCulture));
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CONTROLLER_LOOK_DEADZONE", (padLookDeadzone.Value / 100m).ToString(System.Globalization.CultureInfo.InvariantCulture));
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_AIM_XY", pcMouse.Checked ? aimPoint.Text.Trim() : null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MOUSE_AIM_MODE", mouseAimMode.SelectedIndex == 1 ? "toggle" : "hold");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_CONTROLLER_AIM_MODE", padAimMode.SelectedIndex == 1 ? "toggle" : "hold");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_NATIVE_INPUTS", nativeInputs.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_AIM_WALK", aimedWalk.Checked ? "1" : "0");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_AIM_FORWARD_LIMIT", (aimedForwardPercent.Value / 100m).ToString(System.Globalization.CultureInfo.InvariantCulture));
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_KEY_STICK_GAIN", stickGain.Text.Trim());

            string macroPath = ConfigurationFile("ds-macros.txt");
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MACROS", File.Exists(macroPath) ? macroPath : null);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_AUTO_DISMISS_ALERTS", debugLogs.Checked ? "0" : "1");

            string frameDir = Path.Combine(baseDir, "diagnostics", "frames");
            if (frameDump.Checked)
            {
                Directory.CreateDirectory(frameDir);
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAMES", frameDir);
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAME_EVERY", "120");
                Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_AT_PRESENT", "1");
            }
            else
            {
                // Keep the shipped launcher deterministic, but allow a
                // diagnostic run to provide its own tightly scoped frame dump
                // window. This is only honored when explicitly requested.
                if (Environment.GetEnvironmentVariable(
                    "TOUCHHLE_DS_DIAG_PRESERVE_FRAME_DUMP") != "1")
                {
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAMES", null);
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAME_EVERY", null);
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAME_FROM", null);
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAME_TO", null);
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAME_ROTATE", null);
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_FRAME_SCALE", null);
                    Environment.SetEnvironmentVariable("TOUCHHLE_DS_DUMP_AT_PRESENT", null);
                }
            }
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_LOG_LEVEL", debugLogs.Checked ? "debug" : "normal");

            var args = new StringBuilder();
            args.Append("\"").Append(ipa).Append("\"");
            args.Append(" --force-composition --landscape-right");
            args.Append(" --scale-hack=").Append(renderScale);
            args.Append(" --stabilize-virtual-cursor=0.06,4");
            if (overlayEnabled.Checked) args.Append(" --preferred-languages=zh-Hans,zh");
            args.Append(" --stick-to-touch=30,190,120,110");
            if (fastPresent.Checked)
            {
                args.Append(" --fast-present");
            }
            if (cap == 0)
            {
                args.Append(" --fps-limit=off");
            }
            else
            {
                args.Append(" --fps-limit=").Append(cap);
            }
            if (debugLogs.Checked)
            {
                args.Append(" --print-fps");
            }

            try
            {
                var info = new ProcessStartInfo
                {
                    FileName = gameExe,
                    Arguments = args.ToString(),
                    WorkingDirectory = DistributionLayout.Game(baseDir),
                    UseShellExecute = false,
                    CreateNoWindow = true
                };
                info.EnvironmentVariables["TOUCHHLE_USER_DATA_DIR"] = SelectedDataRoot();
                ApplyTextureEnvironment(info.EnvironmentVariables, texturePath);
                info.EnvironmentVariables["TOUCHHLE_DS_GAMEPAD_BINDINGS"] = ConfigurationFile("DeadSpace-Gamepad.ini");
                info.EnvironmentVariables["TOUCHHLE_DS_INPUT_MODE"] = inputMode.SelectedIndex == 1 ? "keyboard" : inputMode.SelectedIndex == 2 ? "gamepad" : "auto";
                ApplyFireModeEnvironment(info);
                info.EnvironmentVariables["TOUCHHLE_DS_NATIVE_HUD"] = nativeHud.Checked ? "1" : "0";
                info.EnvironmentVariables["TOUCHHLE_DS_CONFIRM_QUIT"] = confirmQuit.Checked ? "1" : "0";
                info.EnvironmentVariables["TOUCHHLE_DS_LAUNCHER_PATH"] = Application.ExecutablePath;
                PrepareDefaultOptimizationEnvironment(info);
                VisualCompatibility.WriteEffective(ConfigurationDirectory, info);
                PrepareStartupReview(info);
                PreparePlayDiagnostics(info);
                gameProcess = Process.Start(info);
                RecordDiagnosticProcess();
                ShowStartupReview(texturePath);
                closeRequested = default(DateTime);
                RefreshGameState();
                status.Text = "游戏已启动；启动器保持打开。";
            }
            catch (Exception exception)
            {
                status.Text = "启动失败：" + exception.Message;
            }
        }
    }
}

