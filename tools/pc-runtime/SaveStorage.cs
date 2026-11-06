using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Security.Cryptography;

namespace DeadSpaceBootstrap
{
    // A migration copies one profile into a new directory. It never overwrites
    // a destination profile or removes the source, including on failure.
    internal static class SaveStorage
    {
        internal const string Profile = "com.ea.deadspace.bv";

        internal static string Root(string portable, string documents, bool documentMode)
        {
            string path = documentMode ? Path.Combine(documents, "DeadSpaceLegacy") : portable;
            if (String.IsNullOrWhiteSpace(documentMode ? documents : portable) || !Path.IsPathRooted(path))
                throw new InvalidDataException("存档根目录必须是有效绝对路径");
            return Path.GetFullPath(path);
        }

        internal static string ProfilePath(string root)
        { return Path.Combine(root, "touchHLE_sandbox", Profile); }

        internal static void RequireStopped()
        {
            var processes = Process.GetProcessesByName("touchHLE");
            try { if (processes.Length != 0) throw new IOException("请先退出全部游戏进程，再切换或迁移存档。"); }
            finally { foreach (var process in processes) process.Dispose(); }
        }

        internal static void EnsureWritable(string root)
        {
            RequireNoLinks(root);
            Directory.CreateDirectory(root);
            string probe = Path.Combine(root, ".write-test-" + Guid.NewGuid().ToString("N"));
            try { using (new FileStream(probe, FileMode.CreateNew, FileAccess.Write, FileShare.None)) {} }
            finally { if (File.Exists(probe)) File.Delete(probe); }
        }

        internal static void RequireNoLinks(string path)
        {
            for (string current = Path.GetFullPath(path); !String.IsNullOrEmpty(current); current = Path.GetDirectoryName(current))
            {
                if ((File.Exists(current) || Directory.Exists(current)) &&
                    (File.GetAttributes(current) & FileAttributes.ReparsePoint) != 0)
                    throw new IOException("拒绝符号链接/目录联接：" + current);
                string parent = Path.GetDirectoryName(current);
                if (parent == current) break;
            }
        }

        private static Dictionary<string, string> Manifest(string directory, bool skipDiagnostics = false)
        {
            RequireNoLinks(directory);
            var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
            var pending = new Stack<string>(); pending.Push(directory);
            while (pending.Count > 0)
            {
                string current = pending.Pop(); RequireNoLinks(current);
                foreach (string child in Directory.GetFileSystemEntries(current))
                {
                    if(skipDiagnostics && current==directory && Path.GetFileName(child).Equals("diagnostics",StringComparison.OrdinalIgnoreCase))continue;
                    RequireNoLinks(child);
                    if (Directory.Exists(child)) { pending.Push(child); continue; }
                    using (var stream = new FileStream(child, FileMode.Open, FileAccess.Read, FileShare.Read))
                    using (var hash = SHA256.Create())
                        result.Add(child.Substring(directory.Length + 1), BitConverter.ToString(hash.ComputeHash(stream)).Replace("-", ""));
                }
            }
            return result;
        }

        private static void Equal(Dictionary<string, string> expected, Dictionary<string, string> actual)
        {
            if (expected.Count != actual.Count) throw new IOException("迁移期间文件数量变化，已取消。");
            foreach (var file in expected)
            {
                string value;
                if (!actual.TryGetValue(file.Key, out value) || value != file.Value)
                    throw new IOException("迁移校验失败或源文件变化：" + file.Key);
            }
        }

        internal static int CopyProfile(string sourceRoot, string destinationRoot)
        {
            RequireStopped();
            string source = Path.GetFullPath(ProfilePath(sourceRoot)).TrimEnd(Path.DirectorySeparatorChar);
            string target = Path.GetFullPath(ProfilePath(destinationRoot)).TrimEnd(Path.DirectorySeparatorChar);
            var comparison = StringComparison.OrdinalIgnoreCase;
            if (source.Equals(target, comparison) || source.StartsWith(target + Path.DirectorySeparatorChar, comparison)
                || target.StartsWith(source + Path.DirectorySeparatorChar, comparison))
                throw new IOException("源和目标必须是两个独立的存档目录。");
            RequireNoLinks(source); RequireNoLinks(target);
            if (!Directory.Exists(source)) throw new IOException("源模式没有存档。");
            if (Directory.Exists(target) || File.Exists(target)) throw new IOException("目标已有存档；为保护进度，不覆盖或合并。");
            var before = Manifest(source);
            if (before.Count == 0) throw new IOException("源存档为空。");
            string parent = Path.GetDirectoryName(target);
            Directory.CreateDirectory(parent);
            string staging = target + ".migrating-" + Guid.NewGuid().ToString("N");
            Directory.CreateDirectory(staging);
            try
            {
                // Preserve empty directories as well as file contents.
                CopyTree(source, staging);
                Equal(before, Manifest(staging));
                Equal(before, Manifest(source));
                File.WriteAllLines(Path.Combine(parent, "migration-" + Guid.NewGuid().ToString("N") + ".sha256"),
                    ManifestLines(source, target, before));
                RequireStopped(); RequireNoLinks(target);
                Directory.Move(staging, target); // Atomic publish, fails if target appeared.
                return before.Count;
            }
            finally
            {
                // This path is a unique staging sibling created by this method.
                if (Directory.Exists(staging)) Directory.Delete(staging, true);
            }
        }

        internal static void CopyDataRoot(string source,string target)
        {
            RequireStopped();RequireNoLinks(source);RequireNoLinks(target);
            if(Directory.Exists(target)||File.Exists(target))throw new IOException("新资料目录已存在，不自动合并。");
            DocumentsSettings.RequireCompatibleLayout(source);
            var before=Manifest(source,true);
            string staging=target+".migrating-"+Guid.NewGuid().ToString("N");Directory.CreateDirectory(staging);
            try {
                CopyTree(source,staging,true);Equal(before,Manifest(staging));Equal(before,Manifest(source,true));
                File.WriteAllLines(Path.Combine(staging,"legacy-migration-"+Guid.NewGuid().ToString("N")+".sha256"),ManifestLines(source,target,before));
                RequireStopped();RequireNoLinks(target);Directory.Move(staging,target);
            } finally {if(Directory.Exists(staging))Directory.Delete(staging,true);}
        }
        private static void CopyTree(string source, string target, bool skipDiagnostics = false)
        {
            RequireNoLinks(source);
            foreach (string child in Directory.GetFileSystemEntries(source))
            {
                if(skipDiagnostics && Path.GetFileName(child).Equals("diagnostics",StringComparison.OrdinalIgnoreCase))continue;
                RequireNoLinks(child);
                string destination = Path.Combine(target, Path.GetFileName(child));
                if (Directory.Exists(child)) { Directory.CreateDirectory(destination); CopyTree(child, destination); }
                else File.Copy(child, destination, false);
            }
        }

        private static string[] ManifestLines(string source, string target, Dictionary<string, string> files)
        {
            var lines = new List<string> { "# source=" + source, "# target=" + target, "# SHA256 relative-path" };
            var names = new List<string>(files.Keys); names.Sort(StringComparer.Ordinal);
            foreach (string name in names) lines.Add(files[name] + "  " + name);
            return lines.ToArray();
        }
    }
}
