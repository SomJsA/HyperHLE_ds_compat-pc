using System;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal static class DocumentsSettings
    {
        private static readonly string[] ConfigurationNames={"DeadSpace-Launcher.ini","DeadSpace-Keys.ini","DeadSpace-Gamepad.ini","DeadSpace-VisualCompat.ini","overlay-style.json"};
        internal static string Resolve(string packageRoot,string documents)
        {bool firstConfiguration;return Resolve(packageRoot,documents,out firstConfiguration);}
        internal static string Resolve(string packageRoot,string documents,out bool firstConfiguration)
        {
            firstConfiguration=false;
            // An existing root INI is the legacy explicit configuration. Never
            // silently migrate it or switch the user's old save location.
            string legacy=Path.Combine(packageRoot,"DeadSpace-Launcher.ini");
            SaveStorage.RequireNoLinks(legacy);
            if(File.Exists(legacy))return legacy;
            string root=SaveStorage.Root(packageRoot,documents,true);
            string previous=Path.Combine(documents,"DeadSpace IOS移植");SaveStorage.RequireNoLinks(previous);
            if(!Directory.Exists(root) && !File.Exists(root) && Directory.Exists(previous))SaveStorage.CopyDataRoot(previous,root);
            EnsureLayout(root);
            string config=Path.Combine(root,"config","pc");SaveStorage.RequireNoLinks(config);Directory.CreateDirectory(config);
            firstConfiguration=!File.Exists(Path.Combine(config,"DeadSpace-Launcher.ini"));
            foreach(string name in ConfigurationNames)
            {
                string target=Path.Combine(config,name);SaveStorage.RequireNoLinks(target);if(File.Exists(target))continue;
                string source=Path.Combine(packageRoot,"Defaults",name);
                if(!File.Exists(source))source=Path.Combine(packageRoot,name);
                if(!File.Exists(source))continue;
                SaveStorage.RequireNoLinks(source);SaveStorage.RequireNoLinks(target);
                try{File.Copy(source,target,false);File.SetAttributes(target,File.GetAttributes(target)&~FileAttributes.ReadOnly);}
                catch(IOException){if(!File.Exists(target))throw;}
            }
            return Path.Combine(config,"DeadSpace-Launcher.ini");
        }
        // Choosing/viewing a location must not create it. Every native save
        // action still rejects a pre-existing newer layout before any writes.
        internal static void RequireCompatibleLayout(string root)
        {
            SaveStorage.RequireNoLinks(root);
            string marker=Path.Combine(root,"storage-layout.json");SaveStorage.RequireNoLinks(marker);
            if(File.Exists(marker))EnsureLayout(root);
        }
        internal static void EnsureLayout(string root)
        {
            SaveStorage.RequireNoLinks(root);Directory.CreateDirectory(root);
            string path=Path.Combine(root,"storage-layout.json");SaveStorage.RequireNoLinks(path);
            if(File.Exists(path))
            {
                var info=new FileInfo(path);if(info.Length>65536)throw new InvalidDataException("资料布局标记异常，原文件已保留。");
                Dictionary<string,object> map;
                try { map=new JavaScriptSerializer{MaxJsonLength=65536,RecursionLimit=16}.DeserializeObject(File.ReadAllText(path,Encoding.UTF8)) as Dictionary<string,object>; }
                catch(ArgumentException error){throw new InvalidDataException("资料布局标记损坏，原文件已保留。",error);}
                catch(InvalidOperationException error){throw new InvalidDataException("资料布局标记损坏，原文件已保留。",error);}
                if(map==null || StartupReviewData.Number(map,"schema",0)!=1 || StartupReviewData.Text(map,"game")!=SaveStorage.Profile
                    || StartupReviewData.Text(map,"exchange")!="three-slot-zip-v1"
                    || StartupReviewData.Text(map,"native_profile")!="touchHLE_sandbox/"+SaveStorage.Profile
                    || StartupReviewData.Text(map,"pc_config")!="config/pc" || StartupReviewData.Text(map,"android_config")!="config/android")
                    throw new InvalidDataException("文档目录的资料版本尚不兼容；为保护进度，本版本不写入该目录。");
                return;
            }
            var layout=new Dictionary<string,object>{{"schema",1},{"game",SaveStorage.Profile},{"exchange","three-slot-zip-v1"},
                {"native_profile","touchHLE_sandbox/"+SaveStorage.Profile},{"pc_config","config/pc"},{"android_config","config/android"}};
            try{using(var stream=new FileStream(path,FileMode.CreateNew,FileAccess.Write,FileShare.None))
                {byte[] data=Encoding.UTF8.GetBytes(new JavaScriptSerializer().Serialize(layout));stream.Write(data,0,data.Length);stream.Flush(true);}}
            catch(IOException){if(!File.Exists(path))throw;EnsureLayout(root);}
            string readme=Path.Combine(root,"README-Storage.txt");SaveStorage.RequireNoLinks(readme);
            if(!File.Exists(readme))
            {
                string text="DeadSpace Legacy 资料布局 v1\r\n\r\n游戏原生进度：touchHLE_sandbox/"+SaveStorage.Profile+"\r\nPC启动器、按键及显示配置：config/pc\r\nAndroid平台配置：config/android\r\n\r\n三槽位ZIP是PC与Android正式交换格式，包含已有三个槽位与公共依赖；导入前自动备份。\r\n游戏配置和原生SBIN不做跨版本猜测转换；未知布局版本拒绝写入。平台启动器配置分别保存，不直接互换。\r\n切换位置不会自动合并进度。升级分发包不会覆盖本文档目录；迁移请使用启动器的复制或ZIP导入导出。\r\n请避免两台设备同时改写同一同步目录；先完成导出、备份，再在另一设备导入。\r\n";
                try{using(var stream=new FileStream(readme,FileMode.CreateNew,FileAccess.Write,FileShare.None))
                    {byte[] data=Encoding.UTF8.GetBytes(text);stream.Write(data,0,data.Length);}}
                catch(IOException){if(!File.Exists(readme))throw;}
            }
        }
        internal static void Save(string path,string[] lines)
        {
            SaveStorage.RequireNoLinks(path);Directory.CreateDirectory(Path.GetDirectoryName(path));
            string text=String.Join(Environment.NewLine,lines)+Environment.NewLine;
            if(File.Exists(path)&&File.ReadAllText(path,Encoding.UTF8)==text)return;
            string temporary=path+"."+Guid.NewGuid().ToString("N")+".tmp";
            try
            {
                File.WriteAllText(temporary,text,new UTF8Encoding(false));
                if(File.Exists(path))File.Replace(temporary,path,path+".backup-"+Guid.NewGuid().ToString("N"));
                else File.Move(temporary,path);
            }
            finally{if(File.Exists(temporary))File.Delete(temporary);}
        }
    }
    internal sealed partial class LauncherForm
    {
        private string ConfigurationDirectory{get{return Path.GetDirectoryName(iniPath);}}
        private string ConfigurationFile(string name){return Path.Combine(ConfigurationDirectory,name);}
        private string SaveIpaReference()
        {
            try
            {
                string selected=DistributionLayout.ResolveIpa(baseDir,ipaPath.Text);
                if(String.Equals(selected,Path.Combine(DistributionLayout.Game(baseDir),"DeadSpace.ipa"),StringComparison.OrdinalIgnoreCase))return DistributionLayout.Game(baseDir)==baseDir?"DeadSpace.ipa":Path.Combine("Game","DeadSpace.ipa");
            }catch(ArgumentException){}
            return ipaPath.Text;
        }
    }
}
