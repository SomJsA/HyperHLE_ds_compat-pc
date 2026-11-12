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
        internal bool CreateDesktopShortcut = true;
        internal bool RegisterInstallation = true;
        internal bool LaunchAfterInstall;
        internal bool InstallHdChinese = true;
        internal string LogPath;
    }

    internal static class InstallerEngine
    {
        private const string ProgramResource = "DeadSpaceInstaller.Program.zip";
        private const string UninstallerResource = "DeadSpaceInstaller.Uninstall.exe";
        private static string I(string value) { return InstallerLanguage.Translate(value); }
        private const string ProductKey = @"Software\DeadSpaceLegacy\R15";
        private const string UninstallKey =
            @"Software\Microsoft\Windows\CurrentVersion\Uninstall\DeadSpaceLegacyR15";
        private const string VersionFallback = "R15-unknown";

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
                    string tier = Convert.ToString(key.GetValue("TextureTiers", key.GetValue("TextureTier", "4")));
                    if (!String.IsNullOrWhiteSpace(program))
                        options.ProgramRoot = program;
                    if (!String.IsNullOrWhiteSpace(data))
                        options.GameDataRoot = data;
                    options.Tier = NormalizeTiers(tier);
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
                    I("找不到 marker 或 IPA 安装源。"), options.ResourcePath);
            bool resourceAlreadyInstalled =
                HasResourceInstallation(options.GameDataRoot);
            if (String.IsNullOrWhiteSpace(options.ResourcePath) &&
                !resourceAlreadyInstalled)
                throw new InvalidOperationException(
                    I("首次安装必须选择 vandal.marker 或 IPA。"));

            progress(I("正在解包程序文件..."));
            ExtractProgram(options.ProgramRoot);
            ApplyLanguageMode(options.ProgramRoot, options.InstallHdChinese);
            string launcher = Path.Combine(
                options.ProgramRoot, "DeadSpace Launcher.exe");
            if (!File.Exists(launcher))
                throw new InvalidDataException(
                    I("安装包缺少 DeadSpace Launcher.exe。"));

            if (!String.IsNullOrWhiteSpace(options.ResourcePath))
            {
                progress(I("正在校验并安装游戏资源，请勿关闭安装器..."));
                RunResourceInstaller(
                    launcher,
                    options.ProgramRoot,
                    options.ResourcePath,
                    options.GameDataRoot,
                    options.Tier);
            }
            else
            {
                progress(I("保留现有 GameData，正在更新程序..."));
            }

            WriteInstallerState(options, launcher);
            ExtractUninstaller(options.ProgramRoot);
            if (options.CreateShortcut)
                CreateShortcut(launcher, Environment.SpecialFolder.Programs);
            if (options.CreateDesktopShortcut)
                CreateShortcut(launcher, Environment.SpecialFolder.DesktopDirectory);
            if (options.RegisterInstallation)
                WriteRegistry(options, launcher);
            progress(I("安装完成。"));
            return FindInstalledResource(options.GameDataRoot);
        }

        internal static void Launch(string programRoot)
        {
            string launcher = Path.Combine(programRoot, "DeadSpace Launcher.exe");
            if (!File.Exists(launcher))
                throw new FileNotFoundException(I("启动器不存在。"), launcher);
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
                        throw new InvalidDataException(I("程序资源不存在。"));
                    using (var archive = new ZipArchive(
                        stream, ZipArchiveMode.Read, false))
                    {
                        int files = 0;
                        foreach (ZipArchiveEntry entry in archive.Entries)
                            if (!String.IsNullOrEmpty(entry.Name))
                                files++;
                        if (files < 20)
                            throw new InvalidDataException(I("程序资源文件过少。"));
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
            // Follow the registered roots for EXE-only upgrades/downgrades.
            // A missing --resource is valid when GameData already has an
            // installed IPA/marker record.
            var options = LoadPrevious();
            bool noShortcut = false;
            bool noRegister = false;
            for (int index = 1; index < args.Length; index++)
            {
                string name = args[index];
                if (name == "--no-shortcut")
                {
                    noShortcut = true;
                    options.CreateDesktopShortcut = false;
                    continue;
                }
                if (name == "--no-desktop-shortcut") { options.CreateDesktopShortcut = false; continue; }
                if (name == "--no-start-menu-shortcut") { noShortcut = true; continue; }
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
                if (name == "--native-language") { options.InstallHdChinese = false; continue; }
                if (name == "--hd-chinese") { options.InstallHdChinese = true; continue; }
                if (index + 1 >= args.Length)
                    throw new ArgumentException(I("缺少参数值：") + name);
                string value = args[++index];
                switch (name)
                {
                    case "--program-root": options.ProgramRoot = value; break;
                    case "--game-data-root": options.GameDataRoot = value; break;
                    case "--resource": options.ResourcePath = value; break;
                    case "--tier":
                    case "--tiers": options.Tier = value; break;
                    case "--log": options.LogPath = value; break;
                    default: throw new ArgumentException(I("未知参数：") + name);
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
            options.Tier = NormalizeTiers(options.Tier);
        }

        internal static string NormalizeTiers(string value)
        {
            if (value == "none") return value;
            var selected = new SortedSet<string>((value ?? "").Split(','));
            foreach (string tier in selected)
                if (tier != "2" && tier != "4" && tier != "6")
                    throw new ArgumentException(I("纹理档位必须是 2、4、6 的组合，或 none。"));
            return String.Join(",", selected);
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
                    I("程序目录和 GameData 目录不能互相包含。"));
            string documents = Environment.GetFolderPath(
                Environment.SpecialFolder.MyDocuments);
            if (!String.IsNullOrWhiteSpace(documents))
            {
                string documentRoot = Path.GetFullPath(documents)
                    .TrimEnd(Path.DirectorySeparatorChar) +
                    Path.DirectorySeparatorChar;
                if (data.StartsWith(documentRoot, StringComparison.OrdinalIgnoreCase))
                    throw new InvalidOperationException(
                        I("GameData 不能放在 Documents；Documents 只保存配置和三槽存档。"));
            }
        }

        private static void EnsureGameNotRunning()
        {
            foreach (string name in new[] { "DeadSpace Launcher", "touchHLE" })
            {
                if (Process.GetProcessesByName(name).Length > 0)
                    throw new InvalidOperationException(
                        I("检测到游戏或启动器正在运行，请先正常退出。"));
            }
        }

        private static void ExtractProgram(string programRoot)
        {
            Stream resource = typeof(InstallerEngine).Assembly
                .GetManifestResourceStream(ProgramResource);
            if (resource == null)
                throw new InvalidDataException(I("安装器内缺少程序资源。"));
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
                            I("程序包缺少启动器或原生运行文件。"));
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
                throw new InvalidDataException(I("程序包包含不安全路径。"));
            string fullRoot = Path.GetFullPath(root)
                .TrimEnd(Path.DirectorySeparatorChar) +
                Path.DirectorySeparatorChar;
            string target = Path.GetFullPath(Path.Combine(fullRoot, normalized));
            if (!target.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException(I("程序包路径越界。"));
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
                .Append(" --tiers ").Append(Quote(tier))
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
                            I("资源安装失败（退出码 ") +
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
            string version = VersionForProgram(options.ProgramRoot);
            var state = new Dictionary<string, object>
            {
                { "schema", 1 },
                { "distribution", "DEAD SPACE MOBILE R2 PC" },
                { "version", version },
                { "program_root", options.ProgramRoot },
                { "game_data_root", options.GameDataRoot },
                { "texture_tier", options.Tier },
                { "texture_tiers", options.Tier == "none" ? new string[0] : options.Tier.Split(',') },
                { "start_menu_shortcut", options.CreateShortcut },
                { "desktop_shortcut", options.CreateDesktopShortcut },
                { "installed_utc", DateTime.UtcNow.ToString("o") },
                { "launcher_sha256", HashFile(launcher) },
                { "documents_policy", "configuration-and-three-slot-saves-only" },
                { "program_update_preserves_game_data", true },
                { "full_R15_complete", false }
                ,{ "hd_chinese_overlay", options.InstallHdChinese }
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

        private static string VersionForProgram(string programRoot)
        {
            try
            {
                string path = Path.Combine(programRoot, "Game", "build-info.json");
                if (File.Exists(path))
                {
                    var map = new JavaScriptSerializer().DeserializeObject(
                        File.ReadAllText(path, Encoding.UTF8)) as Dictionary<string, object>;
                    if (map != null && map.ContainsKey("release_tag"))
                    {
                        string release = Convert.ToString(map["release_tag"]);
                        if (!String.IsNullOrWhiteSpace(release)) return release;
                    }
                    string revision = map == null || !map.ContainsKey("revision")
                        ? null : Convert.ToString(map["revision"]);
                    if (!String.IsNullOrWhiteSpace(revision)) return "R15-" + revision;
                }
            }
            catch (Exception) { }
            return VersionFallback;
        }

        private static void ApplyLanguageMode(string programRoot, bool hdChinese)
        {
            string value = "overlay_enabled=" + (hdChinese ? "1" : "0");
            var files = new List<string> {
                Path.Combine(programRoot, "Defaults", "DeadSpace-Launcher.ini"),
                Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments), "DeadSpaceLegacy", "config", "pc", "DeadSpace-Launcher.ini")
            };
            foreach (string path in files)
            {
                if (!File.Exists(path)) continue;
                string[] lines = File.ReadAllLines(path, Encoding.UTF8);
                bool found = false;
                for (int i = 0; i < lines.Length; i++) if (lines[i].StartsWith("overlay_enabled=", StringComparison.OrdinalIgnoreCase)) { lines[i] = value; found = true; }
                if (!found) { var list = new List<string>(lines); list.Add(value); lines = list.ToArray(); }
                File.WriteAllLines(path, lines, new UTF8Encoding(false));
            }
        }

        private static void ExtractUninstaller(string programRoot)
        {
            string target = Path.Combine(programRoot, "DeadSpace-Uninstall.exe");
            using (Stream source = typeof(InstallerEngine).Assembly
                .GetManifestResourceStream(UninstallerResource))
            {
                if (source == null)
                    throw new InvalidDataException(I("安装器内缺少卸载程序。"));
                using (var output = new FileStream(
                    target,
                    FileMode.Create,
                    FileAccess.Write,
                    FileShare.None))
                    source.CopyTo(output, 128 * 1024);
            }
        }

        internal static void CreateShortcut(string launcher, Environment.SpecialFolder folder)
        {
            try
            {
                string directory = Environment.GetFolderPath(folder);
                if (String.IsNullOrWhiteSpace(directory))
                    throw new IOException(I("Windows 未提供快捷方式目录。"));
                Directory.CreateDirectory(directory);
                string path = Path.Combine(directory, "DEAD SPACE MOBILE.lnk");
                Type shellType = Type.GetTypeFromProgID("WScript.Shell");
                if (shellType == null)
                    throw new InvalidOperationException(I("WScript.Shell 不可用。"));
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
                    new object[] { "DEAD SPACE MOBILE R2" });
                shortcutType.InvokeMember(
                    "Save", BindingFlags.InvokeMethod, null, shortcut, null);
                RemoveLegacyShortcut(directory, launcher);
            }
            catch (Exception error)
            {
                throw new InvalidOperationException(
                    I("程序已安装，但创建") + (folder == Environment.SpecialFolder.Programs ? I("开始菜单") : I("桌面")) + I("快捷方式失败：") +
                    error.Message, error);
            }
        }

        private static void RemoveLegacyShortcut(string directory, string launcher)
        {
            string legacy=Path.Combine(directory,"Dead Space Legacy.lnk");
            if(!File.Exists(legacy)) return;
            var shell=Activator.CreateInstance(Type.GetTypeFromProgID("WScript.Shell"));
            object link=shell.GetType().InvokeMember("CreateShortcut",BindingFlags.InvokeMethod,null,shell,new object[]{legacy});
            string target=Convert.ToString(link.GetType().InvokeMember("TargetPath",BindingFlags.GetProperty,null,link,null));
            if(String.Equals(Path.GetFullPath(target),Path.GetFullPath(launcher),StringComparison.OrdinalIgnoreCase)) File.Delete(legacy);
        }

        private static void WriteRegistry(
            InstallOptions options,
            string launcher)
        {
            string version = VersionForProgram(options.ProgramRoot);
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(ProductKey))
            {
                key.SetValue("ProgramRoot", options.ProgramRoot);
                key.SetValue("GameDataRoot", options.GameDataRoot);
                key.SetValue("TextureTier", options.Tier);
                key.SetValue("TextureTiers", options.Tier);
                key.SetValue("Version", version);
            }
            using (RegistryKey key = Registry.CurrentUser.CreateSubKey(UninstallKey))
            {
                key.SetValue("DisplayName", "DEAD SPACE MOBILE R2");
                key.SetValue("DisplayVersion", version);
                key.SetValue("Publisher", "SomJs");
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
                MessageBox.Show(error.Message, "DEAD SPACE MOBILE",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }
        }
    }
}
