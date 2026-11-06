using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Reflection;
using System.Text;
class PlayDiagnosticsTest
{
    static object Call(Type t,string name,params object[] args){try{return t.GetMethod(name,BindingFlags.Static|BindingFlags.NonPublic).Invoke(null,args);}catch(TargetInvocationException error){throw error.InnerException;}}
    static void Check(bool ok,string text){if(!ok)throw new Exception(text);}
    static void Main(string[] args)
    {
        var a=Assembly.LoadFrom(Path.GetFullPath(args[0]));var t=a.GetType("DeadSpaceBootstrap.DiagnosticFiles",true);
        string root=Path.GetFullPath(args[1]);Check(!Directory.Exists(root),"Test output must be new");Directory.CreateDirectory(root);
        string sessions=(string)Call(t,"Root",root);Directory.CreateDirectory(sessions);
        string bad=Path.Combine(sessions,"session-interrupted"),good=Path.Combine(sessions,"session-normal");Directory.CreateDirectory(bad);Directory.CreateDirectory(good);
        foreach(string session in new[]{bad,good})Call(t,"WriteNew",Path.Combine(session,"host.json"),new {schema=1,kind="deadspace-play-diagnostics",privacy="no personal paths"});
        Call(t,"WriteNew",Path.Combine(good,"session-end.json"),new {schema=1,normal_exit_observed=true});
        Check(Call(t,"ShutdownStatus",bad)==null,"Missing end marker invented a shutdown result");
        Check(((string)Call(t,"ShutdownStatus",good)).Contains("旧记录"),"Legacy end marker falsely certified full persistence");
        string statusRoot=Path.Combine(root,"shutdown-status-cases");Directory.CreateDirectory(statusRoot);
        Action<string,object,string> shutdownCase=(name,value,expected)=>{
            string path=Path.Combine(statusRoot,name);Directory.CreateDirectory(path);
            Call(t,"WriteNew",Path.Combine(path,"health.json"),new {writer_error=(string)null,detail_level=0,persisted_frame_records=100,persisted_event_records=100});
            Call(t,"WriteNew",Path.Combine(path,"session-end.json"),value);
            string text=(string)Call(t,"ShutdownStatus",path);
            Check(text.Contains(expected)&&!text.Contains("逐帧记录"),"Shutdown marker did not supersede stale healthy status: "+name);
        };
        shutdownCase("drained",new {normal_exit_observed=true,writer_error=(string)null,queued_tail_not_persisted=0,shutdown_senders_inflight=0,shutdown_queue_drained=true,shutdown_drain_timed_out=false},"最终写入成功");
        shutdownCase("old-tail2",new {normal_exit_observed=true,writer_error=(string)null,queued_tail_not_persisted=2},"收尾缺口");
        shutdownCase("inflight",new {normal_exit_observed=true,writer_error=(string)null,queued_tail_not_persisted=0,shutdown_senders_inflight=1,shutdown_queue_drained=false,shutdown_drain_timed_out=true},"排空超时");
        shutdownCase("flush-error",new {normal_exit_observed=true,writer_error="PermissionDenied",queued_tail_not_persisted=0,shutdown_senders_inflight=0,shutdown_queue_drained=true},"写入失败");
        shutdownCase("abnormal",new {normal_exit_observed=false,writer_error=(string)null,queued_tail_not_persisted=0,shutdown_senders_inflight=0,shutdown_queue_drained=true},"原因未确认");
        var frameText=new StringBuilder("mono_us,native_present,interval_us,phase,native_state,native_hud\n");
        for(int i=0;i<4000;i++)frameText.AppendLine((i*10000L)+","+i+",10000,4,0,0");File.WriteAllText(Path.Combine(bad,"frames-000001.csv"),frameText.ToString());
        File.WriteAllText(Path.Combine(bad,"diagnostic-map.json"),"{\"schema\":1,\"scope\":\"test-public-map\"}");
        File.WriteAllText(Path.Combine(bad,"touchHLE_log.txt"),"C:/Users/PRIVATE-PATH/secret must be excluded");File.WriteAllText(Path.Combine(bad,"screenshot.png"),"not auto included");
        Directory.CreateDirectory(Path.Combine(bad,"touchHLE_sandbox"));File.WriteAllText(Path.Combine(bad,"touchHLE_sandbox","save.sb"),"not auto included");
        Call(t,"InspectPrevious",root);Check(File.Exists(Path.Combine(bad,"previous-not-normal.json")),"Interrupted session not retained/identified");
        Check(!File.Exists(Path.Combine(good,"previous-not-normal.json")),"Normal session misidentified");Check(File.ReadAllText(Path.Combine(bad,"previous-not-normal.json")).Contains("not proof of a crash"),"Missing end incorrectly proves crash");
        Call(t,"Mark",bad,"Explicit test note");Check(Directory.GetFiles(Path.Combine(bad,"requests"),"*.request.json").Length==1,"Explicit mark not queued");
        Check(!(bool)Call(t,"Allowed","requests/handled/../../secret.request.json"),"Traversal-shaped diagnostic path accepted");
        string zipPath=Path.Combine(root,"default-export.zip");Call(t,"Export",new string[]{bad,good},zipPath,null,null);
        using(var f=File.OpenRead(zipPath))using(var zip=new ZipArchive(f,ZipArchiveMode.Read))
        {
            Check(zip.GetEntry("session-interrupted/diagnostic-map.json")!=null,"Provenance map missing from export");
            Check(zip.GetEntry("session-interrupted/frames-000001.csv")!=null,"Frame journal missing");
            Check(zip.GetEntry("session-interrupted/recovered-last30s.json")!=null,"Unexpected termination persisted tail not reconstructed");
            foreach(var entry in zip.Entries)Check(!entry.FullName.Contains("touchHLE_log")&&!entry.FullName.Contains("screenshot")&&!entry.FullName.Contains("touchHLE_sandbox")&&!entry.FullName.StartsWith("attachments/"),"Unrequested private attachment included");
        }
        byte[] before=File.ReadAllBytes(zipPath);try{Call(t,"Export",new string[]{bad},zipPath,null,null);throw new Exception("Existing ZIP overwritten");}catch(IOException){}
        Check(Convert.ToBase64String(before)==Convert.ToBase64String(File.ReadAllBytes(zipPath)),"Failed export modified old ZIP");
        Check(File.Exists(Path.Combine(bad,"frames-000001.csv")),"Export removed original records");
        string explicitZip=Path.Combine(root,"explicit-export.zip"),image=Path.Combine(root,"explicit.png"),saves=Path.Combine(root,"explicit-saves.zip");File.WriteAllText(image,"chosen image");File.WriteAllText(saves,"chosen saves");
        Call(t,"Export",new string[]{bad},explicitZip,saves,image);
        using(var f=File.OpenRead(explicitZip))using(var zip=new ZipArchive(f,ZipArchiveMode.Read)){Check(zip.GetEntry("attachments/user-selected-image.png")!=null&&zip.GetEntry("attachments/three-slot-saves.zip")!=null,"Explicitly chosen attachments missing");}
        string sameA=Path.Combine(root,"source-A","session-same"),sameB=Path.Combine(root,"source-B","session-same");
        foreach(string path in new[]{sameA,sameB}){Directory.CreateDirectory(path);Call(t,"WriteNew",Path.Combine(path,"host.json"),new {schema=1,kind="deadspace-play-diagnostics",source_id=path==sameA?"first":"second"});Call(t,"WriteNew",Path.Combine(path,"session-end.json"),new {normal_exit_observed=true});}
        string sameZip=Path.Combine(root,"same-name-export.zip");Call(t,"Export",new string[]{sameA,sameB},sameZip,null,null);
        using(var f=File.OpenRead(sameZip))using(var zip=new ZipArchive(f,ZipArchiveMode.Read)){
            var names=new HashSet<string>(StringComparer.OrdinalIgnoreCase);foreach(var entry in zip.Entries)Check(names.Add(entry.FullName),"Duplicate ZIP entry");
            Check(zip.GetEntry("session-same/host.json")!=null&&zip.GetEntry("session-same-2/host.json")!=null,"Same-name sessions collapsed");
            using(var reader=new StreamReader(zip.GetEntry("session-same/host.json").Open()))Check(reader.ReadToEnd().Contains("first"),"First same-name session changed");
            using(var reader=new StreamReader(zip.GetEntry("session-same-2/host.json").Open()))Check(reader.ReadToEnd().Contains("second"),"Second same-name session changed");
            using(var reader=new StreamReader(zip.GetEntry("export-manifest.json").Open())){string text=reader.ReadToEnd();Check(text.Contains("archive_folder")&&text.Contains("session-same-2")&&!text.Contains(root),"Missing alias map or private source path leaked");}
        }
        Console.WriteLine("PASS interrupted/normal end distinction / shutdown drained+tail2+inflight-timeout+flush-error+legacy status / originals retained / explicit mark / selected sessions ZIP / same-name sessions preserved uniquely / no automatic personal logs+image+saves / explicit attachments / existing ZIP refusal / traversal-shaped path refusal");
    }
}
