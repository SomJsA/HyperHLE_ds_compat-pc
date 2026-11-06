using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Reflection;
using System.Windows.Forms;
class PcRenderResolutionTest
{
    static BindingFlags Instance = BindingFlags.NonPublic | BindingFlags.Instance;
    static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
    static object Call(object owner, string name, params object[] args) { return owner.GetType().GetMethod(name, Instance).Invoke(owner, args); }
    static object Field(object owner, string name) { return owner.GetType().GetField(name, Instance).GetValue(owner); }
    [STAThread] static void Main(string[] args)
    {
        try { Run(args); }
        catch(Exception error) { Console.Error.WriteLine("FAIL "+error.GetType().FullName+": "+error.Message);
            if(error.InnerException!=null)Console.Error.WriteLine("INNER "+error.InnerException.GetType().FullName+": "+error.InnerException.Message);
            Environment.ExitCode=1; }
    }
    static void Run(string[] args)
    {
        var assembly = Assembly.LoadFrom(Path.GetFullPath(args[0]));
        var calculator = assembly.GetType("DeadSpaceBootstrap.PcRenderResolution", true);
        var calc = calculator.GetMethod("ScaleForHeight", BindingFlags.NonPublic | BindingFlags.Static);
        int[,] cases = {{1,2},{320,2},{321,2},{720,4},{1080,4},{1280,4},{1440,6},{2160,8},{4320,14}};
        for (int row=0; row<cases.GetLength(0); row++) {
            int scale=(int)calc.Invoke(null,new object[]{cases[row,0]});
            Check(scale==cases[row,1],"Height mapping wrong");
            Check(scale*320>=cases[row,0] && (scale-2)*320<cases[row,0] && scale%2==0,"Automatic resolution did not choose smallest covering even multiplier");
        }
        foreach (int bad in new[]{0,-1,Int32.MaxValue}) {
            bool rejected=false;try {calc.Invoke(null,new object[]{bad});}catch(TargetInvocationException){rejected=true;}
            Check(rejected,"Invalid/unrepresentable height accepted");
        }
        string ini=Path.Combine(AppDomain.CurrentDomain.BaseDirectory,"DeadSpace-Launcher.ini");
        Check(File.Exists(ini),"Isolated legacy INI must exist before constructing form; never touch user documents");
        var type=assembly.GetType("DeadSpaceBootstrap.LauncherForm",true);
        Application.EnableVisualStyles();
        using(var form=(Form)Activator.CreateInstance(type,true)) {
            var hardware=Field(form,"detectedHardware");
            var hf=BindingFlags.NonPublic|BindingFlags.Instance;
            hardware.GetType().GetField("Width",hf).SetValue(hardware,3840);
            hardware.GetType().GetField("Height",hf).SetValue(hardware,2160);
            Check(((ComboBox)Field(form,"msaaSamples")).SelectedIndex==2&&((ComboBox)Field(form,"anisotropy")).SelectedIndex==1&&((RadioButton)Field(form,"texture4x")).Checked&&((CheckBox)Field(form,"hdTextures")).Checked,"Fresh PC defaults must all be4x");
            var scale=(ComboBox)Field(form,"scaleHack");var output=(ComboBox)Field(form,"displayResolution");
            var summary=(Label)Field(form,"renderResolutionSummary");var quality=(ComboBox)Field(form,"qualityPreset");
            output.SelectedItem="自动（主屏分辨率）";quality.SelectedIndex=2;Call(form,"RefreshRenderResolutionSummary");
            Check((int)Call(form,"SelectedRenderScale")==8,"4K screen matching is not8x");
            Check(summary.Text.Contains("3840 × 2560")&&summary.Text.Contains("3840 × 2160"),"Resolved internal/output dimensions missing");
            Check(summary.Parent==(Panel)((List<Panel>)Field(form,"pages"))[0] && summary.Top==96 && summary.Height>=50,"Resolution result hidden in advanced controls");
            Check(scale.Width>=300&&scale.DropDownWidth>=400,"Resolution combo collapsed");
            var diagnostic=Call(form,"RenderResolutionDiagnostic");
            Check((int)diagnostic.GetType().GetProperty("reference_height").GetValue(diagnostic,null)==2160,"Diagnostic reference height wrong");
            Check((int)diagnostic.GetType().GetProperty("requested_internal_width").GetValue(diagnostic,null)==3840,"Diagnostic dimensions wrong");
            output.SelectedItem="1920 × 1080";
            Check((int)Call(form,"SelectedRenderScale")==8&&summary.Text.Contains("1920 × 1080"),"Output choice changed physical-screen internal resolution");
            for(int preset=0;preset<4;preset++){quality.SelectedIndex=preset;Check((int)Call(form,"SelectedRenderScale")==8,"Quality changed automatic internal resolution");}
            ((CheckBox)Field(form,"hdTextures")).Checked=true;((RadioButton)Field(form,"texture4x")).Checked=true;
            quality.SelectedIndex=2;
            Check(scale.Parent==summary.Parent,"Internal resolution remains inside advanced quality group");
            output.SelectedItem="自动（主屏分辨率）";Call(form,"SaveSettings");
            Check(File.ReadAllText(ini).Contains("scale=屏幕匹配（启动时识别）"),"Persisted calculated8x instead of automatic policy");
            hardware.GetType().GetField("Height",hf).SetValue(hardware,1440);Call(form,"LoadSettings");
            Check((int)Call(form,"SelectedRenderScale")==6,"Reload did not recompute1440p to6x");
            Call(form,"SelectRenderScale",3);Call(form,"SaveSettings");Call(form,"LoadSettings");
            Check((int)Call(form,"SelectedRenderScale")==3&&Convert.ToString(scale.SelectedItem)=="3x 清晰","Existing manual3x changed");
            for(int preset=0;preset<4;preset++){quality.SelectedIndex=preset;Check((int)Call(form,"SelectedRenderScale")==3,"Quality changed manual internal resolution");}
            Check(summary.Text.Contains("1440 × 960")&&summary.Text.Contains("手动"),"Manual actual size missing");
            hardware.GetType().GetField("Width",hf).SetValue(hardware,0);hardware.GetType().GetField("Height",hf).SetValue(hardware,0);
            output.SelectedItem="跟随内部";Call(form,"RenderResolutionDiagnostic");
            scale.SelectedItem="屏幕匹配（启动时识别）";
            bool unknown=false;try{Call(form,"SelectedRenderScale");}catch(TargetInvocationException e){unknown=e.InnerException is InvalidOperationException;}
            Check(unknown,"Unknown screen silently assigned fake native resolution");
            output.SelectedItem="1920 × 1080";Check((int)Call(form,"SelectedRenderScale")==4,"Explicit output must work when hardware height unknown");
            hardware.GetType().GetField("Width",hf).SetValue(hardware,3840);hardware.GetType().GetField("Height",hf).SetValue(hardware,2160);
            output.SelectedItem="自动（主屏分辨率）";quality.SelectedIndex=2;Call(form,"RefreshRenderResolutionSummary");
            typeof(Control).GetMethod("SetState",BindingFlags.NonPublic|BindingFlags.Instance).Invoke(form,new object[]{2,true});
            form.CreateControl();Call(form,"SelectPage",0);
            foreach(Control button in summary.Parent.Controls) if(button is Button && button.Top<90)
                foreach(Control label in summary.Parent.Controls) if(label is Label && label.Top<60)
                    Check(!button.Bounds.IntersectsWith(label.Bounds),"Hardware review button is covered by page heading");
            foreach(Control control in summary.Parent.Controls)
                if(control!=summary&&control.Top>=96&&Convert.ToString(control.Tag)!="advanced-content")
                    Check(!control.Bounds.IntersectsWith(summary.Bounds),"Native resolution readout overlaps "+control.GetType().Name);
            using(var bitmap=new Bitmap(form.Width,form.Height)){form.DrawToBitmap(bitmap,new Rectangle(Point.Empty,bitmap.Size));bitmap.Save(args[1]);}
        }
        Console.WriteLine("PASS explicit1080p4x-1440p6x-2160p8x / physical-screen independent of output / all presets preserve auto+manual resolution / default textures-MSAA-AF4x / basic visible internal-output controls / diagnostics / saved policy recompute / old manual / unknown-screen fallback / hidden layout");
    }
}
