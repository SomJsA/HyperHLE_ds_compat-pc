using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Security.Cryptography;
using System.Text;

namespace DeadSpaceBootstrap
{
    internal sealed class SaveValue
    {
        internal string Path, Value, Group;
        internal int Offset, Type, Width;
        internal bool Editable;
    }

    // Strict reader and length-preserving editor. Unknown state, actor lists,
    // pointers, list lengths and string tables are never rewritten.
    internal sealed class SaveDocument
    {
        internal readonly string FilePath;
        internal readonly Dictionary<string, SaveValue> Values = new Dictionary<string, SaveValue>(StringComparer.Ordinal);
        internal readonly List<string> Strings = new List<string>();
        private readonly byte[] original;
        private readonly Dictionary<string, int[]> sections = new Dictionary<string, int[]>();
        private readonly int data, dataLength, ohdr, ohdrLength;
        internal string Sha256 { get { return Digest(original); } }

        internal SaveDocument(string file) : this(Path.GetFullPath(file), File.ReadAllBytes(file)) { }
        internal SaveDocument(string file, byte[] bytes)
        {
            FilePath = file; original = bytes;
            Require(bytes.Length >= 8 && bytes.Length <= 16 * 1024 * 1024 && Encoding.ASCII.GetString(bytes, 0, 4) == "SBIN" && U32(4) == 2, "不是支持的 SBIN v2 存档");
            int cursor = 8;
            while (cursor < bytes.Length)
            {
                Require(cursor + 12 <= bytes.Length, "区段头越界");
                string tag = Encoding.ASCII.GetString(bytes, cursor, 4);
                uint count = U32(cursor + 4);
                Require(count <= bytes.Length - cursor - 12 && !sections.ContainsKey(tag), "区段长度错误或重复区段");
                sections.Add(tag, new[] { cursor, cursor + 12, (int)count });
                Require(Hash(bytes, cursor + 12, (int)count) == U32(cursor + 8), "区段校验失败：" + tag);
                cursor = checked((cursor + 12 + (int)count + 3) & ~3);
            }
            Require(cursor == bytes.Length, "文件尾部越界");
            int[] ds = Section("DATA"), os = Section("OHDR"), ss = Section("STRS");
            data = ds[1]; dataLength = ds[2]; ohdr = os[1]; ohdrLength = os[2];
            Require(ohdrLength % 4 == 0 && ss[2] >= 4, "对象或字符串表无效");
            int at = ss[1] + 4, end = ss[1] + ss[2]; uint names = U32(ss[1]);
            Require(names < 65536, "字符串表过大");
            for (uint i = 0; i < names; i++)
            {
                Require(at + 4 <= end, "字符串头越界"); uint length = U32(at); at += 4;
                Require(length <= end - at, "字符串长度越界");
                Strings.Add(new UTF8Encoding(false, true).GetString(bytes, at, (int)length)); at += (int)length;
            }
            Require(at == end, "字符串表尾部异常");
            Require(ohdrLength >= 4, "缺少根对象");
            uint rootEntry = U32(ohdr);
            Require((rootEntry & 7) == 1 && (rootEntry >> 3) <= dataLength - 4, "根对象引用无效");
            // OHDR[0] is authoritative. Survival/Achievements roots are often
            // at the END of DATA. Root-relative offsets only appear absolute
            // when that root happens to be at DATA+4.
            WalkObject(data + (int)(rootEntry >> 3), "", new HashSet<int>(), 0);
        }

        private int[] Section(string tag) { Require(sections.ContainsKey(tag), "缺少区段：" + tag); return sections[tag]; }
        private static void Require(bool condition, string message) { if (!condition) throw new InvalidDataException(message); }
        private ushort U16(int at) { Require(at >= 0 && at <= original.Length - 2, "u16 越界"); return BitConverter.ToUInt16(original, at); }
        private uint U32(int at) { Require(at >= 0 && at <= original.Length - 4, "u32 越界"); return BitConverter.ToUInt32(original, at); }
        private string Name(uint index) { Require(index > 0 && index <= Strings.Count, "名称索引越界"); return Strings[(int)index - 1]; }
        private static int Width(int type) { return type == 9 || type == 13 ? 2 : type == 5 || type == 6 || type == 10 || type == 15 ? 4 : 0; }

        private void WalkObject(int start, string prefix, HashSet<int> active, int depth)
        {
            Require(depth <= 24 && active.Add(start), "对象引用循环或过深");
            try
            {
                Require(start >= data && start + 4 <= data + dataLength, "对象头越界");
                uint header = U32(start); int size = (int)(header >> 16), count = (int)(header & 65535);
                Require(size >= 4 && size <= data + dataLength - start, "对象长度越界");
                int cursor = start + 4;
                for (int index = 0; index < count; index++)
                {
                    Require(cursor + 8 <= start + size, "字段头越界：" + prefix);
                    string name = Name(U16(cursor)); int type = U16(cursor + 2), width = Width(type);
                    Require(width != 0, "未知字段类型：" + prefix + name);
                    uint offset = U32(cursor + 4); Require(offset <= dataLength, "字段偏移越界");
                    int value = checked(start + (int)offset);
                    // Some final boolean objects declare 13 bytes: only the
                    // actual low byte is owned, the following CD is padding.
                    int owned = Math.Min(width, start + size - value);
                    Require(value >= cursor + 8 && value <= cursor + 10 && owned >= (type == 9 ? 1 : width), "字段值越界：" + prefix + name);
                    string path = prefix + name;
                    if (type == 15) WalkReference(U32(value), path, active, depth + 1);
                    else Add(path, value, type, owned);
                    cursor = value + width;
                }
            }
            finally { active.Remove(start); }
        }

        private void WalkReference(uint reference, string path, HashSet<int> active, int depth)
        {
            Require(depth <= 24, "引用过深：" + path);
            Require(reference < ohdrLength / 4, "对象引用越界：" + path);
            uint entry = U32(ohdr + checked((int)reference * 4)); int flag = (int)(entry & 7), offset = (int)(entry >> 3);
            Require(offset <= dataLength - 4, "对象地址越界：" + path);
            int start = data + offset;
            if (flag == 1) { WalkObject(start, path + "/", active, depth); return; }
            Require(flag == 2 && start + 8 <= data + dataLength, "未知引用类型：" + path);
            int poolEnd = data + dataLength;
            for (int index = 0; index < ohdrLength / 4; index++)
            {
                int other = data + (int)(U32(ohdr + index * 4) >> 3);
                if (other > start && other < poolEnd) poolEnd = other;
            }
            int type = (int)U32(start);
            if (type == 16 && path == "player/inventory/items")
            {
                uint count = U32(start + 4); Require(count <= 1024 && count <= (poolEnd - start - 8) / 8, "背包数组越界");
                for (int index = 0; index < count; index++)
                {
                    int item = start + 8 + index * 8;
                    Add(path + "/" + index + "/type", item, 13, 2);
                    Add(path + "/" + index + "/count", item + 4, 5, 4);
                }
            }
            else if (type == 15 || Width(type) != 0)
            {
                int width = Width(type); uint count = U32(start + 4);
                Require(count <= 65535 && count <= (poolEnd - start - 8) / width, "列表越界：" + path);
                Require(active.Add(start), "列表引用循环");
                try
                {
                    for (int index = 0; index < count; index++)
                    {
                        int value = start + 8 + index * width;
                        if (type == 15) WalkReference(U32(value), path + "/" + index, active, depth + 1);
                        else Add(path + "/" + index, value, type, width);
                    }
                }
                finally { active.Remove(start); }
            }
            else
            {
                // Pool type 0x0D is an array of string IDs, NOT one scalar ID.
                // Do not decode its count (e.g. 2) as an item or DLC bitmask.
                Values.Add(path, new SaveValue { Path = path, Offset = start, Type = type, Width = 0,
                    Value = "保留的列表 / 值池 (0x" + type.ToString("x") + ")", Group = "进度（只读）", Editable = false });
            }
            // Other pools (actors, DLC applied-ID lists, achievement arrays)
            // remain byte-exact and outside the editor's mutable schema.
        }

        private void Add(string path, int offset, int type, int width)
        {
            Require(!Values.ContainsKey(path), "重复字段路径：" + path);
            string value;
            if (type == 13) value = Name(U16(offset));
            else if (type == 10) value = BitConverter.ToSingle(original, offset).ToString("R", CultureInfo.InvariantCulture);
            else if (type == 9) value = original[offset].ToString(CultureInfo.InvariantCulture);
            else value = U32(offset).ToString(CultureInfo.InvariantCulture);
            string group = path.StartsWith("player/inventory/items/") ? "背包" : path.StartsWith("player/weapons/") ? "武器"
                : path.StartsWith("player/") ? "资源与升级" : path.StartsWith("dlc_modules/") || path.IndexOf('/') < 0 ? "共享设置" : "进度（只读）";
            bool editable = path == "player/hp" || path == "player/inventory/credits" || path == "player/inventory/nodes"
                || path == "player/upgrade_level_rig" || path == "player/upgrade_level_health" || path == "player/upgrade_level_o2"
                || path == "player/telekinesis_available" || path == "player/plasmasaw_available"
                || (path.StartsWith("player/weapons/") && (path.EndsWith("/clip") || path.EndsWith("/available") || path.EndsWith("/altmode")))
                || path.StartsWith("player/inventory/items/")
                || (path.StartsWith("dlc_modules/") && path.EndsWith("/unlocked"))
                || path == "misc/gameCompleted" || path == "misc/plus" || path == "plus_unlocked"
                || path == "auto_reload" || path == "subtitles_enabled" || path == "tutorials_enabled" || path == "y_flipped"
                || path == "sfx_volume" || path == "music_volume" || path == "brightness" || path == "sensitivity"
                || (path.StartsWith("levelCompletes/") && path.EndsWith("/complete"))
                || (path.StartsWith("achievementCompletes/") && path.EndsWith("/complete"));
            if (path.StartsWith("levelCompletes/") || path.StartsWith("achievementCompletes/") || path == "misc/gameCompleted" || path == "misc/plus") group = "章节与周目";
            Values.Add(path, new SaveValue { Path = path, Offset = offset, Type = type, Width = width, Value = value, Group = group, Editable = editable });
        }

        private byte[] Encode(SaveValue field, string value)
        {
            if (!field.Editable) throw new InvalidDataException("字段只读：" + field.Path);
            if (field.Type == 9)
            { Require(value == "0" || value == "1", "布尔值只能为 0 或 1"); return new[] { (byte)(value == "1" ? 1 : 0) }; }
            if (field.Type == 13)
            {
                int index = Strings.IndexOf(value);
                Require(index >= 0 && IsItemName(value), "物品类型必须存在于本档字符串表：" + value);
                return BitConverter.GetBytes(checked((ushort)(index + 1)));
            }
            if (field.Type == 10)
            {
                float number; Require(Single.TryParse(value, NumberStyles.Float, CultureInfo.InvariantCulture, out number)
                    && !Single.IsNaN(number) && !Single.IsInfinity(number) && number >= 0 && number <= 1000000, "浮点数值无效");
                if (field.Path == "sfx_volume" || field.Path == "music_volume" || field.Path == "brightness") Require(number <= 1, "音量/亮度范围为 0–1");
                return BitConverter.GetBytes(number);
            }
            uint integer; Require(UInt32.TryParse(value, NumberStyles.None, CultureInfo.InvariantCulture, out integer) && integer <= 1000000000, "请输入 0–1000000000 的整数");
            if (field.Path.IndexOf("upgrade_level_") >= 0) Require(integer <= 4, "升级等级范围为 0–4");
            return BitConverter.GetBytes(integer);
        }
        internal static bool IsItemName(string value)
        { return value.StartsWith("AMMO_", StringComparison.Ordinal) || value.StartsWith("HEALTH_", StringComparison.Ordinal) || value.StartsWith("OXYGEN_", StringComparison.Ordinal) || value == "PENG" || value == "STASIS_PACK"; }

        internal byte[] EditBytes(IDictionary<string, string> changes)
        {
            byte[] edited = (byte[])original.Clone(); var allowed = new HashSet<int>();
            foreach (var change in changes)
            {
                SaveValue field; Require(Values.TryGetValue(change.Key, out field), "找不到字段：" + change.Key);
                byte[] bytes = Encode(field, change.Value.Trim());
                Require(bytes.Length <= field.Width, "写入超出字段长度");
                for (int i = 0; i < bytes.Length; i++) { edited[field.Offset + i] = bytes[i]; allowed.Add(field.Offset + i); }
            }
            int header = Section("DATA")[0];
            Array.Copy(BitConverter.GetBytes(Hash(edited, data, dataLength)), 0, edited, header + 8, 4);
            for (int i = 0; i < 4; i++) allowed.Add(header + 8 + i);
            for (int i = 0; i < edited.Length; i++) Require(edited[i] == original[i] || allowed.Contains(i), "修改越过字段边界");
            var reread = new SaveDocument(FilePath, edited);
            foreach (var change in changes)
            {
                SaveValue field = Values[change.Key]; byte[] expected = Encode(field, change.Value.Trim());
                SaveValue actual = reread.Values[change.Key];
                for (int i = 0; i < expected.Length; i++) Require(edited[actual.Offset + i] == expected[i], "写后字段校验失败");
            }
            // The selected weapon must still be available. Never manufacture
            // or change the current-weapon flags as a side effect of editing.
            foreach (var field in reread.Values.Values)
                if (field.Path.StartsWith("player/weapons/") && field.Path.EndsWith("/current") && field.Value == "1")
                {
                    SaveValue available;
                    if (reread.Values.TryGetValue(field.Path.Substring(0, field.Path.Length - 7) + "available", out available))
                        Require(available.Value == "1", "不能锁定当前装备的武器");
                }
            return edited;
        }

        internal string Preview(IDictionary<string, string> changes)
        {
            byte[] edited = EditBytes(changes); var lines = new List<string>(); int difference = 0;
            foreach (var change in changes) if (Values[change.Key].Value != change.Value.Trim())
                lines.Add(change.Key + ": " + Values[change.Key].Value + " → " + change.Value.Trim());
            for (int i = 0; i < edited.Length; i++) if (edited[i] != original[i]) difference++;
            lines.Add("长度保持 " + edited.Length + " 字节；变化 " + difference + " 字节（含 DATA 校验）。");
            return String.Join("\r\n", lines.ToArray());
        }

        internal string Write(IDictionary<string, string> changes)
        {
            SaveStorage.RequireStopped();
            SaveStorage.RequireNoLinks(FilePath);
            Require(Digest(File.ReadAllBytes(FilePath)) == Sha256, "源存档已变化，请重新读取");
            byte[] edited = EditBytes(changes);
            Require(Digest(edited) != Sha256, "没有实际修改");
            string directory = Path.GetDirectoryName(FilePath), token = DateTime.UtcNow.ToString("yyyyMMdd-HHmmss") + "-" + Guid.NewGuid().ToString("N");
            string backup = Path.Combine(directory, ".save-editor-backups", token);
            SaveStorage.RequireNoLinks(backup);
            // Preserve all three slots and shared settings as one recovery set.
            Directory.CreateDirectory(backup);
            var manifests = new List<string> { "# SBIN editor backup; original files before write" };
            foreach (string sibling in Directory.GetFiles(directory))
            {
                string name = Path.GetFileName(sibling);
                if (!name.EndsWith(".sb", StringComparison.OrdinalIgnoreCase) && name != "last_version.txt") continue;
                SaveStorage.RequireNoLinks(sibling);
                byte[] bytes = File.ReadAllBytes(sibling); string copy = Path.Combine(backup, name);
                File.WriteAllBytes(copy, bytes); Require(Digest(File.ReadAllBytes(copy)) == Digest(bytes), "备份校验失败");
                manifests.Add(Digest(bytes) + "  " + name);
            }
            File.WriteAllLines(Path.Combine(backup, "original.sha256"), manifests.ToArray(), new UTF8Encoding(false));
            File.WriteAllText(Path.Combine(backup, "changes.txt"), Preview(changes), new UTF8Encoding(false));
            string temporary = FilePath + "." + token + ".tmp";
            try
            {
                File.WriteAllBytes(temporary, edited); new SaveDocument(temporary);
                SaveStorage.RequireStopped();
                SaveStorage.RequireNoLinks(FilePath);
                Require(Digest(File.ReadAllBytes(FilePath)) == Sha256, "备份期间源文件变化，已取消写入");
                File.Replace(temporary, FilePath, null);
                Require(Digest(File.ReadAllBytes(FilePath)) == Digest(edited), "写后文件校验失败，备份保留于 " + backup);
                File.WriteAllText(Path.Combine(backup, "result.sha256"), Digest(edited) + "  " + Path.GetFileName(FilePath), new UTF8Encoding(false));
                return backup;
            }
            finally { if (File.Exists(temporary)) File.Delete(temporary); }
        }

        private static string Digest(byte[] bytes) { using (var hash = SHA256.Create()) return BitConverter.ToString(hash.ComputeHash(bytes)).Replace("-", ""); }
        private static uint Hash(byte[] bytes, int offset, int count) { uint value = 2166136261; for (int i = offset; i < offset + count; i++) value = unchecked(value * 16777619) ^ bytes[i]; return value; }
    }
}
