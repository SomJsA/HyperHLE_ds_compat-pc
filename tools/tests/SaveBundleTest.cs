using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Security.Cryptography;
using DeadSpaceBootstrap;

internal static class SaveBundleTest
{
    static void Check(bool condition, string message) { if (!condition) throw new Exception(message); }
    static Dictionary<string, string> Snapshot(string root)
    {
        var result = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase);
        foreach (string path in Directory.GetFiles(root, "*", SearchOption.AllDirectories))
            using (var hash = SHA256.Create()) result.Add(path.Substring(root.Length).TrimStart('\\').Replace('\\', '/'), Convert.ToBase64String(hash.ComputeHash(File.ReadAllBytes(path))));
        return result;
    }
    static void Same(Dictionary<string, string> a, Dictionary<string, string> b)
    { Check(a.Count == b.Count, "File count changed"); foreach (var file in a) Check(b.ContainsKey(file.Key) && b[file.Key] == file.Value, "Changed: " + file.Key); }
    static string Docs(string root) { return Path.Combine(SaveStorage.ProfilePath(root), "Documents"); }
    static void Copy(string source, string target)
    { foreach (string path in Directory.GetFiles(source, "*", SearchOption.AllDirectories)) { string dest = Path.Combine(target, path.Substring(source.Length).TrimStart('\\')); Directory.CreateDirectory(Path.GetDirectoryName(dest)); File.Copy(path, dest); } }
    static void Reject(Action action) { try { action(); } catch (IOException) { return; } catch (InvalidDataException) { return; } throw new Exception("Malformed input accepted"); }
    static void BadZip(string file, string[] names, byte[] bytes)
    { using (var stream = new FileStream(file, FileMode.CreateNew)) using (var zip = new ZipArchive(stream, ZipArchiveMode.Create)) foreach (string name in names) { var entry = zip.CreateEntry(name); using (var dest = entry.Open()) dest.Write(bytes, 0, bytes.Length); } }
    public static void Main(string[] args)
    {
        if (args[0] == "verify") { string target = args[2]; Check(!Directory.Exists(target), "Destination exists"); SaveBundle.Import(target, args[1]); Same(Snapshot(args[3]), Snapshot(Docs(target))); Console.WriteLine("PASS: Android ZIP imported on PC, every save/dependency byte unchanged"); return; }
        string source = args[0], root = args[1]; Check(!Directory.Exists(root), "Test root exists"); Directory.CreateDirectory(root);
        var original = Snapshot(source); string first = Path.Combine(root, "first"); Copy(source, Docs(first));
        string zipFile = Path.Combine(root, "pc-three-slots.zip"); SaveBundle.Export(first, zipFile); Check(File.Exists(zipFile), "No ZIP");
        using (var zip = ZipFile.OpenRead(zipFile)) for (int i = 0; i < 3; i++) Check(zip.GetEntry(String.Format("Documents/var/{0:00}_CheckpointSave.sb", i)) != null, "Slot missing");
        string restored = Path.Combine(root, "restored"); Check(SaveBundle.Import(restored, zipFile) == null, "Fresh import made backup"); Same(original, Snapshot(Docs(restored)));
        string backup = SaveBundle.Import(restored, zipFile); Check(backup != null && File.Exists(backup), "No pre-import backup"); Same(original, Snapshot(Docs(restored)));
        string recover = Path.Combine(root, "recovered"); SaveBundle.Import(recover, backup); Same(original, Snapshot(Docs(recover)));
        byte[] valid = File.ReadAllBytes(Path.Combine(source, "var", "00_CheckpointSave.sb"));
        foreach (var item in new[] { new[] { "traversal", "../bad.sb" }, new[] { "colon", "Documents/var/slot:bad.sb" }, new[] { "duplicate", "Documents/var/00_CheckpointSave.sb", "documents/var/00_checkpointsAVE.sb" } })
        { string bad = Path.Combine(root, item[0] + ".zip"); string[] names = new string[item.Length - 1]; Array.Copy(item, 1, names, 0, names.Length); BadZip(bad, names, valid); Reject(() => SaveBundle.Import(restored, bad)); Same(original, Snapshot(Docs(restored))); }
        valid[valid.Length - 8] ^= 1; string corrupt = Path.Combine(root, "corrupt.zip"); BadZip(corrupt, new[] { "Documents/var/00_CheckpointSave.sb" }, valid); Reject(() => SaveBundle.Import(restored, corrupt)); Same(original, Snapshot(Docs(restored)));
        Reject(() => SaveBundle.Export(first, zipFile)); Same(original, Snapshot(source));
        Console.WriteLine("PASS: three-slot ZIP; byte-exact import; automatic backup and recovery; traversal/colon/duplicate/corrupt refusal without writes; existing ZIP protected; source untouched");
    }
}
