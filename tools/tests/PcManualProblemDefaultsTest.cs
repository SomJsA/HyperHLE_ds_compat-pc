using System;
using System.IO;
using System.Reflection;
using System.Diagnostics;
using System.Windows.Forms;
class PcManualProblemDefaultsTest {
    const BindingFlags F=BindingFlags.Instance|BindingFlags.NonPublic;
    static object Field(object x,string n){return x.GetType().GetField(n,F).GetValue(x);}
    static void Check(bool ok,string text){if(!ok)throw new Exception(text);}
    [STAThread] static int Main(string[] args){try{Run(args);return 0;}catch(Exception e){Console.Error.WriteLine(e);return 1;}}
    static void Run(string[] args){
        string root=Path.GetFullPath(args[1]);Check(!Directory.Exists(root),"Fresh test root required");Directory.CreateDirectory(root);
        var assembly=Assembly.LoadFrom(Path.GetFullPath(args[0]));var type=assembly.GetType("DeadSpaceBootstrap.LauncherForm",true);
        var ht=assembly.GetType("DeadSpaceBootstrap.DisplayHardware",true);Application.EnableVisualStyles();
        foreach(string name in new[]{"fresh","missing","legacy-on","explicit-off","explicit-on"}){
            string p=Path.Combine(root,name,"program"),d=Path.Combine(root,name,"documents");Directory.CreateDirectory(p);Directory.CreateDirectory(d);
            if(name!="fresh")File.WriteAllText(Path.Combine(p,"DeadSpace-Launcher.ini"),"scale=3x 清晰\r\n"+(name=="explicit-on"?"play_diagnostics=1\r\ndiagnostic_defaults_policy=manual-f7-20261107\r\n":name=="explicit-off"?"play_diagnostics=0\r\ndiagnostic_defaults_policy=manual-f7-20261107\r\n":name=="legacy-on"?"play_diagnostics=1\r\nsuper_monitor=1\r\n":""));
            var hardware=Activator.CreateInstance(ht,true);ht.GetField("Width",F).SetValue(hardware,1920);ht.GetField("Height",F).SetValue(hardware,1080);
            using(var form=(Form)Activator.CreateInstance(type,F,null,new[]{(object)p,d,hardware},null)){
                try{
                    bool automatic=name=="explicit-on";Check(((CheckBox)Field(form,"playDiagnostics")).Checked==automatic,"Wrong auto default: "+name);
                    Check(!((CheckBox)Field(form,"superMonitor")).Checked,"Monitor default enabled");
                    // This form fixture intentionally has no installed texture pack.
                    ((CheckBox)Field(form,"hdTextures")).Checked=false;
                    var info=new ProcessStartInfo();type.GetMethod("PreparePlayDiagnostics",F).Invoke(form,new object[]{info});
                    Check(info.EnvironmentVariables["TOUCHHLE_DS_DIAGNOSTICS"]==(automatic?"1":"0"),"Effective diagnostics mismatch");
                    Check(info.EnvironmentVariables["TOUCHHLE_DS_MANUAL_DIAGNOSTICS_ROOT"]==Path.Combine(p,"diagnostics","play-sessions"),"Manual export root missing");
                    if(!automatic){
                        Check(!Directory.Exists(Path.Combine(p,"diagnostics")),"Disabled preparation wrote diagnostic files/directories");
                        Check(info.EnvironmentVariables["TOUCHHLE_DS_SCENE_TRACE"]=="0"&&info.EnvironmentVariables["TOUCHHLE_DS_STUTTER_PROBE"]=="0","Heavy collectors enabled");
                        Check(info.EnvironmentVariables["TOUCHHLE_DS_DIAGNOSTICS_DIR"]==null,"Automatic session configured while off");
                    }
                    type.GetMethod("SaveSettings",F).Invoke(form,null);type.GetMethod("LoadSettings",F).Invoke(form,null);
                    Check(((CheckBox)Field(form,"playDiagnostics")).Checked==automatic,"Preference changed on reload");
                }finally{var timer=(Timer)Field(form,"gameTimer");timer.Stop();timer.Dispose();}
            }
            Console.WriteLine("PASS "+name);
        }
        Console.WriteLine("PASS5/5 hidden launcher defaults/legacy policy/effective environment/no diagnostic writes while off");
    }
}
