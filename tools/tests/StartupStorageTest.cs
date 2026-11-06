using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Text;
class StartupStorageTest
{
    static void Check(bool value,string message){if(!value)throw new Exception(message);}
    static object Call(Type type,string name,params object[] args)
    {try{return Array.Find(type.GetMethods(BindingFlags.Static|BindingFlags.NonPublic),m=>m.Name==name && m.GetParameters().Length==args.Length).Invoke(null,args);}catch(TargetInvocationException error){throw error.InnerException;}}
    static void Refuses(Type type,string root,string label)
    {try{Call(type,"EnsureLayout",root);throw new Exception(label+" accepted");}catch(InvalidDataException){}}
    static void Main(string[] args)
    {
        var assembly=Assembly.LoadFrom(Path.GetFullPath(args[0]));
        var storage=assembly.GetType("DeadSpaceBootstrap.DocumentsSettings",true);
        var review=assembly.GetType("DeadSpaceBootstrap.StartupReviewData",true);
        string output=Path.GetFullPath(args[1]);Check(!Directory.Exists(output),"Test output must be new");Directory.CreateDirectory(output);
        string package=Path.Combine(output,"package"),docs=Path.Combine(output,"Documents");Directory.CreateDirectory(Path.Combine(package,"Defaults"));
        File.WriteAllText(Path.Combine(package,"Defaults","DeadSpace-Launcher.ini"),"save_mode=文档（当前用户）\nipa=DeadSpace.ipa\n",Encoding.UTF8);
        string config=(string)Call(storage,"Resolve",package,docs);
        string root=Path.Combine(docs,"DeadSpaceLegacy");
        Check(config==Path.Combine(root,"config","pc","DeadSpace-Launcher.ini"),"Fresh package must use the stable Documents leaf");
        Check(File.Exists(Path.Combine(root,"storage-layout.json"))&&File.Exists(Path.Combine(root,"README-Storage.txt")),"Version manifest and storage instructions missing");
        string custom="save_mode=便携（启动器目录）\ncustom=preserve\n";File.WriteAllText(config,custom,Encoding.UTF8);
        string native=Path.Combine(root,"touchHLE_sandbox","com.ea.deadspace.bv","Documents");Directory.CreateDirectory(native);
        byte[] saved={1,0,255,17};File.WriteAllBytes(Path.Combine(native,"untouched.sbin"),saved);
        Call(storage,"Resolve",package,docs);Check(File.ReadAllText(config,Encoding.UTF8)==custom,"Seed overwrote existing settings");
        Check(Convert.ToBase64String(File.ReadAllBytes(Path.Combine(native,"untouched.sbin")))==Convert.ToBase64String(saved),"Resolve touched native progress");
        Call(storage,"Save",config,new string[]{"selected=changed"});
        var backups=Directory.GetFiles(Path.GetDirectoryName(config),"DeadSpace-Launcher.ini.backup-*");
        Check(backups.Length==1&&File.ReadAllText(backups[0],Encoding.UTF8)==custom,"Atomic config replacement must preserve the previous file");
        Call(storage,"Save",config,new string[]{"selected=changed"});Check(Directory.GetFiles(Path.GetDirectoryName(config),"*.backup-*").Length==1,"Unchanged configuration should not create backup churn");
        string legacy=Path.Combine(package,"DeadSpace-Launcher.ini");File.WriteAllText(legacy,custom,Encoding.UTF8);
        Check((string)Call(storage,"Resolve",package,Path.Combine(output,"unused-docs"))==legacy,"Legacy explicit package configuration moved");
        Check(!Directory.Exists(Path.Combine(output,"unused-docs")),"Legacy load unexpectedly wrote Documents");
        foreach(string value in new[]{"{\"schema\":999,\"game\":\"com.ea.deadspace.bv\"}","bad-json"})
        {
            string bad=Path.Combine(output,Guid.NewGuid().ToString("N"));Directory.CreateDirectory(bad);
            string marker=Path.Combine(bad,"storage-layout.json");File.WriteAllText(marker,value,Encoding.UTF8);
            Refuses(storage,bad,"Unknown/corrupt layout");Check(File.ReadAllText(marker,Encoding.UTF8)==value&&Directory.GetFiles(bad).Length==1,"Rejected marker was mutated");
        }
        Check(Call(review,"Known","AMD Ryzen 9 9950X 16-Core Processor","NVIDIA GeForce RTX 4090 D",24.0)!=null,"Exact evidence profile missing");
        Check(Call(review,"Known","Unknown CPU","NVIDIA GeForce RTX 4090 D",24.0)==null,"GPU-only heuristic accepted");
        var pending=new Dictionary<string,object>{{"schema",1},{"pid",1234}};
        Check(!(bool)Call(review,"Current",pending,1235),"Stale native report accepted");
        Check(((string)Call(review,"Features",pending)).Contains("待"),"Pending native features shown as enabled");
        var fallback=new Dictionary<string,object>{{"af_requested",4},{"af_actual",1},{"af_verified",false},{"msaa_requested",4},{"msaa_actual",1}};
        string text=(string)Call(review,"Features",fallback);Check(text.Contains("降低")&&text.Contains("减弱"),"Fallback impact missing");
        Console.WriteLine("PASS Documents default / stable leaf / legacy explicit settings / missing-only seed / native saves unchanged / atomic previous-config backup / no backup churn / unknown+corrupt layout refusal / exact hardware match / stale and pending report / fallback impact");
    }
}
