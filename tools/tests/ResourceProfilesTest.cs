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
            string[] bundles = { "com.ea.deadspace.ipad.bv", "com.ea.deadspace.inc", "com.ea.deadspace.bv", "com.ea.deadspace.bv" };
            for (int i = 0; i < ids.Length; i++) {
                var p = IpaProfiles.Inspect(args[i + 1]); Check(p.Id == ids[i], "Real IPA identity " + ids[i]);
                Check(p.BundleIdentifier == bundles[i], "Sandbox bundle for " + ids[i]);
                Check(p.Lighting == (i == 0 || i == 3), "Lighting capability");
                Check(p.HdText && p.Textures == (i == 3), "HD text / texture components");
                Check(p.Languages.Count == 8, "IPA embedded language banks " + ids[i]);
                Check(DeadSpaceResources.IpaProfiles.FindLanguage(p, "zh-tieba") != null && DeadSpaceResources.IpaProfiles.FindLanguage(p, "en-original") != null, "Chinese / English language choices");
                Check(DeadSpaceResources.IpaProfiles.FindLanguage(p, "zh-tieba").HdOverlay && DeadSpaceResources.IpaProfiles.FindLanguage(p, "ja-original").HdOverlay && DeadSpaceResources.IpaProfiles.FindLanguage(p, "de-original").HdOverlay && !DeadSpaceResources.IpaProfiles.FindLanguage(p, "ja-original").Complete && DeadSpaceResources.IpaProfiles.EffectiveLanguage(p, "ja-original").Id == "en-original" && DeadSpaceResources.IpaProfiles.FindLanguage(p, "fr-original").PreferredLanguages == "fr-FR,fr", "multilingual HD overlay, incomplete Japanese fallback and locale mapping");
                Check(p.Describe(true).Contains("1.3.34") && p.Describe(false).Contains("issue"), "Bilingual recommendation / issues");
            }
            if (args.Length >= 7)
            {
                string[] extraIds = { "ipad-world-1.3.11", "ipad-1.0.3-build-1.3.11" };
                for (int i = 0; i < extraIds.Length; i++)
                {
                    var p = IpaProfiles.Inspect(args[5 + i]);
                    Check(p.Id == extraIds[i], "Real IPA identity " + extraIds[i]);
                    Check(p.BundleIdentifier == "com.ea.deadspace.ipad.bv", "Sandbox bundle for " + extraIds[i]);
                    Check(p.Lighting && p.HdText && !p.Textures, "New iPad capability " + extraIds[i]);
                    Check(p.Languages.Count == 8, "New iPad language banks " + extraIds[i]);
                    Check(p.Describe(true).Contains("1.3.34") && p.Describe(false).Contains("旧版"), "New iPad bilingual compatibility disclosure");
                }
            }
            string data = Path.Combine(output, "data"); Directory.CreateDirectory(data);
            string[] roots = new[] { "first", "second" }.Select(n => Path.Combine(data, "marker-content", n)).ToArray();
            foreach (string root in roots) { Directory.CreateDirectory(Path.Combine(root, "payload")); File.WriteAllText(Path.Combine(root, "installation.json"), "{}"); Fixture(Path.Combine(root, "payload", "DeadSpace.ipa"), root); }
            var unknownProfile = IpaProfiles.Inspect(Path.Combine(roots[0], "payload", "DeadSpace.ipa"));
            Check(unknownProfile.BundleIdentifier == null, "Unknown IPA must not select a known sandbox bundle");
            Check(!unknownProfile.Adapted && unknownProfile.Experimental && unknownProfile.HdText && !unknownProfile.Textures, "Unknown published IPA uses explicit experimental HD-text trial");
            Check(unknownProfile.Describe(false).Contains("未识别") && unknownProfile.Describe(true).Contains("not recognized"), "Unknown IPA warning is bilingual");
            ResourceSelection.Activate(data, roots[0]); Check(ResourceSelection.Read(data) == roots[0], "First selected");
            ResourceSelection.Activate(data, roots[1]); Directory.SetLastWriteTimeUtc(roots[0], DateTime.UtcNow.AddHours(1)); Check(ResourceSelection.Read(data) == roots[1], "Directory timestamp cannot override selection");
            ResourceSelection.Activate(data, roots[0]); Check(ResourceSelection.Read(data) == roots[0], "Reselect older existing content");
            string before = File.ReadAllText(Path.Combine(data, "active-resource.json"));
            string bad = Path.Combine(data, "marker-content", "bad"); Directory.CreateDirectory(bad);
            bool rejected = false; try { ResourceSelection.Activate(data, bad); } catch (IOException) { rejected = true; } catch (InvalidDataException) { rejected = true; }
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
