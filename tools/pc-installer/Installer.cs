using System;
using System.Collections.Generic;
using System.ComponentModel;
using System.Diagnostics;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Forms;
using Microsoft.Win32;

namespace DeadSpaceInstaller
{
    internal sealed class InstallOptions
    {
        internal string ProgramRoot;
        internal string GameDataRoot;
        internal string ResourcePath;
        internal string Tier = "4";
        internal bool CreateShortcut = true;
        internal bool RegisterInstallation = true;
        internal bool LaunchAfterInstall;
        internal string LogPath;
    }

    internal static class InstallerEngine
    {
        private const string ProgramResource = "DeadSpaceInstaller.Program.zip";
        private const string UninstallerResource = "DeadSpaceInstaller.Uninstall.exe";
        private const string ProductKey = @"Software\DeadSpaceLegacy\R15";
        private const string UninstallKey =
            @"Software\Microsoft\Windows\CurrentVersion\Uninstall\DeadSpaceLegacyR15";
        private const string Version = "R15-20261106-preview";

        internal static string DefaultProgramRoot()
        {
            return Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "Programs", "DeadSpaceLegacy");
        }

        internal static string DefaultGameDataRoot()
        {
            return Path.Combine(
                Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData),
                "DeadSpaceLegacy", "GameData");
        }

        internal static string FindAdjacentResource()
        {
            string root = AppDomain.CurrentDomain.BaseDirectory;
            foreach (string name in new[] { "vandal.marker", "DeadSpace.ipa" })
            {
                string path = Path.Combine(root, name);
                if (File.Exists(path))
                    return path;
            }
            return String.Empty;
        }

        internal static InstallOptions LoadPrevious()
        {
            var options = new InstallOptions
            {
                ProgramRoot = DefaultProgramRoot(),
                GameDataRoot = DefaultGameDataRoot(),
                ResourcePath = FindAdjacentResource()
            };
            try
            {
                using (RegistryKey key = Registry.CurrentUser.OpenSubKey(ProductKey))
                {
                    if (key == null)
                        return options;
                    string program = Convert.ToString(key.GetValue("ProgramRoot"));
                    string data = Convert.ToString(key.GetValue("GameDataRoot"));
                    string tier = Convert.ToString(key.GetValue("TextureTier"));
                    if (!String.IsNullOrWhiteSpace(program))
                        options.ProgramRoot = program;
                    if (!String.IsNullOrWhiteSpace(data))
                        options.GameDataRoot = data;
                    if (tier == "2" || tier == "4" || tier == "6")
                        options.Tier = tier;
                }
            }
            catch (Exception)
            {
                // The defaults remain usable if registry access is restricted.
            }
            return options;
        }

        internal static bool HasProgramInstallation(string programRoot)
        {
            if (String.IsNullOrWhiteSpace(programRoot))
                return false;
            return File.Exists(Path.Combine(
                programRoot, "DeadSpace Launcher.exe")) &&
                File.Exists(Path.Combine(programRoot, "installer.json"));
        }

        internal static bool HasResourceInstallation(string gameDataRoot)
        {
            return FindInstalledResource(gameDataRoot) != null;
        }

        internal static string FindInstalledResource(string gameDataRoot)
        {
            try
            {
                string content = Path.Combine(gameDataRoot, "marker-content");
                if (!Directory.Exists(content))
                    return null;
                string[] directories = Directory.GetDirectories(content);
                Array.Sort(directories, delegate(string left, string right)
                {
                    return Directory.GetLastWriteTimeUtc(right)
                        .CompareTo(Directory.GetLastWriteTimeUtc(left));
                });
                foreach (string directory in directories)
                {
                    if (Path.GetFileName(directory).StartsWith(
                        ".", StringComparison.Ordinal))
                        continue;
                    if (File.Exists(Path.Combine(directory, "installation.json")) &&
                        File.Exists(Path.Combine(directory, "payload", "DeadSpace.ipa")))
                        return directory;
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            return null;
        }

        internal static string Install(InstallOptions options, Action<string> progress)
        {
            Normalize(options);
            EnsureInstallerCanWrite(options);
            EnsureGameNotRunning();
            if (!String.IsNullOrWhiteSpace(options.ResourcePath) &&
                !File.Exists(options.ResourcePath))
                throw new FileNotFoundException(
                    "找不到 marker 或 IPA 安装源。", options.ResourcePath);
            bool resourceAlreadyInstalled =
                HasResourceInstallation(options.GameDataRoot);
            if (String.IsNullOrWhiteSpace(options.ResourcePath) &&
                !resourceAlreadyInstalled)
                throw new InvalidOperationException(
                    "首次安装必须选择 vandal.marker 或 IPA。");

            progress("正在解包程序文件...");
            ExtractProgram(options.ProgramRoot);
            string launcher = Path.Combine(
                options.ProgramRoot, "DeadSpace Launcher.exe");
            if (!File.Exists(launcher))
                throw new InvalidDataException(
                    "安装包缺少 DeadSpace Launcher.exe。");

            if (!String.IsNullOrWhiteSpace(options.ResourcePath))
            {
                progress("正在校验并安装游戏资源，请勿关闭安装器...");
                RunResourceInstaller(
                    launcher,
                    options.ProgramRoot,
                    options.ResourcePath,
                    options.GameDataRoot,
                    options.Tier);
            }
            else
            {
                progress("保留现有 GameData，正在更新程序...");
            }

            WriteInstallerState(options, launcher);
            ExtractUninstaller(options.ProgramRoot);
            if (options.CreateShortcut)
                CreateStartMenuShortcut(launcher);
            if (options.RegisterInstallation)
                WriteRegistry(options, launcher);
            progress("安装完成。");
            return FindInstalledResource(options.GameDataRoot);
        }

        internal static void Launch(string programRoot)
        {
            string launcher = Path.Combine(programRoot, "DeadSpace Launcher.exe");
            if (!File.Exists(launcher))
                throw new FileNotFoundException("启动器不存在。", launcher);
            Process.Start(new ProcessStartInfo
            {
                FileName = launcher,
                WorkingDirectory = programRoot,
                UseShellExecute = true
            });
        }

        internal static int VerifyEmbeddedProgram(string logPath)
        {
            try
            {
                using (Stream stream = typeof(InstallerEngine).Assembly
                    .GetManifestResourceStream(ProgramResource))
                {
                    if (stream == null)
                        throw new InvalidDataException("程序资源不存在。");
                    using (var archive = new ZipArchive(
                        stream, ZipArchiveMode.Read, false))
                    {
                        int files = 0;
                        foreach (ZipArchiveEntry entry in archive.Entries)
                            if (!String.IsNullOrEmpty(entry.Name))
                                files++;
                        if (files < 20)
                            throw new InvalidDataException("程序资源文件过少。");
                        WriteLog(logPath,
                            "PASS embedded program ZIP entries=" + files);
                    }
                }
                return 0;
            }
            catch (Exception error)
            {
                WriteLog(logPath, "FAIL " + error);
                return 1;
            }
        }

        internal static int RunSilent(string[] args)
        {
            var options = new InstallOptions();
            bool noShortcut = false;
            bool noRegister = false;
            for (int index = 1; index < args.Length; index++)
            {
                string name = args[index];
                if (name == "--no-shortcut")
                {
                    noShortcut = true;
                    continue;
                }
                if (name == "--no-register")
                {
                    noRegister = true;
                    continue;
                }
                if (name == "--launch")
                {
                    options.LaunchAfterInstall = true;
                    continue;
                }
                if (index + 1 >= args.Length)
                    throw new ArgumentException("缺少参数值：" + name);
                string value = args[++index];
                switch (name)
                {
                    case "--program-root": options.ProgramRoot = value; break;
                    case "--game-data-root": options.GameDataRoot = value; break;
                    case "--resource": options.ResourcePath = value; break;
                    case "--tier": options.Tier = value; break;
                    case "--log": options.LogPath = value; break;
                    default: throw new ArgumentException("未知参数：" + name);
                }
            }
            options.CreateShortcut = !noShortcut;
            options.RegisterInstallation = !noRegister;
            try
            {
                if (String.IsNullOrWhiteSpace(options.ProgramRoot))
                    options.ProgramRoot = DefaultProgramRoot();
                if (String.IsNullOrWhiteSpace(options.GameDataRoot))
                    options.GameDataRoot = DefaultGameDataRoot();
                Install(options, delegate(string message)
                {
                    WriteLog(options.LogPath, message);
                });
                WriteLog(options.LogPath, "PASS silent install");
                if (options.LaunchAfterInstall)
                    Launch(options.ProgramRoot);
                return 0;
            }
            catch (Exception error)
            {
                WriteLog(options.LogPath, "FAIL " + error);
                return 1;
            }
        }

        internal static void WriteLog(string path, string message)
        {
            if (String.IsNullOrWhiteSpace(path))
                return;
            Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path)));
            File.AppendAllText(path,
                DateTime.UtcNow.ToString("o") + " " + message + Environment.NewLine,
                new UTF8Encoding(false));
        }

        private static void Normalize(InstallOptions options)
        {
            if (String.IsNullOrWhiteSpace(options.ProgramRoot))
                options.ProgramRoot = DefaultProgramRoot();
            if (String.IsNullOrWhiteSpace(options.GameDataRoot))
                options.GameDataRoot = DefaultGameDataRoot();
            options.ProgramRoot = Path.GetFullPath(options.ProgramRoot);
            options.GameDataRoot = Path.GetFullPath(options.GameDataRoot);
            if (!String.IsNullOrWhiteSpace(options.ResourcePath))
                options.ResourcePath = Path.GetFullPath(options.ResourcePath);
            if (options.Tier != "2" && options.Tier != "4" && options.Tier != "6")
                throw new ArgumentException("纹理档位必须是 2、4 或 6。");
        }

        private static void EnsureInstallerCanWrite(InstallOptions options)
        {
            string program = options.ProgramRoot.TrimEnd(
                Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            string data = options.GameDataRoot.TrimEnd(
                Path.DirectorySeparatorChar) + Path.DirectorySeparatorChar;
            if (program.StartsWith(data, StringComparison.OrdinalIgnoreCase) ||
                data.StartsWith(program, StringComparison.OrdinalIgnoreCase))
                throw new InvalidOperationException(
                    "程序目录和 GameData 目录不能互相包含。");
            string documents = Environment.GetFolderPath(
                Environment.SpecialFolder.MyDocuments);
            if (!String.IsNullOrWhiteSpace(documents))
            {
                string documentRoot = Path.GetFullPath(documents)
                    .TrimEnd(Path.DirectorySeparatorChar) +
                    Path.DirectorySeparatorChar;
                if (data.StartsWith(documentRoot, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException(
                        "GameData 不能放在 Documents；Documents 只保存配置和三槽存档。");
            }
        }

        private static void EnsureGameNotRunning()
        {
            foreach (string name in new[] { "DeadSpace Launcher", "touchHLE" })
            {
                if (Process.GetProcessesByName(name).Length > 0)
                    throw new InvalidOperationException(
                        "检测到游戏或启动器正在运行，请先正常退出。");
            }
        }

        private static void ExtractProgram(string programRoot)
        {
            Stream resource = typeof(InstallerEngine).Assembly
                .GetManifestResourceStream(ProgramResource);
            if (resource == null)
                throw new InvalidDataException("安装器内缺少程序资源。");
            using (resource)
            using (var archive = new ZipArchive(
                resource, ZipArchiveMode.Read, false))
            {
                string parent = Path.GetDirectoryName(programRoot);
                Directory.CreateDirectory(parent);
                string temporary = Path.Combine(
                    parent, ".DeadSpaceLegacy-install-" +
                        Guid.NewGuid().ToString("N"));
                Directory.CreateDirectory(temporary);
                try
                {
                    foreach (ZipArchiveEntry entry in archive.Entries)
                    {
                        if (String.IsNullOrEmpty(entry.Name))
                            continue;
                        string target = SafeTarget(temporary, entry.FullName);
                        Directory.CreateDirectory(
                            Path.GetDirectoryName(target));
                        using (Stream input = entry.Open())
                        using (var output = new FileStream(
                            target,
                            FileMode.CreateNew,
                            FileAccess.Write,
                            FileShare.None))
                            input.CopyTo(output, 4 * 1024 * 1024);
                    }
                    if (!File.Exists(Path.Combine(
                            temporary, "DeadSpace Launcher.exe")) ||
                        !File.Exists(Path.Combine(
                            temporary, "Game", "touchHLE.exe")))
                        throw new InvalidDataException(
                            "程序包缺少启动器或原生运行文件。");
                    if (!Directory.Exists(programRoot))
                    {
                        Directory.Move(temporary, programRoot);
                    }
                    else
                    {
                        CopyTree(temporary, programRoot);
                        Directory.Delete(temporary, true);
                    }
                }
                finally
                {
                    if (Directory.Exists(temporary))
                        Directory.Delete(temporary, true);
                }
            }
        }

        private static void CopyTree(string source, string target)
        {
            foreach (string directory in Directory.GetDirectories(
                source, "*", SearchOption.AllDirectories))
            {
                string relative = directory.Substring(source.Length)
                    .TrimStart(Path.DirectorySeparatorChar);
                Directory.CreateDirectory(Path.Combine(target, relative));
            }
            foreach (string file in Directory.GetFiles(
                source, "*", SearchOption.AllDirectories))
            {
                string relative = file.Substring(source.Length)
                    .TrimStart(Path.DirectorySeparatorChar);
                string destination = Path.Combine(target, relative);
                Directory.CreateDirectory(Path.GetDirectoryName(destination));
                File.Copy(file, destination, true);
            }
        }

        private static string SafeTarget(string root, string relative)
        {
            string normalized = relative.Replace('/', Path.DirectorySeparatorChar);
            if (normalized.StartsWith("\\", StringComparison.Ordinal) ||
                normalized.Contains(":") ||
                normalized.Split(Path.DirectorySeparatorChar)
                    .Length == 0)
                throw new InvalidDataException("程序包包含不安全路径。");
            string fullRoot = Path.GetFullPath(root)
                .TrimEnd(Path.DirectorySeparatorChar) +
                Path.DirectorySeparatorChar;
            string target = Path.GetFullPath(Path.Combine(fullRoot, normalized));
            if (!target.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("程序包路径越界。");
            return target;
        }

        private static void RunResourceInstaller(
            string launcher,
            string programRoot,
            string resourcePath,
            string gameDataRoot,
            string tier)
        {
            string resultPath = Path.Combine(
                Path.GetTempPath(),
                "DeadSpaceLegacy-install-result-" +
                    Guid.NewGuid().ToString("N") + ".json");
            string sourceOption = Path.GetExtension(resourcePath)
                .Equals(".ipa", StringComparison.OrdinalIgnoreCase)
                ? "--ipa" : "--marker";
            var arguments = new StringBuilder();
            arguments.Append("--install-resource ")
                .Append(sourceOption).Append(" ").Append(Quote(resourcePath))
                .Append(" --install-root ").Append(Quote(gameDataRoot))
                .Append(" --tier ").Append(Quote(tier))
                .Append(" --result ").Append(Quote(resultPath));
            try
            {
                using (var process = new Process())
                {
                    process.StartInfo = new ProcessStartInfo
                    {
                        FileName = launcher,
                        Arguments = arguments.ToString(),
                        WorkingDirectory = programRoot,
                        UseShellExecute = false,
                        CreateNoWindow = true,
                        RedirectStandardOutput = true,
                        RedirectStandardError = true
                    };
                    process.Start();
                    string output = process.StandardOutput.ReadToEnd();
                    string error = process.StandardError.ReadToEnd();
                    process.WaitForExit();
                    if (process.ExitCode != 0)
                    {
                        string detail = error.Trim();
                        if (detail.Length == 0 && File.Exists(resultPath))
                            detail = File.ReadAllText(resultPath);
                        if (detail.Length == 0)
                            detail = output.Trim();
                        throw new InvalidDataException(
                            "资源安装失败（退出码 " +
                            process.ExitCode + "）：" + detail);
                    }
                }
            }
            finally
            {
                if (File.Exists(resultPath))
                    File.Delete(resultPath);
            }
        }

        private static string Quote(string value)
        {
            return "\"" + value.Replace("\"", "\\\"") + "\"";
        }

        private static void WriteInstallerState(
            InstallOptions options,
            string launcher)
        {
            var state = new Dictionary<string, object>
            {
                { "schema", 1 },
                { "distribution", "DeadSpace Legacy R15 PC" },
                { "version", Version },
                { "program_root", options.ProgramRoot },
                { "game_data_root", options.GameDataRoot },
                { "texture_tier", options.Tier },
                { "installed_utc", DateTime.UtcNow.ToString("o") },
                { "launcher_sha256", HashFile(launcher) },
                { "documents_policy", "configuration-and-three-slot-saves-only" },
                { "program_update_preserves_game_data", true },
                { "full_R15_complete", false }
            };
            string json = new JavaScriptSerializer().Serialize(state);
            string path = Path.Combine(options.ProgramRoot, "installer.json");
            string temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
            try
            {
                File.WriteAllText(temporary, json, new UTF8Encoding(false));
                if (File.Exists(path))
                    File.Replace(temporary, path, null);
                else
                    File.Move(temporary, path);
            }
            finally
            {
                if (File.Exists(temporary))
                    File.Delete(temporary);
            }
        }

        private static void ExtractUninstaller(string programRoot)
        {
            string target = Path.Combine(programRoot, "DeadSpace-Uninstall.exe");
            using (Stream source = typeof(InstallerEngine).Assembly
                .GetManifestResourceStream(UninstallerResource))
            {
                if (source == null)
                    throw new InvalidDataException("安装器内缺少卸载程序。");
                using (var output = new FileStream(
                    target,
                    FileMode.Create,
                    FileAccess.Write,
                    FileShare.None))
                    source.CopyTo(output, 128 * 1024);
            }
        }

        private static void CreateStartMenuShortcut(string launcher)
        {
            try
            {
                string directory = Path.Combine(
                    Environment.GetFolderPath(
                        Environment.SpecialFolder.ApplicationData),
                    "Microsoft", "Windows", "Start Menu", "Programs");
                Directory.CreateDirectory(directory);
                string path = Path.Combine(directory, "Dead Space Legacy.lnk");
                Type shellType = Type.GetTypeFromProgID("WScript.Shell");
                if (shellType == null)
                    throw new InvalidOperationException("WScript.Shell 不可用。");
                object shell = Activator.CreateInstance(shellType);
                object shortcut = shellType.InvokeMember(
                    "CreateShortcut",
                    BindingFlags.InvokeMethod,
                    null,
                    shell,
                    new object[] { path });
                Type shortcutType = shortcut.GetType();
                shortcutType.InvokeMember(
                    "TargetPath",
                    BindingFlags.SetProperty,
                    null,
                    shortcut,
                    new object[] { launcher });
                shortcutType.InvokeMember(
                    "WorkingDirectory",
                    BindingFlags.SetProperty,
                    null,
                    shortcut,
                    new object[] { Path.GetDirectoryName(launcher) });
                shortcutType.InvokeMember(
                    "IconLocation",
                    BindingFlags.SetProperty,
                    null,
                    shortcut,
                    new object[] { launcher + ",0" });
                shortcutType.InvokeMember(
                    "Description",
                    BindingFlags.SetProperty,
                    null,
                    shortcut,
                    new object[] { "Dead Space Legacy R15" });
                shortcutType.InvokeMember(
                    "Save", BindingFlags.InvokeMethod, null, shortcut, null);
            }
            catch (Exception error)
            {
                throw new InvalidOperationException(
                    "程序已安装，但创建开始菜单快捷方式失败：" +
                    error.Message, error);
            }
        }

        private static void WriteRegistry(
            InstallOptions options,
            string launcher)
        {
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(ProductKey))
            {
                key.SetValue("ProgramRoot", options.ProgramRoot);
                key.SetValue("GameDataRoot", options.GameDataRoot);
                key.SetValue("TextureTier", options.Tier);
                key.SetValue("Version", Version);
            }
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(UninstallKey))
            {
                key.SetValue("DisplayName", "Dead Space Legacy R15");
                key.SetValue("DisplayVersion", Version);
                key.SetValue("Publisher", "DeadSpace Legacy integration");
                key.SetValue("InstallLocation", options.ProgramRoot);
                key.SetValue("DisplayIcon", launcher + ",0");
                key.SetValue("UninstallString",
                    Quote(Path.Combine(options.ProgramRoot,
                        "DeadSpace-Uninstall.exe")));
                key.SetValue("NoModify", 1, RegistryValueKind.DWord);
                key.SetValue("NoRepair", 1, RegistryValueKind.DWord);
            }
        }

        internal static string HashFile(string path)
        {
            using (var sha = System.Security.Cryptography.SHA256.Create())
            using (FileStream stream = File.OpenRead(path))
                return BitConverter.ToString(sha.ComputeHash(stream))
                    .Replace("-", "").ToLowerInvariant();
        }
    }

    internal sealed class InstallerForm : Form
    {
        private readonly TextBox programRoot = new TextBox();
        private readonly TextBox gameDataRoot = new TextBox();
        private readonly TextBox resourcePath = new TextBox();
        private readonly ComboBox tier = new ComboBox();
        private readonly CheckBox launchAfter = new CheckBox();
        private readonly TextBox details = new TextBox();
        private readonly Button install = new Button();
        private readonly BackgroundWorker worker = new BackgroundWorker();
        private bool busy;

        internal InstallerForm()
        {
            Text = "Dead Space Legacy R15 安装器";
            ClientSize = new Size(840, 590);
            MinimumSize = new Size(856, 629);
            StartPosition = FormStartPosition.CenterScreen;
            Font = new Font("Microsoft YaHei UI", 9.5f);
            BackColor = Color.FromArgb(13, 23, 31);
            ForeColor = Color.FromArgb(224, 235, 239);

            Controls.Add(new Label
            {
                Text = "DEAD SPACE LEGACY",
                Font = new Font("Segoe UI", 21, FontStyle.Bold),
                ForeColor = Color.FromArgb(97, 218, 232),
                Location = new Point(30, 24),
                AutoSize = true
            });
            Controls.Add(new Label
            {
                Text = "R15 PC 安装 / 升级",
                Font = new Font("Microsoft YaHei UI", 12, FontStyle.Bold),
                Location = new Point(32, 72),
                AutoSize = true
            });

            Section("程序安装目录", 112);
            Row(programRoot, 146, delegate { BrowseFolder(programRoot); });
            Section("游戏数据 GameData", 194);
            Row(gameDataRoot, 228, delegate { BrowseFolder(gameDataRoot); });
            Controls.Add(new Label
            {
                Text = "程序更新不会覆盖此目录；配置和三槽存档仍在 Documents。",
                ForeColor = Color.FromArgb(145, 166, 178),
                Location = new Point(219, 260),
                Size = new Size(570, 24)
            });
            Section("安装源（marker 或 IPA）", 286);
            Row(resourcePath, 320, BrowseResource);
            Controls.Add(new Label
            {
                Text = "升级已有安装时可留空，仅更新程序。",
                ForeColor = Color.FromArgb(145, 166, 178),
                Location = new Point(219, 352),
                Size = new Size(570, 24)
            });

            Controls.Add(new Label
            {
                Text = "纹理档位",
                Location = new Point(32, 390),
                Size = new Size(170, 26)
            });
            tier.SetBounds(219, 386, 130, 28);
            tier.DropDownStyle = ComboBoxStyle.DropDownList;
            tier.Items.AddRange(new object[] { "2×", "4×（推荐）", "6×" });
            tier.SelectedIndex = 1;
            Controls.Add(tier);
            launchAfter.Text = "安装完成后启动";
            launchAfter.SetBounds(380, 386, 220, 28);
            Controls.Add(launchAfter);

            details.SetBounds(30, 430, 780, 96);
            details.Multiline = true;
            details.ReadOnly = true;
            details.ScrollBars = ScrollBars.Vertical;
            details.BackColor = Color.FromArgb(8, 15, 21);
            details.ForeColor = Color.FromArgb(184, 203, 212);
            details.Text = "选择安装位置和资源后点击安装。";
            Controls.Add(details);

            install.Text = "开始安装";
            install.SetBounds(630, 540, 180, 42);
            install.BackColor = Color.FromArgb(97, 218, 232);
            install.ForeColor = Color.FromArgb(13, 23, 31);
            install.FlatStyle = FlatStyle.Flat;
            install.Click += delegate { BeginInstall(); };
            Controls.Add(install);

            worker.DoWork += delegate(object sender, DoWorkEventArgs eventArgs)
            {
                var options = (InstallOptions)eventArgs.Argument;
                eventArgs.Result = InstallerEngine.Install(
                    options, delegate(string message)
                    {
                        worker.ReportProgress(0, message);
                    });
            };
            worker.ProgressChanged += delegate(
                object sender, ProgressChangedEventArgs eventArgs)
            {
                details.AppendText(Environment.NewLine +
                    Convert.ToString(eventArgs.UserState));
            };
            worker.RunWorkerCompleted += delegate(
                object sender, RunWorkerCompletedEventArgs eventArgs)
            {
                busy = false;
                install.Enabled = true;
                Cursor = Cursors.Default;
                if (eventArgs.Error != null)
                {
                    details.AppendText(Environment.NewLine +
                        "失败：" + eventArgs.Error.Message);
                    MessageBox.Show(this, eventArgs.Error.Message,
                        "安装失败", MessageBoxButtons.OK, MessageBoxIcon.Error);
                    return;
                }
                details.AppendText(Environment.NewLine +
                    "游戏资源已安装到选定 GameData。");
                if (launchAfter.Checked)
                    InstallerEngine.Launch(programRoot.Text.Trim());
            };
            worker.WorkerReportsProgress = true;

            InstallOptions previous = InstallerEngine.LoadPrevious();
            programRoot.Text = previous.ProgramRoot;
            gameDataRoot.Text = previous.GameDataRoot;
            resourcePath.Text = previous.ResourcePath;
            tier.SelectedIndex = previous.Tier == "2" ? 0 :
                previous.Tier == "6" ? 2 : 1;
            bool existing = InstallerEngine.HasProgramInstallation(
                previous.ProgramRoot);
            if (existing)
            {
                install.Text = "更新 / 更改安装";
                details.Text =
                    "检测到现有安装。可直接更新程序并保留 GameData；" +
                    "如需更换资源，请重新选择 marker 或 IPA。";
            }
            FormClosing += delegate(object sender, FormClosingEventArgs eventArgs)
            {
                if (!busy)
                    return;
                eventArgs.Cancel = true;
                MessageBox.Show(this, "资源安装正在进行，请等待完成。",
                    "正在安装", MessageBoxButtons.OK, MessageBoxIcon.Information);
            };
        }

        private void Row(TextBox textBox, int y, EventHandler browse)
        {
            textBox.SetBounds(219, y, 460, 28);
            textBox.BackColor = Color.FromArgb(8, 15, 21);
            textBox.ForeColor = Color.FromArgb(224, 235, 239);
            Controls.Add(textBox);
            var button = new Button
            {
                Text = "选择",
                Location = new Point(690, y - 2),
                Size = new Size(120, 32),
                FlatStyle = FlatStyle.Flat
            };
            button.Click += browse;
            Controls.Add(button);
        }

        private void Section(string text, int y)
        {
            Controls.Add(new Label
            {
                Text = text,
                Location = new Point(32, y),
                Size = new Size(180, 26)
            });
        }

        private void BrowseFolder(TextBox target)
        {
            using (var dialog = new FolderBrowserDialog())
            {
                dialog.Description = "选择安装目录";
                if (Directory.Exists(target.Text))
                    dialog.SelectedPath = target.Text;
                if (dialog.ShowDialog(this) == DialogResult.OK)
                    target.Text = dialog.SelectedPath;
            }
        }

        private void BrowseResource(object sender, EventArgs eventArgs)
        {
            using (var dialog = new OpenFileDialog())
            {
                dialog.Title = "选择 vandal.marker 或 IPA";
                dialog.Filter =
                    "游戏资源 (*.marker;*.ipa)|*.marker;*.ipa|所有文件 (*.*)|*.*";
                string current = resourcePath.Text.Trim();
                if (current.Length > 0)
                {
                    string directory = Path.GetDirectoryName(current);
                    if (Directory.Exists(directory))
                        dialog.InitialDirectory = directory;
                }
                if (dialog.ShowDialog(this) == DialogResult.OK)
                    resourcePath.Text = dialog.FileName;
            }
        }

        private void BeginInstall()
        {
            if (busy)
                return;
            var options = new InstallOptions
            {
                ProgramRoot = programRoot.Text.Trim(),
                GameDataRoot = gameDataRoot.Text.Trim(),
                ResourcePath = resourcePath.Text.Trim(),
                Tier = tier.SelectedIndex == 0 ? "2" :
                    tier.SelectedIndex == 2 ? "6" : "4",
                CreateShortcut = true,
                RegisterInstallation = true,
                LaunchAfterInstall = launchAfter.Checked
            };
            busy = true;
            install.Enabled = false;
            Cursor = Cursors.WaitCursor;
            details.AppendText(Environment.NewLine + "开始安装...");
            worker.RunWorkerAsync(options);
        }
    }

    internal static class InstallerProgram
    {
        [STAThread]
        private static int Main(string[] args)
        {
            Application.EnableVisualStyles();
            Application.SetCompatibleTextRenderingDefault(false);
            try
            {
                if (args.Length > 0 && args[0] == "--verify-embedded")
                {
                    string log = args.Length >= 3 && args[1] == "--log"
                        ? args[2] : null;
                    return InstallerEngine.VerifyEmbeddedProgram(log);
                }
                if (args.Length > 0 && args[0] == "--silent")
                    return InstallerEngine.RunSilent(args);
                Application.Run(new InstallerForm());
                return 0;
            }
            catch (Exception error)
            {
                MessageBox.Show(error.Message, "Dead Space Legacy",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }
        }
    }
}
