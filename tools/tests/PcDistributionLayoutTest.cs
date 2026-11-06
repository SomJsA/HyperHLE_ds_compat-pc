using System;
using System.Collections;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Windows.Forms;

class PcDistributionLayoutTest
{
    const BindingFlags Static=BindingFlags.NonPublic|BindingFlags.Static, Instance=BindingFlags.NonPublic|BindingFlags.Instance;
    static void Check(bool ok,string message){if(!ok)throw new Exception(message);}
    static object Call(Type type,object owner,string name,params object[] args)
    {
        foreach(var m in type.GetMethods(owner==null?Static:Instance))if(m.Name==name && m.GetParameters().Length==args.Length)
            try{return m.Invoke(owner,args);}catch(TargetInvocationException error){throw error.InnerException;}
        throw new Exception("Missing method: "+name);
    }
    static object Field(object owner,string name){return owner.GetType().GetField(name,Instance).GetValue(owner);}
    static void Put(string path,string value){Directory.CreateDirectory(Path.GetDirectoryName(path));File.WriteAllText(path,value);}
    static Dictionary<string,string> Snapshot(string root)
    {var result=new Dictionary<string,string>();foreach(string path in Directory.GetFiles(root,"*",SearchOption.AllDirectories))result.Add(path.Substring(root.Length),Convert.ToBase64String(File.ReadAllBytes(path)));return result;}
    static void Same(Dictionary<string,string> expected,string root)
    {var actual=Snapshot(root);Check(expected.Count==actual.Count,"Original file count changed");foreach(var pair in expected)Check(actual[pair.Key]==pair.Value,"Original bytes changed: "+pair.Key);}
    static object Hardware(Type type,int width,int height)
    {var value=Activator.CreateInstance(type,true);type.GetField("Width",Instance).SetValue(value,width);type.GetField("Height",Instance).SetValue(value,height);return value;}
    static Form NewForm(Type type,string package,string documents,object hardware)
    {return (Form)Activator.CreateInstance(type,Instance,null,new[]{(object)package,documents,hardware},null);}
    static void NoTestControls(Control control)
    {Check(!control.Text.Contains("短测")&&!control.Text.Contains("请求测试"),"Obsolete test UI remains");foreach(Control child in control.Controls)NoTestControls(child);}
    [STAThread]static int Main(string[] args)
    {
        try{Run(args);return 0;}catch(Exception e){Console.Error.WriteLine(e);return 1;}
    }
    static void Run(string[] args)
    {
        var assembly=Assembly.LoadFrom(Path.GetFullPath(args[0]));
        var formType=assembly.GetType("DeadSpaceBootstrap.LauncherForm",true);
        var hardwareType=assembly.GetType("DeadSpaceBootstrap.DisplayHardware",true);
        var storage=assembly.GetType("DeadSpaceBootstrap.DocumentsSettings",true);
        var layout=assembly.GetType("DeadSpaceBootstrap.DistributionLayout",true);
        string output=Path.GetFullPath(args[1]);Check(!Directory.Exists(output),"Use a new fixture directory");Directory.CreateDirectory(output);
        string package=Path.Combine(output,"Package"),documents=Path.Combine(output,"Documents");
        Put(Path.Combine(package,"Game","touchHLE.exe"),"fixture only; never launched");
        Put(Path.Combine(package,"Game","DeadSpace.ipa"),"unchanged original fixture");
        Put(Path.Combine(package,"Assets","texture-packs","4","PACK.json"),"{}");
        Put(Path.Combine(package,"Assets","texture-packs","4","example.png"),"fixture presence only");
        Put(Path.Combine(package,"Defaults","DeadSpace-Launcher.ini"),"save_mode=文档（当前用户）\nscale=3x 清晰\ntexture_scale=2\n");
        // A fresh install must override the template's old graphics once, then
        // persist both resolved display choices without a benchmark prompt.
        using(var form=NewForm(formType,package,documents,Hardware(hardwareType,1920,1080)))
        {
            NoTestControls(form);
            Check(((ComboBox)Field(form,"windowMode")).SelectedIndex==2,"Fresh distribution must default to fullscreen");
            Check((int)Call(formType,form,"SelectedRenderScale")==4,"First1080p preset not4x");
            Check((string)Call(formType,form,"SelectedOutputSize")=="1920x1080","First output not physical display");
            Check(((RadioButton)Field(form,"texture4x")).Checked&&((CheckBox)Field(form,"hdTextures")).Checked,"First texture not4x");
            Check(((ComboBox)Field(form,"msaaSamples")).SelectedIndex==2&&((ComboBox)Field(form,"anisotropy")).SelectedIndex==1,"First filtering not4x");
            Check(((string)Call(formType,form,"SelectedDataRoot")).Equals(Path.Combine(documents,"DeadSpaceLegacy"),StringComparison.OrdinalIgnoreCase),"Default data not English Documents root");
            var info=new ProcessStartInfo();info.EnvironmentVariables["TOUCHHLE_DS_BENCHMARK"]="1";
            Call(formType,form,"PrepareStartupReview",info);Check(info.EnvironmentVariables["TOUCHHLE_DS_BENCHMARK"]=="0","Inherited benchmark enabled");
            Call(formType,form,"PreparePlayDiagnostics",info);
            string session=info.EnvironmentVariables["TOUCHHLE_DS_DIAGNOSTICS_DIR"];
            Check(session!=null&&session.StartsWith(Path.Combine(package,"diagnostics","play-sessions")+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase),"New diagnosis not program-local");
            Check(!Directory.Exists(Path.Combine(documents,"DeadSpaceLegacy","diagnostics")),"Diagnosis leaked into Documents");
            Check(File.Exists(Path.Combine(session,"host.json")),"No real launcher session manifest");
            Check((string)Call(formType,form,"SelectedTextureRoot")==Path.Combine(package,"Assets","texture-packs","4"),"Pack path not relocated");
            Call(formType,form,"SelectRenderScale",3);((ComboBox)Field(form,"fpsCap")).SelectedIndex=2;
            ((ComboBox)Field(form,"windowMode")).SelectedIndex=0;
            ((ComboBox)Field(form,"displayResolution")).SelectedItem="1920 × 1080";
            Call(formType,form,"SaveSettings");
        }
        string config=Path.Combine(documents,"DeadSpaceLegacy","config","pc","DeadSpace-Launcher.ini");
        string saved=File.ReadAllText(config);Check(saved.Contains("scale=3x 清晰")&&saved.Contains("ipa=Game"),"Saved choices/reference missing");
        using(var form=NewForm(formType,package,documents,Hardware(hardwareType,3840,2160)))
        {
            Check((int)Call(formType,form,"SelectedRenderScale")==3,"Second launch reset manually saved resolution");
            Check((string)Call(formType,form,"SelectedOutputSize")=="1920x1080","Second launch reset saved output");
            Check(((ComboBox)Field(form,"fpsCap")).SelectedIndex==2,"Second launch reset cap");
            Check(((ComboBox)Field(form,"windowMode")).SelectedIndex==0,"Second launch reset saved window choice");
            Check(File.ReadAllText(config)==saved,"Constructing second launch rewrote settings");
            NoTestControls(form);
        }
        foreach(int height in new[]{1440,2160})using(var form=NewForm(formType,package,Path.Combine(output,"Fresh"+height),Hardware(hardwareType,height*16/9,height)))
            Check((int)Call(formType,form,"SelectedRenderScale")== (height==1440?6:8),"Fresh height mapping incorrect");
        // Legacy migration preserves all progress, backups, configs and empty
        // directories. Old diagnosis stays read-only in its original location.
        string oldDocs=Path.Combine(output,"OldDocuments"),oldRoot=Path.Combine(oldDocs,"DeadSpace IOS移植");
        Put(Path.Combine(oldRoot,"config","pc","DeadSpace-Launcher.ini"),"scale=3x 清晰\ncustom=retain\n");
        Put(Path.Combine(oldRoot,"touchHLE_sandbox","com.ea.deadspace.bv","Documents","var","save.sb"),"original save");
        Put(Path.Combine(oldRoot,"touchHLE_sandbox","com.ea.deadspace.bv","Documents","var",".save-editor-backups","b1","save.sb"),"backup");
        Put(Path.Combine(oldRoot,"diagnostics","play-sessions","session-old","host.json"),"{\"schema\":1,\"kind\":\"deadspace-play-diagnostics\"}");
        Directory.CreateDirectory(Path.Combine(oldRoot,"empty"));var before=Snapshot(oldRoot);
        Call(storage,null,"Resolve",package,oldDocs);string newRoot=Path.Combine(oldDocs,"DeadSpaceLegacy");
        Same(before,oldRoot);Check(Directory.Exists(Path.Combine(newRoot,"empty")),"Empty directory lost");
        Check(File.ReadAllText(Path.Combine(newRoot,"config","pc","DeadSpace-Launcher.ini")).Contains("custom=retain"),"Migrated configuration lost");
        Check(File.ReadAllText(Path.Combine(newRoot,"touchHLE_sandbox","com.ea.deadspace.bv","Documents","var",".save-editor-backups","b1","save.sb"))=="backup","Backup lost");
        Check(!Directory.Exists(Path.Combine(newRoot,"diagnostics")),"Old diagnostic copied to user data");
        Put(Path.Combine(oldRoot,"config","pc","DeadSpace-Launcher.ini"),"do not merge");
        Call(storage,null,"Resolve",package,oldDocs);
        Check(File.ReadAllText(Path.Combine(newRoot,"config","pc","DeadSpace-Launcher.ini")).Contains("custom=retain"),"Existing English root merged/overwritten");
        using(var form=NewForm(formType,package,oldDocs,Hardware(hardwareType,1920,1080)))
        {Check(((IList)Call(formType,form,"AllDiagnosticSessions")).Contains(Path.Combine(oldRoot,"diagnostics","play-sessions","session-old")),"Historical diagnosis no longer exportable");}
        string badDocs=Path.Combine(output,"BadDocuments"),badRoot=Path.Combine(badDocs,"DeadSpace IOS移植");
        Put(Path.Combine(badRoot,"storage-layout.json"),"{\"schema\":999}");var badBefore=Snapshot(badRoot);bool refused=false;
        try{Call(storage,null,"Resolve",package,badDocs);}catch(InvalidDataException){refused=true;}
        Check(refused&&!Directory.Exists(Path.Combine(badDocs,"DeadSpaceLegacy")),"Unknown old schema copied/published");Same(badBefore,badRoot);
        string external=Path.Combine(output,"external.ipa");Check((string)Call(layout,null,"ResolveIpa",package,external)==external,"Explicit external IPA replaced");
        Check((string)Call(layout,null,"ResolveIpa",package,"DeadSpace.ipa")==Path.Combine(package,"Game","DeadSpace.ipa"),"Old relative IPA default not relocated");
        Check(File.ReadAllText(Path.Combine(package,"Game","DeadSpace.ipa"))=="unchanged original fixture","Original IPA fixture changed");
        Console.WriteLine("PASS first monitor presets1080/1440/2160; restart retains config; no test UI/inherited benchmark; English Documents saves/config/backups; program-local diagnostic session; packs/IPA relocation; verified legacy copy/no merge/original preserved/unknown schema refusal; old diagnostic export");
    }
}
