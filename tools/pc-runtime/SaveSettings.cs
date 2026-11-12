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
        private readonly Dictionary<string, string> fieldPaths = new Dictionary<string, string>(StringComparer.Ordinal);
        private static string Field(string name) { return "dlc_modules/" + name + "/unlocked"; }
        internal string FilePath { get { return path; } }
        internal SaveSettings(string file)
        {
            path = Path.GetFullPath(file);
            original = File.ReadAllBytes(path);
            document = new SaveDocument(path, original);
            foreach (string name in new[] { "attack", "defense", "loot" })
            {
                SaveValue value;
                if (document.Values.TryGetValue(Field(name), out value) && value.Type == 9
                    && value.Editable && (value.Value == "0" || value.Value == "1")) fieldPaths[name] = Field(name);
            }
        }
        private static void Require(bool ok, string message) { if (!ok) throw new InvalidDataException(message); }
        internal bool Available(string name) { return fieldPaths.ContainsKey(name); }
        internal bool NeedsInitialization { get { return fieldPaths.Count != 3; } }
        internal string MissingFields()
        {
            var missing = new List<string>();
            foreach (string name in new[] { "attack", "defense", "loot" }) if (!Available(name)) missing.Add(name);
            return String.Join(", ", missing.ToArray());
        }
        internal string AvailablePluginFields()
        {
            var fields = new List<string>();
            foreach (string pair in document.Values.Keys)
                if (pair.StartsWith("dlc_modules/", StringComparison.Ordinal) && pair.EndsWith("/unlocked", StringComparison.Ordinal)) fields.Add(pair);
            return fields.Count == 0 ? "无" : String.Join(", ", fields.ToArray());
        }
        internal bool Enabled(string name) { return Available(name) && document.Values[fieldPaths[name]].Value == "1"; }
        internal byte[] EditBytes(bool attack, bool defense, bool loot)
        {
            if (fieldPaths.Count != 3)
            {
                byte[] initialized = document.EnsureDlcModules(attack, defense, loot);
                var ready = new SaveDocument(path, initialized);
                return ready.EditBytes(new Dictionary<string, string> {
                    { "dlc_modules/attack/unlocked", attack ? "1" : "0" },
                    { "dlc_modules/defense/unlocked", defense ? "1" : "0" },
                    { "dlc_modules/loot/unlocked", loot ? "1" : "0" }
                });
            }
            return document.EditBytes(new Dictionary<string, string> {
                { fieldPaths["attack"], attack ? "1" : "0" },
                { fieldPaths["defense"], defense ? "1" : "0" },
                { fieldPaths["loot"], loot ? "1" : "0" }
            });
        }

        internal void RequireUnchanged()
        {
            SaveStorage.RequireNoLinks(path);
            Require(Convert.ToBase64String(File.ReadAllBytes(path)) == Convert.ToBase64String(original), "存档已改变，请重新读取");
        }
        internal string Write(bool attack, bool defense, bool loot)
        {
            SaveStorage.RequireStopped(); RequireUnchanged();
            byte[] edited = EditBytes(attack, defense, loot);
            var check = new SaveDocument(path, edited);
            Require(check.Values[Field("attack")].Value == (attack ? "1" : "0") &&
                check.Values[Field("defense")].Value == (defense ? "1" : "0") &&
                check.Values[Field("loot")].Value == (loot ? "1" : "0"), "写入验证失败");
            return document.WritePrepared(edited, "attack=" + attack + "; defense=" + defense + "; loot=" + loot);
        }
    }
}
