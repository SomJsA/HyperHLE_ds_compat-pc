using System;
using System.Collections.Generic;
using System.IO;
using System.Security.Cryptography;
using System.Text;

namespace DeadSpaceBootstrap
{
    internal static class SaveBackup
    {
        private static string Hash(byte[] bytes) { using (var hash = SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-", ""); }
        private static bool IsSaveName(string name)
        { return name == Path.GetFileName(name) && name.IndexOfAny(new[] { '/', '\\', ':' }) < 0 && (name.EndsWith(".sb", StringComparison.OrdinalIgnoreCase) || name == "last_version.txt"); }

        internal static string Restore(string manifest, string destination)
        {
            SaveStorage.RequireStopped();
            manifest = Path.GetFullPath(manifest); destination = Path.GetFullPath(destination);
            SaveStorage.RequireNoLinks(manifest); SaveStorage.RequireNoLinks(destination);
            if (Path.GetFileName(manifest) != "original.sha256") throw new InvalidDataException("请选择编辑器生成的 original.sha256");
            string source = Path.GetDirectoryName(manifest);
            var requested = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
            foreach (string line in File.ReadAllLines(manifest, Encoding.UTF8))
            {
                if (line.Length == 0 || line.StartsWith("#")) continue;
                if (line.Length < 67 || line.Substring(64, 2) != "  ") throw new InvalidDataException("备份清单格式异常");
                string name = line.Substring(66);
                if (!IsSaveName(name) || requested.ContainsKey(name)) throw new InvalidDataException("备份文件名异常或重复");
                string path = Path.Combine(source, name); SaveStorage.RequireNoLinks(path);
                byte[] bytes = File.ReadAllBytes(path);
                if (!String.Equals(Hash(bytes), line.Substring(0, 64), StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("备份校验失败：" + name);
                requested.Add(name, bytes);
            }
            if (requested.Count == 0) throw new InvalidDataException("备份为空");
            Directory.CreateDirectory(destination);
            foreach (string name in requested.Keys)
            {
                string target = Path.Combine(destination, name); SaveStorage.RequireNoLinks(target);
                if (Directory.Exists(target)) throw new IOException("恢复目标是目录：" + target);
            }
            string token = "restore-" + DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N");
            string backup = Path.Combine(destination, ".save-editor-backups", token);
            SaveStorage.RequireNoLinks(backup); Directory.CreateDirectory(backup);
            var before = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
            var lines = new List<string> { "# Full save set before restoration" };
            foreach (string file in Directory.GetFiles(destination))
            {
                string name = Path.GetFileName(file); if (!IsSaveName(name)) continue;
                SaveStorage.RequireNoLinks(file); byte[] bytes = File.ReadAllBytes(file); before.Add(name, bytes);
                string copy = Path.Combine(backup, name); File.WriteAllBytes(copy, bytes);
                if (Hash(File.ReadAllBytes(copy)) != Hash(bytes)) throw new IOException("恢复前备份失败");
                lines.Add(Hash(bytes) + "  " + name);
            }
            File.WriteAllLines(Path.Combine(backup, "original.sha256"), lines.ToArray(), new UTF8Encoding(false));
            var staged = new Dictionary<string, string>(); var published = new List<string>();
            try
            {
                foreach (var file in requested)
                {
                    string temp = Path.Combine(destination, ".restore-" + token + "-" + file.Key + ".tmp");
                    using (var stream = new FileStream(temp, FileMode.CreateNew, FileAccess.Write, FileShare.None)) stream.Write(file.Value, 0, file.Value.Length);
                    staged.Add(file.Key, temp);
                    if (Hash(File.ReadAllBytes(temp)) != Hash(file.Value)) throw new IOException("恢复暂存校验失败");
                }
                SaveStorage.RequireStopped();
                foreach (var file in requested)
                {
                    string target = Path.Combine(destination, file.Key); SaveStorage.RequireNoLinks(target);
                    byte[] previous;
                    bool existed = before.TryGetValue(file.Key, out previous);
                    if (existed ? !File.Exists(target) || Hash(File.ReadAllBytes(target)) != Hash(previous) : File.Exists(target))
                        throw new IOException("恢复期间目标变化：" + file.Key);
                    if (existed) File.Replace(staged[file.Key], target, null); else File.Move(staged[file.Key], target);
                    published.Add(file.Key);
                    if (Hash(File.ReadAllBytes(target)) != Hash(file.Value)) throw new IOException("恢复后校验失败");
                }
                File.WriteAllText(Path.Combine(backup, "restore-result.txt"), "Committed verified files from " + manifest + "\r\nUnlisted destination files retained.");
                return backup;
            }
            catch
            {
                // Roll back every published member. The complete recovery set
                // remains on disk even if the filesystem refuses rollback.
                foreach (string name in published)
                {
                    string target = Path.Combine(destination, name), temp = Path.Combine(destination, ".rollback-" + token + "-" + name + ".tmp");
                    byte[] previous;
                    if (before.TryGetValue(name, out previous))
                    {
                        File.WriteAllBytes(temp, previous); File.Replace(temp, target, null);
                    }
                    else File.Move(target, Path.Combine(backup, "new-during-failed-restore-" + name));
                }
                throw;
            }
            finally { foreach (string path in staged.Values) if (File.Exists(path)) File.Delete(path); }
        }
    }
}
