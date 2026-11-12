using System;
using System.Collections;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Reflection;
using System.Text.RegularExpressions;
using System.Windows.Forms;

// Runs against the compiled launcher, uses private copies and never shows a
// window or starts the game. Usage: test.exe launcher.exe output fixture.sb en|zh
internal static class PcEyeModeTest
{
    private const BindingFlags Private = BindingFlags.Instance | BindingFlags.NonPublic;
    private const BindingFlags Static = BindingFlags.Static | BindingFlags.NonPublic;
    private static void Check(bool ok, string message) { if (!ok) throw new Exception(message); }
    private static object Get(object instance, string name) { return instance.GetType().GetField(name, Private).GetValue(instance); }
    private static object Call(object instance, string name) { return instance.GetType().GetMethod(name, Private).Invoke(instance, null); }
    private static IEnumerable<Control> Walk(Control root)
    {
        yield return root;
        foreach (Control child in root.Controls) foreach (Control next in Walk(child)) yield return next;
    }
    private static void Render(Form form, string file)
    {
        typeof(Control).GetMethod("SetState", Private).Invoke(form, new object[] { 2, true });
        form.CreateControl(); form.PerformLayout();
        using (var bitmap = new Bitmap(form.Width, form.Height))
        { form.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size)); bitmap.Save(file, ImageFormat.Jpeg); }
    }
    [STAThread]
    private static int Main(string[] args)
    {
        try
        {
            Environment.SetEnvironmentVariable("TOUCHHLE_UI_LANGUAGE", args[3]);
            Application.EnableVisualStyles();
            string output = Path.GetFullPath(args[1]); Directory.CreateDirectory(output);
            string package = Path.Combine(output, "package"), documents = Path.Combine(output, "documents");
            Directory.CreateDirectory(package); Directory.CreateDirectory(documents);
            string ini = Path.Combine(package, "DeadSpace-Launcher.ini");
            File.WriteAllText(ini, "ipa=\nplay_diagnostics=0\nsuper_monitor=0\ndebug_logs=0\nframe_dump=0\n");
            var assembly = Assembly.LoadFrom(Path.GetFullPath(args[0]));
            var hardwareType = assembly.GetType("DeadSpaceBootstrap.DisplayHardware", true);
            var hardware = Activator.CreateInstance(hardwareType, true);
            hardwareType.GetField("Width", Private).SetValue(hardware, 1920);
            hardwareType.GetField("Height", Private).SetValue(hardware, 1080);
            var type = assembly.GetType("DeadSpaceBootstrap.LauncherForm", true);
            using (var form = (Form)Activator.CreateInstance(type, Private, null, new[] { (object)package, documents, hardware }, null))
            {
                Check(!form.Visible, "Launcher was shown");
                Check(!((CheckBox)Get(form, "safeBlackFloor")).Checked, "Eye mode default must be off");
                Check(((TrackBar)Get(form, "safeBlackGray")).Value == 10, "Missing gray setting must default to 10");
                foreach (string field in new[] { "debugLogs", "frameDump", "playDiagnostics", "superMonitor" })
                    Check(!((CheckBox)Get(form, field)).Checked, "Diagnostic default enabled: " + field);
                ((TrackBar)Get(form, "safeBlackGray")).Value = 23;
                ((CheckBox)Get(form, "safeBlackFloor")).Checked = true;
                Call(form, "SaveSettings");
                ((TrackBar)Get(form, "safeBlackGray")).Value = 10;
                Call(form, "LoadSettings");
                Check(((TrackBar)Get(form, "safeBlackGray")).Value == 23 && ((CheckBox)Get(form, "safeBlackFloor")).Checked, "Explicit eye settings lost on reload");
                Call(form, "ApplyDefaults");
                Check(((TrackBar)Get(form, "safeBlackGray")).Value == 10 && !((CheckBox)Get(form, "safeBlackFloor")).Checked, "Reset must return to 10/off");

                var info = new System.Diagnostics.ProcessStartInfo();
                string stateRoot = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "DeadSpaceLegacy");
                var triggerBackup = new Dictionary<string, byte[]>();
                foreach (string name in new[] { "all-pending-skin.trigger", "all-pending-transform.trigger", "all-pending-animation-position.trigger" })
                { string path = Path.Combine(stateRoot, name); triggerBackup[path] = File.Exists(path) ? File.ReadAllBytes(path) : null; }
                try { type.GetMethod("PrepareDefaultOptimizationEnvironment", Private).Invoke(form, new object[] { info }); }
                finally { foreach (var item in triggerBackup) { if (item.Value == null) File.Delete(item.Key); else File.WriteAllBytes(item.Key, item.Value); } }
                foreach (string env in new[] { "TOUCHHLE_DS_PC_JIT_PREWARM", "TOUCHHLE_DS_PC_COMBAT_LATENCY", "TOUCHHLE_DS_PC_TRANSFORM_CACHE", "TOUCHHLE_DS_PC_ANIMATION_POSITION_CACHE", "TOUCHHLE_DS_GL_STACK_DRIVER", "TOUCHHLE_DS_GL_ENV_CACHE" })
                    Check(info.EnvironmentVariables[env] == "1", "Optimization missing: " + env);
                Check(info.EnvironmentVariables["TOUCHHLE_DS_PC_SKIN_MATH"] == "host" && info.EnvironmentVariables["TOUCHHLE_DS_PC_TRANSFORM_MATH"] == "host", "Host paths missing");

                var docType = assembly.GetType("DeadSpaceBootstrap.SaveDocument", true);
                var document = Activator.CreateInstance(docType, Private, null, new object[] { Path.GetFullPath(args[2]) }, null);
                type.GetField("editorDocument", Private).SetValue(form, document);
                var grid = (DataGridView)Get(form, "saveGrid");
                var groups = (ComboBox)Get(form, "editorGroup");
                string[] category = { "资源与升级", "武器", "背包", "内购插件", "章节与周目", "共享设置", "进度（只读）", "全部" };
                var values = (IDictionary)docType.GetField("Values", Private).GetValue(document);
                for (int index = 0; index < category.Length; index++)
                {
                    groups.SelectedIndex = index; Call(form, "PopulateSaveGrid");
                    int count = 0;
                    foreach (DictionaryEntry entry in values)
                    {
                        string group = (string)entry.Value.GetType().GetField("Group", Private).GetValue(entry.Value);
                        if (index == 7 || group == category[index]) count++;
                    }
                    Check(grid.Rows.Count == count, "Translated category filter lost fields: " + category[index]);
                }
                var language = assembly.GetType("DeadSpaceUi.UiLanguage", true);
                var pending = (IDictionary)Get(form, "pendingSaveChanges");
                pending["player/credits"] = "123";
                language.GetMethod("Toggle", Static).Invoke(null, null);
                language.GetMethod("Apply", Static).Invoke(null, new object[] { form });
                language.GetMethod("Toggle", Static).Invoke(null, null);
                language.GetMethod("Apply", Static).Invoke(null, new object[] { form });
                Check(Object.ReferenceEquals(Get(form, "editorDocument"), document) && Convert.ToString(pending["player/credits"]) == "123", "Language toggle discarded the loaded save or pending changes");
                var pages = (IList)Get(form, "pages");
                foreach (Control control in Walk((Control)pages[5]))
                    if (control is Label && control.Text.Length > 0)
                        Check(TextRenderer.MeasureText(control.Text, control.Font, new Size(control.Width, 10000), TextFormatFlags.WordBreak).Height <= control.Height + 2, "Editor text clipped: " + control.Text);
                if (args[3] == "en")
                {
                    int untranslated = 0;
                    foreach (Control control in Walk((Control)pages[5]))
                        if (control is Label || control is Button || control is CheckBox)
                            if (Regex.IsMatch(control.Text, "[\\u4e00-\\u9fff]")) { Console.WriteLine("Untranslated editor text: " + control.Text); untranslated++; }
                    foreach (DataGridViewColumn column in grid.Columns) Check(!Regex.IsMatch(column.HeaderText, "[\\u4e00-\\u9fff]"), "Untranslated editor header");
                    foreach (string field in new[] { "editorSlot", "editorKind", "editorGroup" })
                        foreach (object item in ((ComboBox)Get(form, field)).Items)
                            Check(!Regex.IsMatch(Convert.ToString(item), "[\\u4e00-\\u9fff]"), "Untranslated editor category: " + item);
                    Check(untranslated == 0, "Untranslated editor controls: " + untranslated);
                }
                // Language changes cannot affect category identity or native values.
                type.GetMethod("SelectPage", Private).Invoke(form, new object[] { 5 });
                Render(form, Path.Combine(output, "editor.jpg"));
            }
            using (var warning = (Form)Activator.CreateInstance(assembly.GetType("DeadSpaceBootstrap.FlickerWarningForm", true), true))
            {
                Check(((Button)warning.AcceptButton).DialogResult == DialogResult.OK, "Continue does not confirm");
                Check(((Button)warning.CancelButton).DialogResult == DialogResult.Cancel, "Back does not cancel");
                foreach (Control c in Walk(warning))
                    if (c is Label) Check(TextRenderer.MeasureText(c.Text, c.Font, new Size(c.Width, 10000), TextFormatFlags.WordBreak).Height <= c.Height, "Warning text clipped");
                Render(warning, Path.Combine(output, "warning.jpg"));
            }
            Console.WriteLine("PASS " + args[3] + ": default/reset 10+off, settings roundtrip, diagnostics off, optimization environment, eight real-save category filters, editor text and warning controls/layout");
            return 0;
        }
        catch (Exception error) { while (error.InnerException != null) error = error.InnerException; Console.WriteLine(error); return 1; }
    }
}
