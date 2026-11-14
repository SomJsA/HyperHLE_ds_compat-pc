using System;
using System.IO;
using System.Reflection;
using System.Drawing;
using System.Drawing.Imaging;
using System.Windows.Forms;

internal static class ResourceUiTest
{
    private const BindingFlags Hidden = BindingFlags.NonPublic | BindingFlags.Instance;
    private static int checks;
    private static void Check(bool value, string message) { if (!value) throw new Exception(message); checks++; }
    private static object Field(object o, string name) { return o.GetType().GetField(name, Hidden).GetValue(o); }
    [STAThread] private static int Main(string[] args)
    {
        try {
            Application.EnableVisualStyles();
            string output = Path.GetFullPath(args[2]); Directory.CreateDirectory(output);
            var installer = Assembly.LoadFrom(Path.GetFullPath(args[0])); var launcher = Assembly.LoadFrom(Path.GetFullPath(args[1]));
            foreach (bool english in new[] { false, true }) {
                installer.GetType("DeadSpaceUi.UiLanguage").GetField("english", BindingFlags.Static | BindingFlags.NonPublic).SetValue(null, english);
                using (var form = (Form)Activator.CreateInstance(installer.GetType("DeadSpaceInstaller.InstallerForm"), true)) {
                    typeof(Control).GetMethod("SetState", Hidden).Invoke(form, new object[] { 2, true }); form.CreateControl();
                    ((TabControl)Field(form, "sections")).SelectedIndex = 1;
                    for (int i = 0; i < 5; i++) {
                        ((TextBox)Field(form, "resourcePath")).Text = args[3 + i]; form.PerformLayout();
                        var p = Field(form, "sourceProfile"); Check(p != null, "Profile visible " + i);
                        string id = Convert.ToString(p.GetType().GetField("Id", Hidden).GetValue(p));
                        Check(id == new[] { "ipad-1.0.3", "iphone-1.0.3", "world-1.0.1", "current", "current" }[i], "UI exact profile");
                        var text = (CheckBox)Field(form, "hdChinesePack"); Check(text.Enabled == (i >= 3), "HD text gating");
                        foreach (CheckBox tier in (CheckBox[])Field(form, "tiers")) Check(tier.Enabled == (i == 4), "Texture tier availability");
                        string hint = ((Label)Field(form, "sourceHint")).Text;
                        Check(hint.Contains("1.3.34") && hint.Contains(english ? "Lighting:" : "光影："), "Bilingual inline compatibility");
                        if ((!english && i == 0) || (english && i == 1)) using (var image = new Bitmap(form.Width, form.Height)) { form.DrawToBitmap(image, new Rectangle(Point.Empty, image.Size)); image.Save(Path.Combine(output, english ? "installer-phone-en.jpg" : "installer-ipad-zh.jpg"), ImageFormat.Jpeg); }
                    }
                }
            }
            string package = Path.Combine(output, "package"), documents = Path.Combine(output, "documents"); Directory.CreateDirectory(package); Directory.CreateDirectory(documents);
            var type = launcher.GetType("DeadSpaceBootstrap.LauncherForm"); var hw = Activator.CreateInstance(launcher.GetType("DeadSpaceBootstrap.DisplayHardware"), true);
            hw.GetType().GetField("Width", Hidden | BindingFlags.Public).SetValue(hw, 1920); hw.GetType().GetField("Height", Hidden | BindingFlags.Public).SetValue(hw, 1080);
            using (var form = (Form)Activator.CreateInstance(type, Hidden, null, new object[] { package, documents, hw }, null)) {
                var apply = type.GetMethod("ApplyResourceCompatibility", Hidden);
                foreach (int i in new[] { 0, 1, 2, 3, 1, 3 }) {
                    apply.Invoke(form, new object[] { args[3 + i], true });
                    Check(((CheckBox)Field(form, "advancedLighting")).Enabled == (i == 0 || i == 3), "Launcher lighting gating");
                    Check(((CheckBox)Field(form, "overlayEnabled")).Enabled == (i == 3), "Launcher text gating");
                    Check(((CheckBox)Field(form, "nativeInputs")).Checked == (i == 3), "Native controls switch");
                    Check(!(bool)Field(form, "launchReviewCompleted"), "Resource change requires first review");
                    string saves = Convert.ToString(type.GetMethod("SelectedDataRoot", Hidden).Invoke(form, null));
                    Check(saves.Contains("legacy-ipa") == (i != 3), "Legacy saves cannot share 1.3.34 progress");
                    Check(((Control)Field(form, "saveEditorPage")).Enabled == (i == 3), "Unsupported legacy save editor disabled");
                }
            }
            Console.WriteLine("PASS " + checks + " checks; installer ZH/EN identities, availability, launcher switching / review. No desktop input or installation."); return 0;
        } catch (Exception e) { Console.Error.WriteLine(e); return 1; }
    }
}
