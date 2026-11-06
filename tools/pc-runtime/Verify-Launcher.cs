using System;
using System.Drawing;
using System.Drawing.Imaging;
using System.IO;
using System.Reflection;
using System.Windows.Forms;
namespace DeadSpaceBootstrap
{
    internal static class VerifyLauncher
    {
        [STAThread]
        private static void Main(string[] args)
        {
            try { Run(args); }
            catch (Exception error) { Console.Error.WriteLine(error.GetType().FullName + ": " + error.Message); Environment.ExitCode = 1; }
        }

        private static void Run(string[] args)
        {
            string fixture = args[0], output = args[1]; Directory.CreateDirectory(output);
            var save = new SaveSettings(fixture); byte[] original = File.ReadAllBytes(fixture);
            for (int mask = 0; mask < 8; mask++)
            {
                byte[] edited = save.EditBytes((mask & 1) != 0, (mask & 2) != 0, (mask & 4) != 0);
                if (edited.Length != original.Length) throw new Exception("Length changed");
                int changed = 0; for (int i = 0; i < original.Length; i++) if (edited[i] != original[i]) changed++;
                if (changed > 7) throw new Exception("Unexpected edits outside 3 flags and checksum");
                string path = Path.Combine(output, "fixture-" + mask + ".sb"); File.WriteAllBytes(path, edited);
                var reread = new SaveSettings(path);
                if (reread.Enabled("attack") != ((mask & 1) != 0) || reread.Enabled("defense") != ((mask & 2) != 0) || reread.Enabled("loot") != ((mask & 4) != 0)) throw new Exception("Toggle mismatch");
                if (Convert.ToBase64String(reread.EditBytes(save.Enabled("attack"), save.Enabled("defense"), save.Enabled("loot"))) != Convert.ToBase64String(original)) throw new Exception("Round trip lost bytes");
            }
            byte[] corrupt = (byte[])original.Clone(); corrupt[corrupt.Length / 2] ^= 1;
            string bad = Path.Combine(output, "corrupt.sb"); File.WriteAllBytes(bad, corrupt);
            bool rejected = false; try { new SaveSettings(bad); } catch (InvalidDataException) { rejected = true; }
            if (!rejected) throw new Exception("Corruption accepted");
            if (args.Length > 2)
            {
                var checkpoint = new SaveDocument(args[2]);
                byte[] before = File.ReadAllBytes(args[2]);
                var changes = new System.Collections.Generic.Dictionary<string, string> {
                    { "player/upgrade_level_rig", "0" }, { "player/inventory/credits", "100000" }
                };
                byte[] edited = checkpoint.EditBytes(changes);
                string editedFile = Path.Combine(output, "armor-checkpoint.sb"); File.WriteAllBytes(editedFile, edited);
                var reread = new SaveDocument(editedFile);
                foreach (var field in checkpoint.Values.Values)
                    if (!changes.ContainsKey(field.Path) && reread.Values[field.Path].Value != field.Value) throw new Exception("Unrelated save value changed: " + field.Path);
                var reverse = new System.Collections.Generic.Dictionary<string, string>();
                foreach (string path in changes.Keys) reverse.Add(path, checkpoint.Values[path].Value);
                if (Convert.ToBase64String(reread.EditBytes(reverse)) != Convert.ToBase64String(before)) throw new Exception("Armor/resource round trip lost bytes");
            }
            Application.EnableVisualStyles();
            using (var form = new LauncherForm())
            {
                // Never Show()/Activate(): renders only this constructed form.
                // Set managed visibility only: no ShowWindow and no foreground window.
                typeof(Control).GetMethod("SetState", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { 2, true });
                form.CreateControl();
                for (int i = 0; i < 8; i++)
                {
                    typeof(LauncherForm).GetMethod("SelectPage", BindingFlags.Instance | BindingFlags.NonPublic).Invoke(form, new object[] { i });
                    CheckLayout(form);
                    using (var bitmap = new Bitmap(form.Width, form.Height))
                    { form.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size)); bitmap.Save(Path.Combine(output, "page-" + i + ".jpg"), ImageFormat.Jpeg); }
                    var pages = (System.Collections.Generic.List<Panel>)typeof(LauncherForm).GetField("pages", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                    foreach (Control control in pages[i].Controls)
                    {
                        if (Convert.ToString(control.Tag) != "advanced-toggle") continue;
                        ((Button)control).PerformClick(); CheckLayout(form);
                        using (var bitmap = new Bitmap(form.Width, form.Height))
                        { form.DrawToBitmap(bitmap, new Rectangle(Point.Empty, bitmap.Size)); bitmap.Save(Path.Combine(output, "page-" + i + "-advanced.jpg"), ImageFormat.Jpeg); }
                        ((Button)control).PerformClick();
                    }
                }
                var preset = (ComboBox)typeof(LauncherForm).GetField("qualityPreset", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                var scale = (ComboBox)typeof(LauncherForm).GetField("scaleHack", BindingFlags.Instance | BindingFlags.NonPublic).GetValue(form);
                string beforeScale = Convert.ToString(scale.SelectedItem);
                for (int i = 0; i < 4; i++) { preset.SelectedIndex = i; if (Convert.ToString(scale.SelectedItem) != beforeScale) throw new Exception("Quality preset changed independent internal resolution"); }
            }
            Console.WriteLine("PASS: 8 DLC combinations, byte-exact reversal, corruption rejection, 8 offscreen pages plus expanded settings, no overlapping inputs, 4 working quality presets; live saves untouched.");
        }

        private static bool Input(Control c)
        { return c is ComboBox || c is CheckBox || c is TextBox || c is NumericUpDown || c is Button; }

        private static void CheckLayout(Control parent)
        {
            if (!parent.Visible) return;
            for (int i = 0; i < parent.Controls.Count; i++)
            {
                Control a = parent.Controls[i]; if (!a.Visible) continue;
                if (a is ComboBox && a.Width < 100) throw new Exception("Narrow dropdown: " + a.Text);
                if (Input(a)) for (int j = i + 1; j < parent.Controls.Count; j++)
                {
                    Control b = parent.Controls[j];
                    if (b.Visible && Input(b) && a.Bounds.IntersectsWith(b.Bounds)) throw new Exception("Overlapping inputs: " + a.Text + " / " + b.Text);
                }
                if (a is Panel) CheckLayout(a);
            }
        }
    }
}
