using System;
using System.Diagnostics;
using System.IO;
using System.Text;
using System.Windows.Forms;
using Microsoft.Win32;

namespace DeadSpaceUninstaller
{
    internal static class UninstallerProgram
    {
        private const string ProductKey = @"Software\DeadSpaceLegacy\R15";
        private const string UninstallKey =
            @"Software\Microsoft\Windows\CurrentVersion\Uninstall\DeadSpaceLegacyR15";

        [STAThread]
        private static int Main(string[] args)
        {
            Application.EnableVisualStyles();
            try
            {
                if (args.Length >= 2 && args[0] == "--cleanup")
                    return Cleanup(args[1]);
                string programRoot = FindProgramRoot();
                DialogResult answer = MessageBox.Show(
                    "将移除程序文件、开始菜单快捷方式和卸载登记。\n\n" +
                    "GameData、Documents 配置和三槽存档不会删除。",
                    "卸载 Dead Space Legacy",
                    MessageBoxButtons.OKCancel,
                    MessageBoxIcon.Question);
                if (answer != DialogResult.OK)
                    return 0;
                RemoveLinksAndRegistry();
                ScheduleCleanup(programRoot);
                return 0;
            }
            catch (Exception error)
            {
                MessageBox.Show(error.Message, "卸载失败",
                    MessageBoxButtons.OK, MessageBoxIcon.Error);
                return 1;
            }
        }

        private static string FindProgramRoot()
        {
            string executable = Path.GetFullPath(
                System.Reflection.Assembly.GetExecutingAssembly().Location);
            string root = Path.GetDirectoryName(executable);
            if (File.Exists(Path.Combine(root, "installer.json")) ||
                File.Exists(Path.Combine(root, "DeadSpace Launcher.exe")))
                return root;
            using (RegistryKey key = Registry.CurrentUser.OpenSubKey(ProductKey))
            {
                if (key != null)
                {
                    string selected = Convert.ToString(
                        key.GetValue("ProgramRoot"));
                    if (!String.IsNullOrWhiteSpace(selected))
                        return Path.GetFullPath(selected);
                }
            }
            throw new InvalidOperationException("无法确定程序安装目录。");
        }

        private static void RemoveLinksAndRegistry()
        {
            try
            {
                string shortcut = Path.Combine(
                    Environment.GetFolderPath(
                        Environment.SpecialFolder.ApplicationData),
                    "Microsoft", "Windows", "Start Menu", "Programs",
                    "Dead Space Legacy.lnk");
                if (File.Exists(shortcut))
                    File.Delete(shortcut);
            }
            catch (IOException) { }
            Registry.CurrentUser.DeleteSubKeyTree(ProductKey, false);
            Registry.CurrentUser.DeleteSubKeyTree(UninstallKey, false);
        }

        private static void ScheduleCleanup(string programRoot)
        {
            string executable = Path.GetFullPath(
                System.Reflection.Assembly.GetExecutingAssembly().Location);
            string temporary = Path.Combine(
                Path.GetTempPath(),
                "DeadSpaceLegacy-cleanup-" +
                    Guid.NewGuid().ToString("N") + ".exe");
            File.Copy(executable, temporary, false);
            Process.Start(new ProcessStartInfo
            {
                FileName = temporary,
                Arguments = "--cleanup " + Quote(programRoot),
                UseShellExecute = false,
                CreateNoWindow = true
            });
        }

        private static int Cleanup(string programRoot)
        {
            programRoot = Path.GetFullPath(programRoot);
            string executable = Path.Combine(
                programRoot, "DeadSpace-Uninstall.exe");
            for (int attempt = 0; attempt < 60; attempt++)
            {
                try
                {
                    if (File.Exists(executable))
                        File.Delete(executable);
                    if (!File.Exists(executable))
                        break;
                }
                catch (IOException) { }
                System.Threading.Thread.Sleep(250);
            }
            for (int attempt = 0; attempt < 80; attempt++)
            {
                try
                {
                    if (Directory.Exists(programRoot))
                        Directory.Delete(programRoot, true);
                    return 0;
                }
                catch (IOException) { }
                catch (UnauthorizedAccessException) { }
                System.Threading.Thread.Sleep(250);
            }
            string log = Path.Combine(
                Path.GetTempPath(), "DeadSpaceLegacy-uninstall-error.txt");
            File.WriteAllText(log,
                "Unable to remove program directory: " + programRoot,
                new UTF8Encoding(false));
            return 1;
        }

        private static string Quote(string value)
        {
            return "\"" + value.Replace("\"", "\\\"") + "\"";
        }
    }
}
