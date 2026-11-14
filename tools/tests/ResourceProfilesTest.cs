using System;
using System.IO;
using System.IO.Compression;
using System.Linq;
using DeadSpaceResources;

internal static class ResourceProfilesTest
{
    private static int assertions;
    private static void Check(bool ok, string message) { if (!ok) throw new Exception(message); assertions++; }
    private static string Value(string ini, string key) { return File.ReadAllLines(ini).Single(line => line.StartsWith(key + "=")).Substring(key.Length + 1); }
    private static void Fixture(string path, string text)
    {
        using (var file = File.Create(path)) using (var zip = new ZipArchive(file, ZipArchiveMode.Create))
        foreach (string name in new[] { "Payload/Test.app/Info.plist", "Payload/Test.app/Dead Space", "Payload/Test.app/published/test" })
            using (var writer = new StreamWriter(zip.CreateEntry(name).Open())) writer.Write(text);
    }
    internal static int Main(string[] args)
    {
        try {
            string output = Path.GetFullPath(args[0]); if (Directory.Exists(output)) throw new Exception("New output required."); Directory.CreateDirectory(output);
            string[] ids = { "ipad-1.0.3", "iphone-1.0.3", "world-1.0.1", "current" };
            for (int i = 0; i < ids.Length; i++) {
                var p = IpaProfiles.Inspect(args[i + 1]); Check(p.Id == ids[i], "Real IPA identity " + ids[i]);
                Check(p.Lighting == (i == 0 || i == 3), "Lighting capability");
                Check(p.HdText == (i == 3) && p.Textures == (i == 3), "Enhanced components");
                Check(p.Describe(true).Contains("1.3.34") && p.Describe(false).Contains("issue"), "Bilingual recommendation / issues");
            }
            string data = Path.Combine(output, "data"); Directory.CreateDirectory(data);
            string[] roots = new[] { "first", "second" }.Select(n => Path.Combine(data, "marker-content", n)).ToArray();
            foreach (string root in roots) { Directory.CreateDirectory(Path.Combine(root, "payload")); File.WriteAllText(Path.Combine(root, "installation.json"), "{}"); Fixture(Path.Combine(root, "payload", "DeadSpace.ipa"), root); }
            Check(!IpaProfiles.Inspect(Path.Combine(roots[0], "payload", "DeadSpace.ipa")).Adapted, "Unknown remains unadapted");
            ResourceSelection.Activate(data, roots[0]); Check(ResourceSelection.Read(data) == roots[0], "First selected");
            ResourceSelection.Activate(data, roots[1]); Directory.SetLastWriteTimeUtc(roots[0], DateTime.UtcNow.AddHours(1)); Check(ResourceSelection.Read(data) == roots[1], "Directory timestamp cannot override selection");
            ResourceSelection.Activate(data, roots[0]); Check(ResourceSelection.Read(data) == roots[0], "Reselect older existing content");
            string before = File.ReadAllText(Path.Combine(data, "active-resource.json"));
            string bad = Path.Combine(data, "marker-content", "bad"); Directory.CreateDirectory(bad);
            bool rejected = false; try { ResourceSelection.Activate(data, bad); } catch (IOException) { rejected = true; }
            Check(rejected && before == File.ReadAllText(Path.Combine(data, "active-resource.json")), "Failed selection preserves active pointer");
            File.WriteAllText(Path.Combine(data, "active-resource.json"), "{\"schema\":1,\"content_id\":\"../escape\"}"); rejected = false;
            try { ResourceSelection.Read(data); } catch (InvalidDataException) { rejected = true; } Check(rejected, "Traversal rejected");
            string ini = Path.Combine(output, "settings.ini"); File.WriteAllText(ini, "user_binding=preserve\r\nhd_textures=1\r\ntexture_root_4=OLD\r\nadvanced_lighting=1\r\nlaunch_review_r4_completed=1\r\n");
            ResourceSelection.Configure(ini, roots[0], true, true);
            Check(Value(ini, "user_binding") == "preserve", "Personal settings retained");
            Check(Value(ini, "ipa") == Path.Combine(roots[0], "payload", "DeadSpace.ipa"), "IPA fully switched");
            Check(Value(ini, "advanced_lighting") == "0" && Value(ini, "hd_textures") == "0" && Value(ini, "native_inputs") == "0" && Value(ini, "overlay_enabled") == "0", "Unknown safe options");
            Check(Value(ini, "texture_root_4").StartsWith(roots[0]) && Value(ini, "launch_review_r4_completed") == "0", "Old texture paths cleared / review reset");
            Console.WriteLine("PASS " + assertions + " assertions; exact real identities, capabilities, bilingual text, active selection, failed / malicious selections and configuration replacement."); return 0;
        } catch (Exception e) { Console.Error.WriteLine(e); return 1; }
    }
}
