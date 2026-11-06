using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Reflection;
using System.Security.Cryptography;
using System.Text;

namespace DeadSpaceBootstrap
{
    internal static class VisualCompatibility
    {
        // Permanent correctness baseline, user accepted 2026-10-30 (R4).
        // Keep repairs when changing quality/performance presets or packaging.
        // The embedded profile + build contract prevent inherited-environment
        // omissions; changing a repair also requires its original visual repro.
        internal static Dictionary<string,string> ReadDefaults()
        {
            var values = new Dictionary<string,string>(StringComparer.Ordinal);
            using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("DeadSpaceBootstrap.VisualCompat.ini"))
            {
                if (stream == null) throw new InvalidDataException("缺少内置画面兼容配置。");
                using (var reader = new StreamReader(stream))
                {
                    string line;
                    while ((line = reader.ReadLine()) != null)
                    {
                        line = line.Trim();
                        if (line.Length == 0 || line.StartsWith("#")) continue;
                        int split = line.IndexOf('=');
                        if (split <= 0 || !line.StartsWith("TOUCHHLE_DS_")) throw new InvalidDataException("画面兼容配置无效。");
                        values.Add(line.Substring(0, split), line.Substring(split + 1));
                    }
                }
            }
            foreach (string name in new[] { "TOUCHHLE_DS_PC_DEPTH24", "TOUCHHLE_DS_OPENING_FOG_FIX", "TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH", "TOUCHHLE_DS_DEPTH_LEQUAL_HASH" })
                if (!values.ContainsKey(name)) throw new InvalidDataException("画面兼容配置不完整：" + name);
            return values;
        }

        // Apply before the existing UI overrides. Depth precision must survive
        // MSAA=off; texture sync, scale, AA, FSR and mip checkboxes stay explicit.
        internal static void Apply()
        {
            foreach (var entry in ReadDefaults())
            {
                if (entry.Key == "TOUCHHLE_DS_DEPTH_LEQUAL_HASH" && Environment.GetEnvironmentVariable("TOUCHHLE_DS_DIAG_PRESERVE_DEPTH_HASH") == "1") continue;
                Environment.SetEnvironmentVariable(entry.Key, entry.Value);
            }
        }

        // Checking the embedded profile alone cannot catch a later quality
        // preset clearing a repair. Check the actual child environment at the
        // last launch boundary. Extra content keys and explicit mip-generation
        // choices remain allowed; normal presets cannot remove these repairs.
        private static void RequireRepairs(System.Collections.Specialized.StringDictionary environment)
        {
            if (environment["TOUCHHLE_DS_PC_DEPTH24"] != "1")
                throw new InvalidDataException("画面兼容修复被覆盖：请恢复启动器设置后重试。");
            foreach (string name in new[] { "TOUCHHLE_DS_AUTO_MIPS_HASH", "TOUCHHLE_DS_CPU_ALPHA_MIPS_HASH" })
            {
                var keys = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
                foreach (string key in (environment[name] ?? "").Split(',')) keys.Add(key.Trim());
                foreach (string required in new[] { "0712153f", "24bec7ad", "60bb1a47", "e0549cfb", "02d3486c" })
                    if (!keys.Contains(required))
                        throw new InvalidDataException("画面兼容纹理修复不完整：请恢复启动器设置后重试。");
            }
        }

        // Record the actual child environment, after UI settings/diagnostic
        // overrides. A copied launcher must be auditable without its old wrapper.
        internal static void WriteEffective(string directory, ProcessStartInfo info)
        {
            RequireRepairs(info.EnvironmentVariables);
            var names = new SortedSet<string>(ReadDefaults().Keys, StringComparer.Ordinal);
            foreach (string name in new[] { "TOUCHHLE_USER_DATA_DIR", "TOUCHHLE_DS_MSAA", "TOUCHHLE_DS_ANISO", "TOUCHHLE_DS_AUTO_OPENING_MIPS", "TOUCHHLE_DS_AUTO_MIPS_ALL", "TOUCHHLE_DS_TEX_UPLOAD_FINISH", "TOUCHHLE_DS_NO_PROJ_Z_FIX", "TOUCHHLE_DS_NO_FOG", "TOUCHHLE_DS_NO_FOG_SCREEN", "TOUCHHLE_DS_GPU_PRESENT_COPY", "TOUCHHLE_DS_GPU_TEXT_OVERLAY" }) names.Add(name);
            var lines = new List<string>();
            lines.Add("# Effective visual settings for the game process; <unset> differs from 0.");
            lines.Add("started=" + DateTimeOffset.Now.ToString("o"));
            using (var hash = SHA256.Create()) using (var stream = Assembly.GetExecutingAssembly().GetManifestResourceStream("DeadSpaceBootstrap.VisualCompat.ini"))
                lines.Add("visual_profile_sha256=" + BitConverter.ToString(hash.ComputeHash(stream)).Replace("-", ""));
            using (var hash = SHA256.Create()) using (var file = File.OpenRead(info.FileName))
                lines.Add("exe_sha256=" + BitConverter.ToString(hash.ComputeHash(file)).Replace("-", ""));
            foreach (string name in names)
                lines.Add(name + "=" + (info.EnvironmentVariables[name] ?? "<unset>"));
            File.WriteAllLines(Path.Combine(directory,"DeadSpace-visual-effective.ini"),lines.ToArray(),new UTF8Encoding(false));
        }
    }
}
