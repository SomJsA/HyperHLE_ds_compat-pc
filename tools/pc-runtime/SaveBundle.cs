using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Security.Cryptography;
using System.Text;

namespace DeadSpaceBootstrap
{
    // Shared Documents/ ZIP layout with Android: all three slots and common state.
    internal static class SaveBundle
    {
        private const long Limit = 256L * 1024 * 1024;
        private static string Documents(string root) { return Path.Combine(SaveStorage.ProfilePath(root), "Documents"); }
        private static string Hash(byte[] bytes) { using (var h = SHA256.Create()) return Convert.ToBase64String(h.ComputeHash(bytes)); }
        private static uint U32(byte[] bytes, int at) { return BitConverter.ToUInt32(bytes, at); }
        internal static void Validate(byte[] bytes)
        {
            if (bytes.Length < 8 || bytes.Length > 16 * 1024 * 1024 || Encoding.ASCII.GetString(bytes, 0, 4) != "SBIN" || U32(bytes, 4) != 2)
                throw new InvalidDataException("不是有效的 SBIN v2 存档");
            var tags = new HashSet<string>();
            for (int at = 8; at < bytes.Length; )
            {
                if (bytes.Length - at < 12) throw new InvalidDataException("存档段头不完整");
                string tag = Encoding.ASCII.GetString(bytes, at, 4); uint length = U32(bytes, at + 4);
                if (!tags.Add(tag) || length > bytes.Length - at - 12) throw new InvalidDataException("存档段长度或名称异常");
                uint hash = 0x811c9dc5; for (int i = at + 12; i < at + 12 + length; i++) hash = unchecked(hash * 0x01000193) ^ bytes[i];
                if (hash != U32(bytes, at + 8)) throw new InvalidDataException("存档校验失败");
                at = (at + 12 + (int)length + 3) & ~3;
                if (at > bytes.Length) throw new InvalidDataException("存档段填充不完整");
            }
            foreach (string tag in new[] { "DATA", "OHDR", "STRS" }) if (!tags.Contains(tag)) throw new InvalidDataException("存档缺少 " + tag);
        }
        private static string Normalize(string raw)
        {
            string path = raw.Replace('\\', '/');
            while (path.StartsWith("./", StringComparison.Ordinal)) path = path.Substring(2);
            if (path.StartsWith("/", StringComparison.Ordinal) || path.IndexOf(':') >= 0) throw new InvalidDataException("ZIP 包含绝对路径");
            foreach (string part in path.Split('/')) if (part == ".." || part == "." || part.Length == 0) throw new InvalidDataException("ZIP 包含不安全路径");
            foreach (string prefix in new[] { "touchHLE_sandbox/com.ea.deadspace.bv/Documents/", "com.ea.deadspace.bv/Documents/", "Documents/", "save/Documents/" })
                if (path.StartsWith(prefix, StringComparison.OrdinalIgnoreCase)) { path = path.Substring(prefix.Length); break; }
            bool dependency = path == "loggedIn" || path == "mtxprods" || path == "adtrackingevents" || path == ".dat1fda.006";
            bool version = path == "last_version.txt" || path == "var/last_version.txt";
            if (!path.EndsWith(".sb", StringComparison.OrdinalIgnoreCase) && !dependency && !version) return null;
            if (!dependency && path.IndexOf('/') < 0) path = "var/" + path;
            foreach (string part in path.Split('/')) if (part.StartsWith(".save-editor-", StringComparison.OrdinalIgnoreCase)) return null;
            return path;
        }
        private static Dictionary<string, byte[]> ReadDocuments(string root)
        {
            SaveStorage.RequireNoLinks(root); if (!Directory.Exists(root)) throw new IOException("当前没有可导出的存档");
            var files = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase); long total = 0;
            var pending = new Stack<string>(); pending.Push(root);
            while (pending.Count != 0)
            {
                string folder = pending.Pop(); SaveStorage.RequireNoLinks(folder);
                foreach (string path in Directory.GetFileSystemEntries(folder))
                {
                    SaveStorage.RequireNoLinks(path);
                    if (Directory.Exists(path)) { if (!Path.GetFileName(path).StartsWith(".save-editor-", StringComparison.OrdinalIgnoreCase)) pending.Push(path); continue; }
                    string relative = path.Substring(root.Length).TrimStart('\\', '/').Replace('\\', '/');
                    if (Normalize("Documents/" + relative) == null) continue;
                    total += new FileInfo(path).Length; if (total > Limit || files.Count >= 10000) throw new IOException("存档包超过 256 MB 或文件数上限");
                    byte[] bytes = File.ReadAllBytes(path); if (relative.EndsWith(".sb", StringComparison.OrdinalIgnoreCase)) Validate(bytes);
                    files.Add(relative, bytes);
                }
            }
            return files;
        }
        internal static void Export(string dataRoot, string output)
        {
            SaveStorage.RequireStopped(); output = Path.GetFullPath(output); SaveStorage.RequireNoLinks(output);
            var files = ReadDocuments(Documents(dataRoot)); if (files.Count == 0) throw new IOException("当前没有可导出的存档");
            using (var stream = new FileStream(output, FileMode.CreateNew, FileAccess.Write, FileShare.None))
            using (var zip = new ZipArchive(stream, ZipArchiveMode.Create))
                foreach (var file in files)
                {
                    var entry = zip.CreateEntry("Documents/" + file.Key, CompressionLevel.Optimal);
                    using (var destination = entry.Open()) destination.Write(file.Value, 0, file.Value.Length);
                }
        }
        internal static string Import(string dataRoot, string input)
        {
            SaveStorage.RequireStopped(); input = Path.GetFullPath(input); SaveStorage.RequireNoLinks(input);
            if (input.StartsWith(Path.GetFullPath(Documents(dataRoot)) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
                throw new IOException("请先把导入 ZIP 放到当前 Documents 存档目录以外；原 ZIP 不会被移动");
            var files = new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase); long total = 0; int saves = 0;
            using (var stream = File.OpenRead(input))
            using (var zip = new ZipArchive(stream, ZipArchiveMode.Read))
            {
                if (zip.Entries.Count > 10000) throw new InvalidDataException("ZIP 文件数超过上限");
                foreach (var entry in zip.Entries)
                {
                    if (entry.FullName.EndsWith("/", StringComparison.Ordinal)) continue;
                    string relative = Normalize(entry.FullName); if (relative == null) continue;
                    if (files.ContainsKey(relative)) throw new InvalidDataException("ZIP 存档路径重复");
                    if (entry.Length > Limit - total) throw new InvalidDataException("ZIP 解压后超过 256 MB");
                    using (var source = entry.Open()) using (var memory = new MemoryStream())
                    {
                        byte[] buffer = new byte[32768]; int count;
                        while ((count = source.Read(buffer, 0, buffer.Length)) != 0)
                        { total += count; if (total > Limit) throw new InvalidDataException("ZIP 解压后超过上限"); memory.Write(buffer, 0, count); }
                        byte[] bytes = memory.ToArray(); if (relative.EndsWith(".sb", StringComparison.OrdinalIgnoreCase)) { Validate(bytes); saves++; }
                        files.Add(relative, bytes);
                    }
                }
            }
            if (saves == 0) throw new InvalidDataException("ZIP 内没有 .sb 存档");
            string profile = SaveStorage.ProfilePath(dataRoot), docs = Documents(dataRoot); SaveStorage.RequireNoLinks(profile);
            Directory.CreateDirectory(profile);
            var before = Directory.Exists(docs) ? ReadDocuments(docs) : new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
            string token = DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N");
            string staging = Path.Combine(profile, "Documents.import-" + token), previous = Path.Combine(profile, "Documents.before-import-" + token);
            SaveStorage.RequireNoLinks(staging); Directory.CreateDirectory(staging);
            foreach (var file in files)
            {
                string target = Path.GetFullPath(Path.Combine(staging, file.Key.Replace('/', Path.DirectorySeparatorChar)));
                if (!target.StartsWith(Path.GetFullPath(staging) + Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase)) throw new IOException("恢复路径超出存档目录");
                Directory.CreateDirectory(Path.GetDirectoryName(target));
                using (var fileStream = new FileStream(target, FileMode.CreateNew, FileAccess.Write, FileShare.None)) fileStream.Write(file.Value, 0, file.Value.Length);
                if (Hash(File.ReadAllBytes(target)) != Hash(file.Value)) throw new IOException("恢复暂存校验失败");
            }
            string backup = null;
            if (before.Count != 0)
            {
                string backupRoot = Path.Combine(dataRoot, "save-backups"); SaveStorage.RequireNoLinks(backupRoot); Directory.CreateDirectory(backupRoot);
                backup = Path.Combine(backupRoot, "Three-slots-before-import-" + token + ".zip"); Export(dataRoot, backup);
            }
            SaveStorage.RequireStopped(); SaveStorage.RequireNoLinks(docs);
            var current = Directory.Exists(docs) ? ReadDocuments(docs) : new Dictionary<string, byte[]>(StringComparer.OrdinalIgnoreCase);
            if (current.Count != before.Count) throw new IOException("导入期间当前存档发生变化，未覆盖");
            foreach (var file in before) if (!current.ContainsKey(file.Key) || Hash(current[file.Key]) != Hash(file.Value)) throw new IOException("导入期间当前存档发生变化，未覆盖");
            // Both moves stay under the selected profile; previous is retained.
            SaveStorage.RequireNoLinks(staging); SaveStorage.RequireNoLinks(previous);
            if (!String.Equals(Path.GetDirectoryName(Path.GetFullPath(docs)), Path.GetFullPath(profile), StringComparison.OrdinalIgnoreCase)
                || !String.Equals(Path.GetDirectoryName(Path.GetFullPath(staging)), Path.GetFullPath(profile), StringComparison.OrdinalIgnoreCase)
                || !String.Equals(Path.GetDirectoryName(Path.GetFullPath(previous)), Path.GetFullPath(profile), StringComparison.OrdinalIgnoreCase)) throw new IOException("存档替换路径超出所选目录");
            if (Directory.Exists(docs)) Directory.Move(docs, previous);
            try { Directory.Move(staging, docs); }
            catch { if (Directory.Exists(previous) && !Directory.Exists(docs)) Directory.Move(previous, docs); throw; }
            return backup;
        }
    }
}
