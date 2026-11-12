using System;
using System.IO;
using System.Reflection;
using System.Windows.Forms;

class PcGpuTextDefaultsTest
{
    static readonly BindingFlags Hidden = BindingFlags.Instance | BindingFlags.NonPublic;
    static object Field(object owner, string name) { return owner.GetType().GetField(name, Hidden).GetValue(owner); }
    static void Call(object owner, string name) { owner.GetType().GetMethod(name, Hidden).Invoke(owner, null); }
    static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }

    [STAThread] static void Main(string[] args)
    {
        try { Run(args); }
        catch (Exception error) { Console.Error.WriteLine("FAIL " + error); Environment.ExitCode = 1; }
    }
    static void Run(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("launcher assembly and new private output required");
        string root = Path.GetFullPath(args[1]);
        Check(!Directory.Exists(root) && !File.Exists(root), "Output must be new");
        Directory.CreateDirectory(root);
        var assembly = Assembly.LoadFrom(Path.GetFullPath(args[0]));
        var hardwareType = assembly.GetType("DeadSpaceBootstrap.DisplayHardware", true);
        var formType = assembly.GetType("DeadSpaceBootstrap.LauncherForm", true);
        Application.EnableVisualStyles();
        int failures = 0;
        string[] names = { "fresh", "legacy-missing", "explicit-off", "explicit-on", "copy-off-text-missing" };
        foreach (string name in names)
        {
            try {
                string caseRoot = Path.Combine(root, name), package = Path.Combine(caseRoot, "package"), documents = Path.Combine(caseRoot, "documents");
                Directory.CreateDirectory(package); Directory.CreateDirectory(documents);
                bool fresh = name == "fresh", textExpected = name != "explicit-off", copyExpected = name != "copy-off-text-missing";
                string ini = Path.Combine(package, "DeadSpace-Launcher.ini");
                string original = "scale=3x 清晰\r\ndisplay_resolution=1920 × 1080\r\ngpu_present_copy=" + (copyExpected ? "1" : "0") + "\r\n";
                if (name == "explicit-off") original += "gpu_text_overlay=0\r\n";
                if (name == "explicit-on") original += "gpu_text_overlay=1\r\n";
                if (!fresh) File.WriteAllText(ini, original);
                object hardware = Activator.CreateInstance(hardwareType, true);
                hardwareType.GetField("Width", Hidden).SetValue(hardware, 1920);
                hardwareType.GetField("Height", Hidden).SetValue(hardware, 1080);
                using (var form = (Form)Activator.CreateInstance(formType, Hidden, null, new object[] { package, documents, hardware }, null))
                {
                    try {
                        Check(!form.Visible, "Test form became visible");
                        if (!fresh) Check(File.ReadAllText(ini) == original, "Loading rewrote existing INI");
                        Check(((CheckBox)Field(form, "gpuTextOverlay")).Checked == textExpected, name + ": wrong text default");
                        Check(((CheckBox)Field(form, "gpuPresentCopy")).Checked == copyExpected, name + ": copy preference changed");
                        if (!fresh) Check(Convert.ToString(((ComboBox)Field(form, "scaleHack")).SelectedItem) == "3x 清晰", "Manual resolution changed");
                        Call(form, "SaveSettings"); Call(form, "LoadSettings");
                        Check(((CheckBox)Field(form, "gpuTextOverlay")).Checked == textExpected, "Saved text selection changed on reload");
                        Check(((CheckBox)Field(form, "gpuPresentCopy")).Checked == copyExpected, "Saved copy selection changed on reload");
                        string actualIni = (string)Field(form, "iniPath");
                        Check(Path.GetFullPath(actualIni).StartsWith(caseRoot + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase), "INI escaped private root");
                        Check(File.ReadAllText(actualIni).Contains("gpu_text_overlay=" + (textExpected ? "1" : "0")), "Effective text mode not persisted");
                    }
                    finally { var timer = (Timer)Field(form, "gameTimer"); timer.Stop(); timer.Dispose(); }
                }
                Console.WriteLine("PASS " + name);
            }
            catch (Exception error) { failures++; Console.Error.WriteLine("CASE FAIL " + name + ": " + error.Message); }
        }
        Check(failures == 0, failures + " case(s) failed");
        Console.WriteLine("PASS 5/5 real hidden launcher load/save/reload cases; no game launch");
    }
}
