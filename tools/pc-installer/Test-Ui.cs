using System;
using System.Collections.Generic;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Reflection;
using System.Windows.Forms;

internal static class TestUi
{
    private static readonly BindingFlags Private = BindingFlags.NonPublic | BindingFlags.Instance;
    private static readonly List<string> errors = new List<string>();
    private static void Check(Control parent, string path)
    {
        if (parent is TextBoxBase || parent is ComboBox || parent is NumericUpDown) return;
        foreach (Control control in parent.Controls)
        {
            if (!control.Visible) continue;
            string name = control.Text.Replace("\r\n", " / ");
            var label = control as Label;
            if (label != null && !label.AutoSize && !label.AutoEllipsis && label.Text.Length > 0) {
                int height = TextRenderer.MeasureText(label.Text, label.Font, new Size(label.ClientSize.Width, 10000), TextFormatFlags.WordBreak).Height;
                if (height > label.ClientSize.Height + 2) errors.Add(path + " clipped label: " + name + " required=" + height + " actual=" + label.Height);
            }
            foreach (Control other in parent.Controls) {
                if (other == control) break;
                if (!other.Visible || control is Panel || other is Panel || control is ProgressBar || other is ProgressBar) continue;
                Rectangle overlap = Rectangle.Intersect(control.Bounds, other.Bounds);
                if (overlap.Width > 2 && overlap.Height > 2) errors.Add(path + " overlap: " + name + " / " + other.Text.Replace("\r\n", " / "));
            }
            if (!(parent is ScrollableControl) || !((ScrollableControl)parent).AutoScroll) {
                if (control.Right > parent.ClientSize.Width + 2 || control.Bottom > parent.ClientSize.Height + 2 || control.Left < -2 || control.Top < -2)
                    errors.Add(path + " outside parent: " + name);
            }
            Check(control, path + "/" + control.GetType().Name);
        }
    }
    private static void Visible(Form form)
    {
        typeof(Control).GetMethod("SetState", Private).Invoke(form, new object[] { 2, true });
        form.CreateControl(); form.PerformLayout();
    }
    private static void Shot(Form form, string file) {
        using (var bitmap = new Bitmap(form.Width, form.Height)) {
            form.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size));
            bitmap.Save(file, ImageFormat.Jpeg);
        }
    }
    [STAThread]
    private static int Main(string[] args)
    {
        try { return Run(args); }
        catch (Exception error) { while (error.InnerException != null) error = error.InnerException; Console.WriteLine(error.GetType().FullName + ": " + error.Message); return 2; }
    }
    private static int Run(string[] args)
    {
        Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false);
        string output = Path.GetFullPath(args[2]); Directory.CreateDirectory(output);
        Assembly assembly = Assembly.LoadFrom(Path.GetFullPath(args[0]));
        bool launcher = args[1] == "launcher";
        Type type = assembly.GetType(launcher ? "DeadSpaceBootstrap.LauncherForm" : "DeadSpaceInstaller.InstallerForm", true);
        foreach (float scale in new[] { 1f, 1.25f, 1.5f, 2f }) {
            Form form;
            if (launcher) {
                string package = Path.Combine(output, "package"); string documents = Path.Combine(output, "documents");
                Directory.CreateDirectory(package); Directory.CreateDirectory(documents);
                var hardware = Activator.CreateInstance(assembly.GetType("DeadSpaceBootstrap.DisplayHardware"), true);
                foreach (var item in new Dictionary<string, object> { { "Width", 1920 }, { "Height", 1080 }, { "GpuName", "UI-test" }, { "CpuName", "UI-test" }, { "DedicatedBytes", 8589934592UL } })
                    hardware.GetType().GetField(item.Key, Private | BindingFlags.Public).SetValue(hardware, item.Value);
                form = (Form)Activator.CreateInstance(type, Private, null, new[] { (object)package, documents, hardware }, null);
            } else form = (Form)Activator.CreateInstance(type, true);
            using (form) {
                Visible(form);
                if (scale != 1) form.Scale(new SizeF(scale, scale));
                form.PerformLayout();
                int pageCount = launcher ? 7 : 1;
                for (int page = 0; page < pageCount; page++) {
                    if (launcher) type.GetMethod("SelectPage", Private).Invoke(form, new object[] { page });
                    string scope = args[1] + " scale=" + scale + " page=" + page;
                    Check(form, scope);
                    if (scale == 1 && (page == 0 || page == 4 || page == 7)) Shot(form, Path.Combine(output, "page-" + page + ".jpg"));
                    if (launcher) {
                        var pages = (System.Collections.IList)type.GetField("pages", Private).GetValue(form);
                        foreach (Control c in ((Control)pages[page]).Controls) if (Convert.ToString(c.Tag) == "advanced-toggle") {
                            ((Button)c).PerformClick(); Check(form, scope + " expanded");
                            ((Button)c).PerformClick();
                        }
                    }
                }
                form.ClientSize = new Size((int)(1000 * scale), (int)(620 * scale));
                if (launcher) ((Button)type.GetField("stopGame", Private).GetValue(form)).Enabled = true;
                form.PerformLayout(); Check(form, args[1] + " compact scale=" + scale);
                form.ClientSize = new Size(Math.Min(1920, (int)(960 * scale)), Math.Min(1000, (int)(580 * scale)));
                form.PerformLayout(); Check(form, args[1] + " bounded viewport scale=" + scale);
            }
        }
        File.WriteAllLines(Path.Combine(output, "layout-errors.txt"), errors);
        Console.WriteLine("UI geometry: " + errors.Count + " findings; output=" + output);
        for (int i = 0; i < Math.Min(12, errors.Count); i++) Console.WriteLine(errors[i]);
        return errors.Count == 0 ? 0 : 1;
    }
}

