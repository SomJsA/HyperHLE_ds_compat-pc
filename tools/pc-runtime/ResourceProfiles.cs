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
    internal sealed class IpaLanguage
    {
        internal string Id;
        internal string Directory;
        internal string PreferredLanguages;
        internal string LabelZh;
        internal string LabelEn;
        internal bool HdOverlay;
        internal bool Complete = true;
        internal string FallbackId;
        internal string LaunchPreferredLanguages()
        {
            return Complete || String.IsNullOrEmpty(FallbackId) ? PreferredLanguages : PreferredLanguages + ",en-US,en";
        }
        internal string Display(bool english)
        {
            string label = english ? LabelEn : LabelZh;
            return Complete ? label : label + (english ? " (partial; missing entries fall back to English)" : "（部分资源，缺失条目回退英语）");
        }
    }

    // Match the uncompressed application, never the user supplied filename.
    // Recompressing an unchanged IPA therefore keeps its compatibility profile.
    internal sealed class IpaProfile
    {
        internal string Id = "unknown", Name = "Unrecognized IPA";
        internal bool Lighting, HdText, Textures;
        internal string ExecutableSha256, PlistSha256;
        internal readonly List<IpaLanguage> Languages = new List<IpaLanguage>();
        internal bool Adapted { get { return Id != "unknown"; } }
        internal bool Experimental { get { return Id == "unknown" && HdText; } }
        // Every recognized legacy build is identified, but only 1.3.34 has
        // completed chapter-12 gameplay acceptance. Keep the distinction
        // visible so the launcher can warn before trying an older binary.
        internal bool RequiresCompatibilityWarning { get { return Id != "current"; } }
        internal string Describe(bool english)
        {
            string support = Id == "current" ? (english ? "Most fully adapted: 1.3.34." : "开发者推荐：1.3.34 的适配最完整。")
                : Adapted ? (english ? "Opening and menu smoke-tested in R6. Legacy editions use the generic PC-to-touch compatibility path; the verified 1.3.34 native camera/HUD/action layout is not used. HD language banks and host fonts load, but binary-specific chapter/subtitle/objective takeover and full gameplay action acceptance still require per-edition verification." : "已在 R6 完成开场和菜单冒烟测试。旧版使用通用 PC 到触摸兼容路径，不套用已验证的 1.3.34 原生相机 / HUD / 动作布局。高清语言库和宿主字体会加载，但章节、字幕、任务接管以及完整游戏内动作仍需按二进制逐版验证。")
                : Experimental ? (english ? "This IPA fingerprint is not recognized. Experimental PC controls and HD text are enabled by default for a trial; turn either off if it does not work." : "未识别此 IPA 指纹；默认试用实验性 PC 操作和高清文字组件，遇到问题可分别关闭。")
                : (english ? "Not yet adapted or verified. Experimental native resources only; do not assume this package is supported." : "尚未适配或验证；仅尝试原生资源，不代表此构建已兼容。");
            return Name + "\r\n" + support
                + "\r\n" + (english ? "Lighting: " : "光影：") + (Lighting ? (english ? "native standard / advanced." : "原生标准 / 高级可切换。") : (english ? "native standard only; advanced unavailable." : "仅原生标准；不开放高级光影。"))
                + "\r\n" + (english ? "Textures: " : "纹理：") + (Textures ? (english ? "original, or matching marker packs at 2x / 4x / 6x when supplied." : "原版；匹配的 marker 提供时可选 2× / 4× / 6×。") : (english ? "original only; external HD packs are not adapted to this edition." : "仅原版；当前外置高清包未适配此构建。"))
                + "\r\n" + (english ? "IPA languages: " : "IPA 内置语言：") + (Languages.Count == 0 ? (english ? "not detected." : "未检测到。") : String.Join(", ", Languages.Select(language => language.Display(english)).ToArray()))
                + "\r\n" + (english ? "Automatic internal resolution follows the detected output height and this IPA's original guest surface. Manual 1x-8x remains available; 8x is not an automatic limit and may need substantial VRAM. Output resolution and MSAA / AF remain host settings." : "自动内部分辨率按检测到的输出高度和此 IPA 的原始客体画面匹配。手动 1×-8× 仍可用；8×不是自动上限，可能需要大量显存。输出分辨率、MSAA / AF 仍是宿主设置。")
                + "\r\n" + (english ? "HD text is installed with the PC program and does not require a marker. External texture packs remain edition-specific. Developer recommendation: 1.3.34. Other IPA owners are welcome to report issues with version / file fingerprints; arrange sample sharing privately." : "高清文字组件随 PC 程序安装，不依赖 marker。外置高清纹理仍按 IPA 版本分别适配。开发者推荐 1.3.34；也欢迎携带其他 IPA 的版本和文件指纹提 issue，样本另行私下联系。")
                + (Id == "current" ? "" : "\r\n" + (english ? "Legacy progress is stored separately from 1.3.34 and is never migrated automatically. Save editing / transfer tools for these editions are not adapted." : "旧包进度与 1.3.34 分开保存，不自动跨版迁移；旧包存档编辑与互导工具尚未适配。"))
                + "\r\nhttps://github.com/SomJsA/DeadSpaceMobile-Community/issues";
        }
    }

    internal static class IpaProfiles
    {
        private static readonly string[][] Known = {
            new[] { "current", "Dead Space 1.3.34", "4732db2574300a5407b783c72e0caa6002e961441379cbb86ef5780fa06d36f0", "90fdce9a5b90f84a39156050c1011d36168357085e466fa268821b6ea03010b5" },
            new[] { "ipad-1.0.3", "Dead Space iPad 1.0.3", "28888b16cf63e7b548f197d046f71bb658a0ba05dd3fb33d015f64c820022d2f", "e5811c990a9797338f8fd76c78aa356b9b9a8997b5875df27783a607d65a2206" },
            new[] { "ipad-world-1.3.11", "Dead Space iPad World 1.3.11", "4be1a481ca9127dd118a7088b2810fb1fd86e69210d952f248335b9a07b797a6", "e4abcb790afea3c084976b2a31e6e1b8e34a4f87079d46ada24012a4220d504b" },
            new[] { "ipad-1.0.3-build-1.3.11", "Dead Space iPad 1.0.3 (build 1.3.11)", "dfaf979d0726de925d803c4baa9108acf7fdbfae221618b11ec1b315e9c9305b", "650e741753567e9b55cfaf9eb9f3f2b00397074b5b6ed1f9b1abab6e4cf8795d" },
            new[] { "iphone-1.0.3", "Dead Space iPhone 1.0.3 (build 1.3.8)", "cb751c1e6c4dffb67b08496c574df2bbf9bf637a86a1f44ce0d2c0cd2a7f3436", "0d83bece924b68c58c20353daeec21f9f1a6a064ad39170a2dc5391d8228c8cf" },
            new[] { "world-1.0.1", "Dead Space World 1.0.1", "311a769fc23614981649384fb264f638574999dde05bbe709faef3a5cd5aca8f", "7c67ee8cec2d88c64e6a643369c3b29f7f7f5f5b077d8380cf66210fc8a80851" }
        };
        private static readonly IpaLanguage[] LanguageCatalog = {
            new IpaLanguage { Id = "zh-tieba", Directory = "CHS_CN", PreferredLanguages = "zh-Hans,zh", LabelZh = "中文-贴吧汉化组", LabelEn = "Chinese - Tieba Translation Group", HdOverlay = true },
            new IpaLanguage { Id = "en-original", Directory = "ENG_US", PreferredLanguages = "en-US,en", LabelZh = "English-原生 IPA", LabelEn = "English - Original IPA", HdOverlay = true },
            new IpaLanguage { Id = "fr-original", Directory = "FRE_FR", PreferredLanguages = "fr-FR,fr", LabelZh = "Français-原生 IPA", LabelEn = "Français - Original IPA", HdOverlay = true },
            new IpaLanguage { Id = "de-original", Directory = "GER_DE", PreferredLanguages = "de-DE,de", LabelZh = "Deutsch-原生 IPA", LabelEn = "Deutsch - Original IPA", HdOverlay = true },
            new IpaLanguage { Id = "it-original", Directory = "ITA_IT", PreferredLanguages = "it-IT,it", LabelZh = "Italiano-原生 IPA", LabelEn = "Italiano - Original IPA", HdOverlay = true },
            new IpaLanguage { Id = "ja-original", Directory = "JPN_JP", PreferredLanguages = "ja-JP,ja", LabelZh = "日本語-原生 IPA", LabelEn = "日本語 - Original IPA", HdOverlay = true },
            new IpaLanguage { Id = "ko-original", Directory = "KOR_KR", PreferredLanguages = "ko-KR,ko", LabelZh = "한국어-原生 IPA", LabelEn = "한국어 - Original IPA", HdOverlay = true },
            new IpaLanguage { Id = "es-original", Directory = "SPA_ES", PreferredLanguages = "es-ES,es", LabelZh = "Español-原生 IPA", LabelEn = "Español - Original IPA", HdOverlay = true }
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
                bool hasPublished = zip.Entries.Any(e => e.Length > 0 && e.FullName.StartsWith(prefix + "published/", StringComparison.Ordinal));
                if (!hasPublished) return profile;
                if (known == null)
                {
                    // Unknown versions may still expose the standard stringdata
                    // layout. Offer an explicitly experimental host overlay and
                    // compatibility input path; never claim native high-tier or
                    // texture support for an unrecognized fingerprint.
                    profile.Name = "Unrecognized IPA";
                    profile.HdText = true;
                    profile.Textures = false;
                }
                else
                {
                    bool high = known[0] == "current" || known[0].StartsWith("ipad-", StringComparison.OrdinalIgnoreCase);
                    if (high && !zip.Entries.Any(e => e.Length > 0 && e.FullName.StartsWith(prefix + "published_ipad2/", StringComparison.Ordinal))) return profile;
                    profile.Id = known[0]; profile.Name = known[1]; profile.Lighting = high;
                    // The PC HD text layer is host-side and is not tied to the
                    // external texture marker. Every known IPA can use it; only
                    // the 1.3.34 profile currently has matching texture packs.
                    profile.HdText = true;
                    profile.Textures = profile.Id == "current";
                }
                string stringData = prefix + "published/stringdata/";
                ZipArchiveEntry englishBank = zip.GetEntry(stringData + "ENG_US/deadspace_iphone.bin");
                long englishBankLength = englishBank == null ? 0 : englishBank.Length;
                foreach (IpaLanguage language in LanguageCatalog)
                {
                    string member = stringData + language.Directory + "/deadspace_iphone.bin";
                    ZipArchiveEntry bank = zip.GetEntry(member);
                    if (bank != null)
                    {
                        profile.Languages.Add(new IpaLanguage {
                            Id = language.Id, Directory = language.Directory,
                            PreferredLanguages = language.PreferredLanguages,
                            LabelZh = language.LabelZh, LabelEn = language.LabelEn,
                            HdOverlay = language.HdOverlay,
                            // Legacy World banks use a different key layout; compare against
                            // the same IPA's English bank instead of fixed key names or size.
                            Complete = englishBankLength <= 0 || bank.Length >= Math.Max(32768, englishBankLength / 2),
                            FallbackId = language.Id == "en-original" ? null : "en-original"
                        });
                    }
                }
                return profile;
            }
        }
        internal static IpaLanguage FindLanguage(IpaProfile profile, string id)
        {
            if (String.Equals(id, "native", StringComparison.OrdinalIgnoreCase)) return null;
            if (profile != null && !String.IsNullOrEmpty(id))
            {
                IpaLanguage match = profile.Languages.FirstOrDefault(language => String.Equals(language.Id, id, StringComparison.OrdinalIgnoreCase));
                if (match != null) return match;
            }
            return profile == null ? null : profile.Languages.FirstOrDefault(language => language.Id == "zh-tieba");
        }
        internal static string NormalizeLanguage(IpaProfile profile, string id, bool enabled)
        {
            if (!enabled || profile == null || !profile.HdText) return "native";
            IpaLanguage match = FindLanguage(profile, id);
            return match == null ? "native" : match.Id;
        }
        internal static IpaLanguage EffectiveLanguage(IpaProfile profile, string id)
        {
            IpaLanguage selected = FindLanguage(profile, id);
            if (selected == null || selected.Complete || String.IsNullOrEmpty(selected.FallbackId)) return selected;
            return FindLanguage(profile, selected.FallbackId) ?? selected;
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
            Configure(ini, installed, hdChinese ? "zh-tieba" : "native", reset);
        }
        internal static void Configure(string ini, string installed, string languageId, bool reset)
        {
            var p = IpaProfiles.Inspect(Path.Combine(installed, "payload", "DeadSpace.ipa"));
            string selectedLanguage = IpaProfiles.NormalizeLanguage(p, languageId, languageId != "native");
            IpaLanguage language = IpaProfiles.FindLanguage(p, selectedLanguage);
            bool verifiedNativeInput = p.Id == "current";
            var values = new Dictionary<string, string>(StringComparer.OrdinalIgnoreCase) {
                { "ipa", Path.Combine(installed, "payload", "DeadSpace.ipa") }, { "resource_profile", p.Id },
                { "overlay_enabled", p.HdText && language != null ? "1" : "0" },
                { "hd_text_language", selectedLanguage }
            };
            if (!p.Lighting) values["advanced_lighting"] = "0";
            if (!verifiedNativeInput) values["native_inputs"] = values["native_hud"] = "0";
            if (!p.Textures) values["hd_textures"] = "0";
            if (reset) {
                values["launch_review_r4_completed"] = "0";
                values["advanced_lighting"] = p.Lighting ? "1" : "0";
                values["native_inputs"] = values["native_hud"] = verifiedNativeInput ? "1" : "0";
                values["hd_textures"] = "0";
                selectedLanguage = IpaProfiles.NormalizeLanguage(p, languageId, languageId != "native");
                values["hd_text_language"] = selectedLanguage;
                language = IpaProfiles.FindLanguage(p, selectedLanguage);
                values["overlay_enabled"] = p.HdText && language != null ? "1" : "0";
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
