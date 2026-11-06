using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
namespace DeadSpaceBootstrap
{
    // The native game may relocate the root object. Use the checked OHDR
    // graph reader shared with the editor instead of guessing its offset.
    internal sealed class SaveSettings
    {
        private readonly byte[] original;
        private readonly string path;
        private readonly SaveDocument document;
        private static string Field(string name) { return "dlc_modules/" + name + "/unlocked"; }
        internal SaveSettings(string file)
        {
            path = Path.GetFullPath(file);
            original = File.ReadAllBytes(path);
            document = new SaveDocument(path, original);
            foreach (string name in new[] { "attack", "defense", "loot" })
            {
                SaveValue value;
                Require(document.Values.TryGetValue(Field(name), out value) && value.Type == 9
                    && value.Editable && (value.Value == "0" || value.Value == "1"), "存档缺少布尔插件字段：" + name);
            }
        }
        private static void Require(bool ok, string message) { if (!ok) throw new InvalidDataException(message); }
        internal bool Enabled(string name) { return document.Values[Field(name)].Value == "1"; }
        internal byte[] EditBytes(bool attack, bool defense, bool loot)
        {
            return document.EditBytes(new Dictionary<string, string> {
                { Field("attack"), attack ? "1" : "0" },
                { Field("defense"), defense ? "1" : "0" },
                { Field("loot"), loot ? "1" : "0" }
            });
        }

        internal string Write(bool attack, bool defense, bool loot)
        {
            Require(Process.GetProcessesByName("touchHLE").Length == 0, "请先退出游戏，再修改存档");
            byte[] now = File.ReadAllBytes(path);
            Require(Convert.ToBase64String(now) == Convert.ToBase64String(original), "存档已改变，请重新读取");
            byte[] edited = EditBytes(attack, defense, loot);
            string token = DateTime.UtcNow.ToString("yyyyMMdd-HHmmss-fffffff") + "-" + Guid.NewGuid().ToString("N");
            string temp = path + "." + token + ".tmp", backup = path + "." + token + ".bak";
            try
            {
                File.WriteAllBytes(temp, edited);
                var check = new SaveSettings(temp);
                Require(check.Enabled("attack") == attack && check.Enabled("defense") == defense && check.Enabled("loot") == loot, "写入验证失败");
                Require(Process.GetProcessesByName("touchHLE").Length == 0, "游戏已启动，取消修改");
                Require(Convert.ToBase64String(File.ReadAllBytes(path)) == Convert.ToBase64String(original), "存档已改变，取消修改");
                File.Replace(temp, path, backup);
                return backup;
            }
            finally { if (File.Exists(temp)) File.Delete(temp); }
        }
    }
}
