using System;
using System.Collections.Generic;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Web.Script.Serialization;
class CollectorMapTest
{
    static void Check(bool condition,string message){if(!condition)throw new Exception(message);}
    static void Main(string[] args)
    {
        var a=Assembly.LoadFrom(Path.GetFullPath(args[0]));var type=a.GetType("DeadSpaceBootstrap.LauncherForm",true);
        var method=type.GetMethod("CopyDiagnosticMap",BindingFlags.Static|BindingFlags.NonPublic);
        string root=Path.GetFullPath(args[1]);Check(!Directory.Exists(root),"Output must be new");Directory.CreateDirectory(root);
        string package=Path.Combine(root,"package"),session=Path.Combine(root,"session");Directory.CreateDirectory(package);Directory.CreateDirectory(session);
        string map=Path.Combine(package,"diagnostic-map.json");File.WriteAllText(map,"{\"schema\":1,\"public_source_map\":true}");
        string hash;using(var sha=SHA256.Create())hash=BitConverter.ToString(sha.ComputeHash(File.ReadAllBytes(map))).Replace("-","").ToLowerInvariant();
        var serializer=new JavaScriptSerializer();var manifest=new Dictionary<string,object>();
        Func<Dictionary<string,object>> run=()=>serializer.Deserialize<Dictionary<string,object>>(serializer.Serialize(method.Invoke(null,new object[]{package,session,manifest})));
        Check(!(bool)run()["copied"]&&!File.Exists(Path.Combine(session,"diagnostic-map.json")),"Unbound map copied");
        manifest["diagnostic_map_sha256"]=new String('0',64);Check(!(bool)run()["copied"],"Mismatched map copied");
        manifest["diagnostic_map_sha256"]=hash;Check((bool)run()["copied"],"Bound map not copied");
        Check(File.ReadAllText(map)==File.ReadAllText(Path.Combine(session,"diagnostic-map.json")),"Map bytes altered");
        File.WriteAllText(Path.Combine(session,"diagnostic-map.json"),"existing unknown sentinel");Check(!(bool)run()["copied"],"Existing map overwritten");
        Check(File.ReadAllText(Path.Combine(session,"diagnostic-map.json"))=="existing unknown sentinel","Unknown existing map changed");
        Console.WriteLine("PASS bound SHA exact map copy / missing binding refusal / mismatch refusal / existing map preserved");
    }
}
