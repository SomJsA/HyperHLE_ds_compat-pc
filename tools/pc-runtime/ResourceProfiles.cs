using System;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Web.Script.Serialization;

namespace DeadSpaceResources
{
    // Match the uncompressed application, never the user supplied filename.
    // Recompressing an unchanged IPA therefore keeps its compatibility profile.
    internal sealed class IpaProfile
    {
        internal string Id = "unknown", Name = "Unrecognized IPA";
        internal bool Lighting, HdText, Textures;
        internal string ExecutableSha256, PlistSha256;
        internal bool Adapted { get { return Id != "unknown"; } }
        internal string Describe(bool english)
        {
            string support = Id == "current" ? (english ? "Most fully adapted: 1.3.34." : "开发者推荐：1.3.34 的适配最完整。")
                : Adapted ? (english ? "Adapted in R4.2; complete opening tested. Legacy editions retain native touch controls and native text." : "已在 R4.2 适配并验证完整开场；旧版沿用原生触摸和原生文字。")
                : (english ? "Not yet adapted or verified. Experimental native resources only; do not assume this package is supported." : "尚未适配或验证；仅尝试原生资源，不代表此构建已兼容。");
            return Name + "\r\n" + support
                + "\r\n" + (english ? "Lighting: " : "光影：") + (Lighting ? (english ? "native standard / advanced." : "原生标准 / 高级可切换。") : (english ? "native standard only; advanced unavailable." : "仅原生标准；不开放高级光影。"))
                + "\r\n" + (english ? "Textures: " : "纹理：") + (Textures ? (english ? "original, or matching marker packs at 2x / 4x / 6x when supplied." : "原版；匹配的 marker 提供时可选 2× / 4× / 6×。") : (english ? "original only; external HD packs are not adapted to this edition." : "仅原版；当前外置高清包未适配此构建。"))
                + "\r\n" + (english ? "Internal / output resolution and MSAA / AF remain host settings; hardware limits apply. Opening validation used internal 2x, MSAA 4x / AF 4x; higher settings are not validated limits." : "内部 / 输出分辨率、MSAA / AF 为宿主设置，受硬件限制；开场实测内部 2×、MSAA 4× / AF 4×，不将更高选项当成已验证上限。")
                + "\r\n" + (english ? "No external texture pack is required. Developer recommendation: 1.3.34. Other IPA owners are welcome to report issues with version / file fingerprints; arrange sample sharing privately." : "不依赖外置纹理也可运行。开发者推荐 1.3.34；也欢迎携带其他 IPA 的版本和文件指纹提 issue，样本另行私下联系。")
                + (Id == "current" ? "" : "\r\n" + (english ? "Legacy progress is stored separately from 1.3.34 and is never migrated automatically. Save editing / transfer tools for these editions are not adapted." : "旧包进度与 1.3.34 分开保存，不自动跨版迁移；旧包存档编辑与互导工具尚未适配。"))
                + "\r\nhttps://github.com/SomJsA/DeadSpaceMobile-Community/issues";
        }
    }

    internal static class IpaProfiles
    {
        private static readonly string[][] Known = {
            new[] { "current", "Dead Space 1.3.34", "4732db2574300a5407b783c72e0caa6002e961441379cbb86ef5780fa06d36f0", "90fdce9a5b90f84a39156050c1011d36168357085e466fa268821b6ea03010b5" },
            new[] { "ipad-1.0.3", "Dead Space iPad 1.0.3", "28888b16cf63e7b548f197d046f71bb658a0ba05dd3fb33d015f64c820022d2f", "e5811c990a9797338f8fd76c78aa356b9b9a8997b5875df27783a607d65a2206" },
            new[] { "iphone-1.0.3", "Dead Space iPhone 1.0.3 (build 1.3.8)", "cb751c1e6c4dffb67b08496c574df2bbf9bf637a86a1f44ce0d2c0cd2a7f3436", "0d83bece924b68c58c20353daeec21f9f1a6a064ad39170a2dc5391d8228c8cf" },
            new[] { "world-1.0.1", "Dead Space World 1.0.1", "311a769fc23614981649384fb264f638574999dde05bbe709faef3a5cd5aca8f", "7c67ee8cec2d88c64e6a643369c3b29f7f7f5f5b077d8380cf66210fc8a80851" }
        };
        internal static string Digest(Stream source)
        { using (var sha = SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(source)).Replace("-", "").ToLowerInvariant(); }
        private static string Digest(ZipArchiveEntry entry, long limit)
        {
            if (entry == null || entry.Length <= 0 || entry.Length > limit) throw new InvalidDataException("Invalid IPA application entry.");
            using (var stream = entry.Open()) return Digest(stream);
        }
        internal static IpaProfile Inspect(string source)
        {
            using (var file = new FileStream(source, FileMode.Open, FileAccess.Read, FileShare.Read))
            using (var zip = new ZipArchive(file, ZipArchiveMode.Read, false))
            {
                if (!Path.GetExtension(source).Equals(".ipa", StringComparison.OrdinalIgnoreCase))
                {
                    var manifest = zip.GetEntry("vandal.json");
                    if (manifest == null || manifest.Length > 16 * 1024 * 1024) throw new InvalidDataException("Invalid marker manifest.");
                    Dictionary<string, object> root;
                    using (var reader = new StreamReader(manifest.Open())) root = new JavaScriptSerializer { MaxJsonLength = 16 * 1024 * 1024 }.DeserializeObject(reader.ReadToEnd()) as Dictionary<string, object>;
                    var game = root["game"] as Dictionary<string, object>;
                    string name = Convert.ToString(game["ipa_path"]);
                    var entry = zip.GetEntry(name);
                    if (entry == null || entry.Length > 2L * 1024 * 1024 * 1024) throw new InvalidDataException("Missing marker IPA.");
                    // Nested ZIP needs seeking. The temporary file is not an IPA derivative.
                    string temporary = Path.Combine(Path.GetTempPath(), "ds-inspect-" + Guid.NewGuid().ToString("N") + ".ipa");
                    try {
                        using (var input = entry.Open()) using (var output = new FileStream(temporary, FileMode.CreateNew)) input.CopyTo(output);
                        return Inspect(temporary);
                    } finally { if (File.Exists(temporary)) File.Delete(temporary); }
                }
                var plists = zip.Entries.Where(e => e.FullName.StartsWith("Payload/", StringComparison.Ordinal) && e.FullName.EndsWith(".app/Info.plist", StringComparison.Ordinal) && e.FullName.Count(c => c == '/') == 2).ToArray();
                if (plists.Length != 1 || zip.Entries.Select(e => e.FullName).Distinct(StringComparer.OrdinalIgnoreCase).Count() != zip.Entries.Count) throw new InvalidDataException("Ambiguous IPA application.");
                string prefix = plists[0].FullName.Substring(0, plists[0].FullName.Length - "Info.plist".Length);
                var exe = zip.GetEntry(prefix + "Dead Space");
                var profile = new IpaProfile { PlistSha256 = Digest(plists[0], 1024 * 1024) };
                if (exe == null) return profile;
                profile.ExecutableSha256 = Digest(exe, 128 * 1024 * 1024);
                var known = Known.SingleOrDefault(k => k[2] == profile.ExecutableSha256 && k[3] == profile.PlistSha256);
                if (known == null || !zip.Entries.Any(e => e.Length > 0 && e.FullName.StartsWith(prefix + "published/", StringComparison.Ordinal))) return profile;
                bool high = known[0] == "current" || known[0] == "ipad-1.0.3";
                if (high && !zip.Entries.Any(e => e.Length > 0 && e.FullName.StartsWith(prefix + "published_ipad2/", StringComparison.Ordinal))) return profile;
                profile.Id = known[0]; profile.Name = known[1]; profile.Lighting = high;
                profile.HdText = profile.Textures = profile.Id == "current";
                return profile;
            }
        }
    }

    internal static class ResourceSelection
    {
        private static void NoLinks(string path)
        {
            for (string p = Path.GetFullPath(path); !String.IsNullOrEmpty(p); p = Path.GetDirectoryName(p))
                if ((Directory.Exists(p) || File.Exists(p)) && (File.GetAttributes(p) & FileAttributes.ReparsePoint) != 0) throw new IOException("Resource path contains a link.");
        }
        internal static string Read(string dataRoot)
        {
            string record = Path.Combine(dataRoot, "active-resource.json");
            NoLinks(record);
            if (!File.Exists(record)) return null; // Legacy installation: caller may migrate its old selection.
            if (new FileInfo(record).Length > 16384) throw new InvalidDataException("Invalid active resource record.");
            var obj = new JavaScriptSerializer().DeserializeObject(File.ReadAllText(record)) as Dictionary<string, object>;
            if (obj == null || !obj.ContainsKey("schema") || Convert.ToString(obj["schema"]) != "1") throw new InvalidDataException("Unknown active resource record schema.");
            string leaf = obj == null || !obj.ContainsKey("content_id") ? "" : Convert.ToString(obj["content_id"]);
            if (String.IsNullOrEmpty(leaf) || leaf.StartsWith(".") || leaf.IndexOfAny(new[] { '/', '\\', ':' }) >= 0 || leaf != Path.GetFileName(leaf)) throw new InvalidDataException("Invalid active resource selection.");
            string root = Path.Combine(dataRoot, "marker-content", leaf); NoLinks(root);
            if (!File.Exists(Path.Combine(root, "installation.json")) || !File.Exists(Path.Combine(root, "payload", "DeadSpace.ipa"))) throw new InvalidDataException("Active resources are incomplete; select IPA / marker again.");
            return Path.GetFullPath(root);
        }
        internal static void Activate(string dataRoot, string installed)
        {
            string content = Path.GetFullPath(Path.Combine(dataRoot, "marker-content"));
            installed = Path.GetFullPath(installed); NoLinks(installed);
            if (!String.Equals(Path.GetDirectoryName(installed), content, StringComparison.OrdinalIgnoreCase)) throw new InvalidDataException("Resource selection outside GameData.");
            if (!File.Exists(Path.Combine(installed, "installation.json"))) throw new InvalidDataException("Missing installation record.");
            // Inspect before changing the pointer. Failed installs leave the old selection active.
            var profile = IpaProfiles.Inspect(Path.Combine(installed, "payload", "DeadSpace.ipa"));
            string target = Path.Combine(dataRoot, "active-resource.json"); NoLinks(target);
            WriteAtomic(target, new JavaScriptSerializer().Serialize(new { schema = 1, content_id = Path.GetFileName(installed), profile = profile.Id, executable_sha256 = profile.ExecutableSha256, plist_sha256 = profile.PlistSha256 }));
        }
        internal static void WriteAtomic(string path, string value)
        {
            NoLinks(path); Directory.CreateDirectory(Path.GetDirectoryName(path));
            string temporary = path + ".tmp-" + Guid.NewGuid().ToString("N");
            try { File.WriteAllText(temporary, value, new UTF8Encoding(false)); if (File.Exists(path)) File.Replace(temporary, path, null); else File.Move(temporary, path); }
            finally { if (File.Exists(temporary)) File.Delete(temporary); }
        }
        internal static void Configure(string ini, string installed, bool hdChinese, bool reset)
        {
            var p = IpaProfiles.Inspect(Path.Combine(installed, "payload", "DeadSpace.ipa"));
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase) {
                { "ipa", Path.Combine(installed, "payload", "DeadSpace.ipa") }, { "resource_profile", p.Id },
                { "overlay_enabled", p.HdText && hdChinese ? "1" : "0" }
            };
            if (!p.Lighting) values["advanced_lighting"] = "0";
            if (!p.HdText) values["native_inputs"] = values["native_hud"] = "0";
            if (!p.Textures) values["hd_textures"] = "0";
            if (reset) {
                values["launch_review_r4_completed"] = "0";
                values["advanced_lighting"] = p.Lighting ? "1" : "0";
                values["native_inputs"] = values["native_hud"] = p.HdText ? "1" : "0";
                values["hd_textures"] = "0";
                foreach (string tier in new[] { "2", "4", "6" }) values["texture_root_" + tier] = Path.Combine(installed, "payload", "texture-packs", tier);
                string available = new[] { "4", "2", "6" }.FirstOrDefault(t => File.Exists(Path.Combine(installed, "payload", "texture-packs", t, "PACK.json")));
                values["texture_scale"] = available ?? "4";
                if (p.Textures && available != null) values["hd_textures"] = "1";
            }
            var lines = new List<string>();
            foreach (string line in File.Exists(ini) ? File.ReadAllLines(ini) : new string[0]) {
                int eq = line.IndexOf('=');
                if (eq > 0 && values.ContainsKey(line.Substring(0, eq).Trim())) continue;
                lines.Add(line);
            }
            lines.AddRange(values.Select(kv => kv.Key + "=" + kv.Value));
            WriteAtomic(ini, String.Join("\r\n", lines) + "\r\n");
        }
    }
}
