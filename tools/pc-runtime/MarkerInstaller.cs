using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Security.Cryptography;
using System.Web.Script.Serialization;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed class VandalMarkerEntry
    {
        internal string Path;
        internal long Size;
        internal string Sha256;
        internal string Role;
    }

    internal sealed class VandalMarkerPackage
    {
        internal string Path;
        internal string MarkerId;
        internal string MarkerVersion;
        internal string BasePath;
        internal HashSet<string> TextureTiers;
        internal List<VandalMarkerEntry> Entries;
    }

    internal static class VandalMarker
    {
        private const string ManifestName = "vandal.json";
        private const string Kind = "deadspace-vandal-marker";
        private const int Schema = 1;

        internal static VandalMarkerPackage Read(string path)
        {
            using (var stream = new FileStream(
                path, FileMode.Open, FileAccess.Read, FileShare.Read))
            using (var archive = new ZipArchive(
                stream, ZipArchiveMode.Read, false))
            {
                var names = archive.Entries.Select(entry => entry.FullName).ToList();
                if (names.Count != names.Distinct(StringComparer.Ordinal).Count())
                    throw new InvalidDataException("marker 内存在重复文件。");
                var manifestEntry = archive.GetEntry(ManifestName);
                if (manifestEntry == null)
                    throw new InvalidDataException("marker 缺少 vandal.json。");
                string manifestText;
                using (var reader = new StreamReader(manifestEntry.Open()))
                    manifestText = reader.ReadToEnd();
                var serializer = new JavaScriptSerializer
                {
                    MaxJsonLength = 16 * 1024 * 1024,
                    RecursionLimit = 32
                };
                var root = serializer.DeserializeObject(manifestText)
                    as Dictionary<string, object>;
                if (root == null ||
                    Number(root, "schema") != Schema ||
                    Text(root, "kind") != Kind)
                    throw new InvalidDataException("不支持的 marker 版本。");
                string markerId = Text(root, "marker_id").ToLowerInvariant();
                if (markerId.Length != 64 ||
                    markerId.Any(ch => !"0123456789abcdef".Contains(ch)))
                    throw new InvalidDataException("marker ID 无效。");
                var game = Object(root, "game");
                string basePath = SafeRelative(Text(game, "ipa_path"));
                var tiersObject = Object(root, "texture_tiers");
                var tiers = new HashSet<string>(
                    tiersObject.Keys.Where(
                        key => key == "2" || key == "4" || key == "6"),
                    StringComparer.Ordinal);
                if (tiers.Count == 0)
                    throw new InvalidDataException("marker 没有可安装的纹理档。");
                object[] entriesObject = root.ContainsKey("entries")
                    ? root["entries"] as object[]
                    : null;
                if (entriesObject == null)
                    throw new InvalidDataException("marker entries 无效。");
                var entries = new List<VandalMarkerEntry>();
                foreach (object value in entriesObject)
                {
                    var row = value as Dictionary<string, object>;
                    if (row == null)
                        throw new InvalidDataException("marker entry 无效。");
                    var entry = new VandalMarkerEntry
                    {
                        Path = SafeRelative(Text(row, "path")),
                        Size = Number(row, "size"),
                        Sha256 = Text(row, "sha256").ToLowerInvariant(),
                        Role = Text(row, "role")
                    };
                    if (entry.Size < 0 || entry.Sha256.Length != 64)
                        throw new InvalidDataException("marker entry 元数据无效。");
                    entries.Add(entry);
                }
                var baseEntry = entries.SingleOrDefault(
                    entry => entry.Path == basePath &&
                    entry.Role == "ipa");
                if (baseEntry == null)
                    throw new InvalidDataException("marker 基座数据路径无效。");
                foreach (var entry in entries)
                {
                    if (entry.Role != "ipa" &&
                        entry.Role != "texture")
                        throw new InvalidDataException(
                            "marker 含有不支持的内容角色。");
                }
                return new VandalMarkerPackage
                {
                    Path = System.IO.Path.GetFullPath(path),
                    MarkerId = markerId,
                    MarkerVersion = Text(root, "marker_version"),
                    BasePath = basePath,
                    TextureTiers = tiers,
                    Entries = entries
                };
            }
        }

        internal static string Install(
            VandalMarkerPackage marker,
            string dataRoot,
            string tier)
        {
            if (!marker.TextureTiers.Contains(tier))
                throw new InvalidDataException(
                    "marker 不包含 " + tier + "× 纹理。");
            string contentRoot = System.IO.Path.Combine(dataRoot, "marker-content");
            Directory.CreateDirectory(contentRoot);
            string destination = System.IO.Path.Combine(
                contentRoot, marker.MarkerId);
            string installationPath = System.IO.Path.Combine(
                destination, "installation.json");
            string baseFile = System.IO.Path.Combine(
                destination,
                marker.BasePath.Replace('/', System.IO.Path.DirectorySeparatorChar));
            if (File.Exists(installationPath) && File.Exists(baseFile))
            {
                string tierPack = System.IO.Path.Combine(
                    destination, "payload", "texture-packs", tier);
                if (File.Exists(System.IO.Path.Combine(tierPack, "PACK.json")))
                    return destination;
            }
            if (Directory.Exists(destination))
                throw new InvalidDataException(
                    "已存在的 marker 内容不完整或缺少安装记录。");

            string prefix = "payload/texture-packs/" + tier + "/";
            var selected = marker.Entries.Where(
                entry => entry.Path == marker.BasePath ||
                entry.Path.StartsWith(prefix, StringComparison.Ordinal)).ToList();
            string temporary = System.IO.Path.Combine(
                contentRoot,
                ".install-" + marker.MarkerId + "-" + Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(temporary);
            try
            {
                using (var stream = new FileStream(
                    marker.Path,
                    FileMode.Open,
                    FileAccess.Read,
                    FileShare.Read))
                using (var archive = new ZipArchive(
                    stream, ZipArchiveMode.Read, false))
                {
                    var manifest = archive.GetEntry(ManifestName);
                    string manifestTarget = System.IO.Path.Combine(
                        temporary, ManifestName);
                    using (var source = manifest.Open())
                    using (var target = new FileStream(
                        manifestTarget,
                        FileMode.CreateNew,
                        FileAccess.Write,
                        FileShare.None))
                        source.CopyTo(target, 4 * 1024 * 1024);
                    var files = new List<object>();
                    foreach (var entry in selected)
                    {
                        var zipEntry = archive.GetEntry(entry.Path);
                        if (zipEntry == null || zipEntry.Length != entry.Size)
                            throw new InvalidDataException(
                                "marker 文件大小不匹配：" + entry.Path);
                        string targetPath = SafeTarget(temporary, entry.Path);
                        Directory.CreateDirectory(
                            System.IO.Path.GetDirectoryName(targetPath));
                        string actual;
                        using (var source = zipEntry.Open())
                        using (var target = new FileStream(
                            targetPath,
                            FileMode.CreateNew,
                            FileAccess.Write,
                            FileShare.None))
                        using (var digest = SHA256.Create())
                        {
                            byte[] block = new byte[4 * 1024 * 1024];
                            int count;
                            while ((count = source.Read(block, 0, block.Length)) > 0)
                            {
                                digest.TransformBlock(block, 0, count, null, 0);
                                target.Write(block, 0, count);
                            }
                            digest.TransformFinalBlock(new byte[0], 0, 0);
                            actual = BitConverter.ToString(digest.Hash)
                                .Replace("-", "")
                                .ToLowerInvariant();
                        }
                        if (actual != entry.Sha256)
                            throw new InvalidDataException(
                                "marker SHA-256 不匹配：" + entry.Path);
                        files.Add(new
                        {
                            path = entry.Path,
                            size = entry.Size,
                            sha256 = actual
                        });
                    }
                    var installation = new
                    {
                        schema = 2,
                        marker_id = marker.MarkerId,
                        marker_version = marker.MarkerVersion,
                        source_marker = marker.Path,
                        installed_utc = DateTime.UtcNow.ToString("o"),
                        base_data = new
                        {
                            installed = true,
                            path = marker.BasePath
                        },
                        texture_tiers = new[] { tier },
                        files,
                        marker_file_modified = false
                    };
                    File.WriteAllText(
                        System.IO.Path.Combine(temporary, "installation.json"),
                        new JavaScriptSerializer().Serialize(installation));
                }
                Directory.Move(temporary, destination);
                return destination;
            }
            finally
            {
                if (Directory.Exists(temporary))
                    Directory.Delete(temporary, true);
            }
        }

        private static string SafeRelative(string value)
        {
            if (String.IsNullOrWhiteSpace(value) ||
                value.StartsWith("/") ||
                value.StartsWith("\\") ||
                value.Contains(":") ||
                value.Contains("\\") ||
                value.Split('/').Any(part => part == ".." || part.Length == 0))
                throw new InvalidDataException("marker 路径不安全：" + value);
            return value;
        }

        private static string SafeTarget(string root, string relative)
        {
            string fullRoot = System.IO.Path.GetFullPath(root)
                .TrimEnd(System.IO.Path.DirectorySeparatorChar) +
                System.IO.Path.DirectorySeparatorChar;
            string target = System.IO.Path.GetFullPath(
                System.IO.Path.Combine(
                    fullRoot,
                    relative.Replace('/', System.IO.Path.DirectorySeparatorChar)));
            if (!target.StartsWith(fullRoot, StringComparison.OrdinalIgnoreCase))
                throw new InvalidDataException("marker 解包路径越界。");
            return target;
        }

        private static Dictionary<string, object> Object(
            Dictionary<string, object> source,
            string key)
        {
            object value;
            if (!source.TryGetValue(key, out value))
                throw new InvalidDataException("marker 字段无效：" + key);
            var result = value as Dictionary<string, object>;
            if (result == null)
                throw new InvalidDataException("marker 字段无效：" + key);
            return result;
        }

        private static string Text(
            Dictionary<string, object> source,
            string key)
        {
            object value;
            return source.TryGetValue(key, out value) ? Convert.ToString(value) : "";
        }

        private static long Number(
            Dictionary<string, object> source,
            string key)
        {
            object value;
            if (!source.TryGetValue(key, out value))
                return -1;
            return Convert.ToInt64(value,
                System.Globalization.CultureInfo.InvariantCulture);
        }
    }

    internal sealed partial class LauncherForm
    {
        private readonly Label markerStatus = new Label();
        private readonly Button importMarker = new Button();
        private string installedMarkerRoot;

        private void BuildMarkerControls(Control page)
        {
            importMarker.Text = "导入 vandal.marker";
            importMarker.SetBounds(200, 182, 190, 38);
            importMarker.Click += delegate { ChooseMarker(); };
            page.Controls.Add(importMarker);
            markerStatus.SetBounds(30, 222, 680, 42);
            markerStatus.ForeColor = Muted;
            page.Controls.Add(markerStatus);
            UpdateMarkerStatus();
        }

        private void EnsureMarkerReady(bool userRequested)
        {
            string installed = FindInstalledMarker();
            if (installed != null)
            {
                ApplyInstalledMarker(installed);
                return;
            }
            string nearby = FindNearbyMarker();
            if (nearby != null)
            {
                ImportMarker(nearby, true);
                return;
            }
            string ipa = ipaPath.Text.Trim();
            if (ipa.Length > 0 && File.Exists(ipa))
            {
                UpdateMarkerStatus();
                return;
            }
            if (userRequested ||
                MessageBox.Show(
                    this,
                    "需要资源文件 vandal.marker。\n\n请选择 marker，启动器会校验并安装，之后不再修改 marker。",
                    "需要游戏资源",
                    MessageBoxButtons.OKCancel,
                    MessageBoxIcon.Information) == DialogResult.OK)
                ChooseMarker();
        }

        private void ChooseMarker()
        {
            using (var dialog = new OpenFileDialog())
            {
                dialog.Title = "选择 vandal.marker";
                dialog.Filter = "Vandal marker (*.marker)|*.marker|所有文件 (*.*)|*.*";
                if (dialog.ShowDialog(this) == DialogResult.OK)
                    ImportMarker(dialog.FileName, false);
            }
        }

        private void ImportMarker(string markerPath, bool automatic)
        {
            try
            {
                Cursor previous = Cursor;
                Cursor = Cursors.WaitCursor;
                importMarker.Enabled = false;
                try
                {
                    string tier = TextureScale();
                    var marker = VandalMarker.Read(markerPath);
                    if (!marker.TextureTiers.Contains(tier))
                        tier = marker.TextureTiers.Contains("4")
                            ? "4"
                            : marker.TextureTiers.OrderBy(value => value).First();
                    status.Text = "正在校验并安装 marker（" + tier + "× 纹理），请稍候…";
                    Application.DoEvents();
                    string root = VandalMarker.Install(
                        marker,
                        SelectedDataRoot(),
                        tier);
                    ApplyInstalledMarker(root);
                    SaveSettings();
                    status.Text = "marker 已安装；后续运行不再依赖 marker 文件。";
                }
                finally
                {
                    importMarker.Enabled = true;
                    Cursor = previous;
                }
            }
            catch (Exception error)
            {
                status.Text = "marker 安装失败：" + error.Message;
                if (!automatic)
                    MessageBox.Show(
                        this,
                        "无法安装 marker：\n" + error.Message,
                        "marker",
                        MessageBoxButtons.OK,
                        MessageBoxIcon.Error);
            }
        }

        private void ApplyInstalledMarker(string root)
        {
            installedMarkerRoot = System.IO.Path.GetFullPath(root);
            string ipa = System.IO.Path.Combine(
                installedMarkerRoot,
                "payload",
                "DeadSpace.ipa");
            if (!File.Exists(ipa))
                throw new InvalidDataException("已安装 marker 缺少基座数据。");
            ipaPath.Text = ipa;
            foreach (string tier in new[] { "2", "4", "6" })
            {
                string path = System.IO.Path.Combine(
                    installedMarkerRoot,
                    "payload",
                    "texture-packs",
                    tier);
                textureRoots[tier] = path;
            }
            hdTextures.Checked = true;
            UpdateTextureDescription();
            UpdateMarkerStatus();
        }

        private string FindInstalledMarker()
        {
            try
            {
                string content = System.IO.Path.Combine(
                    SelectedDataRoot(), "marker-content");
                if (!Directory.Exists(content))
                    return null;
                foreach (string directory in Directory.GetDirectories(content)
                    .OrderByDescending(path => Directory.GetLastWriteTimeUtc(path)))
                {
                    string install = System.IO.Path.Combine(
                        directory, "installation.json");
                    if (!File.Exists(install))
                        continue;
                    string baseFile = System.IO.Path.Combine(
                        directory, "payload", "DeadSpace.ipa");
                    if (File.Exists(baseFile))
                        return directory;
                }
            }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            return null;
        }

        private string FindNearbyMarker()
        {
            foreach (string path in new[]
            {
                System.IO.Path.Combine(baseDir, "vandal.marker"),
                System.IO.Path.Combine(documentsDirectory, "DeadSpaceLegacy", "vandal.marker")
            })
                if (File.Exists(path))
                    return path;
            return null;
        }

        private void UpdateMarkerStatus()
        {
            if (!String.IsNullOrEmpty(installedMarkerRoot))
                markerStatus.Text =
                    "已安装资源：" + installedMarkerRoot +
                    "\r\nmarker 不会被修改；切换分辨率不需要重新导入。";
            else if (ipaPath.Text.Trim().Length > 0)
                markerStatus.Text = "直接 IPA 模式；不会安装或修改 marker。";
            else
                markerStatus.Text =
                    "未安装游戏资源。可点击“导入 vandal.marker”，或把 marker 放在启动器同目录。";
        }
    }
}
