using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Runtime.InteropServices;
using Microsoft.Win32;

internal static class TestShortcuts
{
    [DllImport("advapi32.dll")] private static extern int RegOverridePredefKey(IntPtr key, IntPtr target);
    private static readonly BindingFlags Flags = BindingFlags.NonPublic | BindingFlags.Static;
    private static object Get(object link, string property) { return link.GetType().InvokeMember(property, BindingFlags.GetProperty, null, link, null); }
    private static object Link(string path) { Type type = Type.GetTypeFromProgID("WScript.Shell"); return type.InvokeMember("CreateShortcut", BindingFlags.InvokeMethod, null, Activator.CreateInstance(type), new object[] { path }); }
    private static void Set(object link, string property, object value) { link.GetType().InvokeMember(property, BindingFlags.SetProperty, null, link, new[] { value }); }
    private static void Assert(bool condition, string message) { if (!condition) throw new Exception(message); }
    [STAThread]
    private static int Main(string[] args)
    {
        string output = Path.GetFullPath(args[2]); Directory.CreateDirectory(output);
        var installer = Assembly.LoadFrom(Path.GetFullPath(args[0]));
        var engine = installer.GetType("DeadSpaceInstaller.InstallerEngine");
        var uninstall = Assembly.LoadFrom(Path.GetFullPath(args[1])).GetType("DeadSpaceUninstaller.UninstallerProgram");
        string root = Path.Combine(output, "Program with spaces"); Directory.CreateDirectory(root);
        string launcher = Path.Combine(root, "DeadSpace Launcher.exe"); File.Copy(args[0], launcher);
        var backups = new Dictionary<string, byte[]>();
        string keyName = @"Software\DeadSpaceLegacy\InstallerTests\" + Guid.NewGuid().ToString("N");
        RegistryKey sandbox = Registry.CurrentUser.CreateSubKey(keyName);
        bool overridden = false;
        try {
            foreach (var folder in new[] { Environment.SpecialFolder.Programs, Environment.SpecialFolder.DesktopDirectory }) {
                string path = Path.Combine(Environment.GetFolderPath(folder), "DEAD SPACE MOBILE.lnk");
                backups[path] = File.Exists(path) ? File.ReadAllBytes(path) : null;
                engine.GetMethod("CreateShortcut", Flags).Invoke(null, new object[] { launcher, folder });
                object link = Link(path);
                Assert(String.Equals(Convert.ToString(Get(link, "TargetPath")), launcher, StringComparison.OrdinalIgnoreCase), "Shortcut target mismatch");
                Assert(String.Equals(Convert.ToString(Get(link, "WorkingDirectory")), root, StringComparison.OrdinalIgnoreCase), "Shortcut working directory mismatch");
                Assert(Convert.ToString(Get(link, "IconLocation")) == launcher + ",0", "Shortcut icon mismatch");
                File.Copy(path, Path.Combine(output, folder + ".lnk"));
            }
            int code = RegOverridePredefKey(new IntPtr(unchecked((int)0x80000001)), sandbox.Handle.DangerousGetHandle());
            Assert(code == 0, "HKCU test isolation failed: " + code); overridden = true;
            var options = Activator.CreateInstance(installer.GetType("DeadSpaceInstaller.InstallOptions"), true);
            options.GetType().GetField("ProgramRoot", BindingFlags.NonPublic | BindingFlags.Instance).SetValue(options, root);
            options.GetType().GetField("GameDataRoot", BindingFlags.NonPublic | BindingFlags.Instance).SetValue(options, Path.Combine(output, "GameData"));
            engine.GetMethod("WriteRegistry", Flags).Invoke(null, new[] { options, (object)launcher });
            using (var key = Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Uninstall\DeadSpaceLegacyR15")) {
                Assert(key != null, "Uninstall registration missing");
                Assert(Convert.ToString(key.GetValue("InstallLocation")) == root, "InstallLocation mismatch");
                Assert(Convert.ToString(key.GetValue("UninstallString")).Contains("DeadSpace-Uninstall.exe"), "Uninstall command missing");
            }
            uninstall.GetMethod("RemoveLinksAndRegistry", Flags).Invoke(null, new object[] { root });
            foreach (string path in backups.Keys) Assert(!File.Exists(path), "Owned shortcut not removed");
            using (var key = Registry.CurrentUser.OpenSubKey(@"Software\DeadSpaceLegacy\R15")) Assert(key == null, "Product registration not removed");
            string desktop = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), "DEAD SPACE MOBILE.lnk");
            var other = Link(desktop); Set(other, "TargetPath", Path.Combine(output, "other.exe")); other.GetType().InvokeMember("Save", BindingFlags.InvokeMethod, null, other, null);
            uninstall.GetMethod("RemoveLinksAndRegistry", Flags).Invoke(null, new object[] { root });
            Assert(File.Exists(desktop), "Uninstaller deleted another target's shortcut");
            Console.WriteLine("PASS desktop + Start Menu links, targets, icons, isolated HKCU uninstall registration, owned-link removal and foreign-link preservation");
            File.WriteAllText(Path.Combine(output, "PASS.txt"), "Actual Windows .lnk creation verified. HKCU redirected to disposable test key. User links restored in finally.");
            return 0;
        } catch (Exception error) { while (error.InnerException != null) error = error.InnerException; Console.Error.WriteLine(error.Message); return 1; }
        finally {
            if (overridden) RegOverridePredefKey(new IntPtr(unchecked((int)0x80000001)), IntPtr.Zero);
            sandbox.Dispose(); Registry.CurrentUser.DeleteSubKeyTree(keyName, false);
            foreach (var item in backups) { if (item.Value == null) { if (File.Exists(item.Key)) File.Delete(item.Key); } else File.WriteAllBytes(item.Key, item.Value); }
            foreach (var item in backups) Assert(item.Value == null ? !File.Exists(item.Key) : Convert.ToBase64String(File.ReadAllBytes(item.Key)) == Convert.ToBase64String(item.Value), "User shortcut restore mismatch");
        }
    }
}
