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
            return InstallSelected(marker, dataRoot, new[] { tier });
        }

        internal static string[] ParseTiers(string value)
        {
            if (value == "none") return new string[0];
            var tiers = value.Split(',').Select(t => t.Trim()).Distinct().OrderBy(t => t).ToArray();
            if (tiers.Length == 0 || tiers.Any(t => t != "2" && t != "4" && t != "6"))
                throw new ArgumentException("纹理档位必须是 2、4、6 的组合，或 none（仅原版）。");
            return tiers;
        }

        internal static string InstallSelected(
            VandalMarkerPackage marker, string dataRoot, IEnumerable<string> selectedTiers)
        {
            var tiers = selectedTiers.Distinct().OrderBy(t => t).ToArray();
            // Validate the complete selection before writing any content.
            foreach (string tier in tiers)
                if (!marker.TextureTiers.Contains(tier))
                    throw new InvalidDataException("marker 不包含 " + tier + "× 纹理。");
            string contentRoot = System.IO.Path.Combine(dataRoot, "marker-content");
            Directory.CreateDirectory(contentRoot);
            string destination = System.IO.Path.Combine(
                contentRoot, marker.MarkerId);
            string installationPath = System.IO.Path.Combine(
                destination, "installation.json");
            string baseFile = System.IO.Path.Combine(
                destination,
                marker.BasePath.Replace('/', System.IO.Path.DirectorySeparatorChar));
            if (Directory.Exists(destination))
            {
                if (!File.Exists(installationPath) || !File.Exists(baseFile))
                    throw new InvalidDataException(
                        "已存在的 marker 内容不完整或缺少安装记录。");
                if (HashFile(baseFile) != marker.Entries.Single(e => e.Path == marker.BasePath && e.Role == "ipa").Sha256)
                    throw new InvalidDataException("Installed marker IPA fingerprint mismatch.");
                foreach (string tier in tiers)
                {
                    string tierPack = System.IO.Path.Combine(destination, "payload", "texture-packs", tier);
                    if (!File.Exists(System.IO.Path.Combine(tierPack, "PACK.json")))
                        AddTextureTier(marker, destination, tier);
                }
                return destination;
            }

            var selected = marker.Entries.Where(
                entry => entry.Path == marker.BasePath ||
                tiers.Any(tier => entry.Path.StartsWith(
                    "payload/texture-packs/" + tier + "/", StringComparison.Ordinal))).ToList();
            string temporary = System.IO.Path.Combine(
                contentRoot,
                ".i-" + marker.MarkerId.Substring(0, 12) + "-"
                    + Guid.NewGuid().ToString("N").Substring(0, 12));
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
                    }
                }
                WriteInstallationRecord(temporary, marker);
                Directory.Move(temporary, destination);
                return destination;
            }
            finally
            {
                if (Directory.Exists(temporary))
                    Directory.Delete(temporary, true);
            }
        }

        internal static string InstallIpa(string ipaPath, string dataRoot)
        {
            string source = System.IO.Path.GetFullPath(ipaPath);
            if (!File.Exists(source))
                throw new FileNotFoundException("IPA 不存在。", source);
            string digest = HashFile(source);
            string contentRoot = System.IO.Path.Combine(dataRoot, "marker-content");
            Directory.CreateDirectory(contentRoot);
            string markerId = "direct-" + digest;
            string destination = System.IO.Path.Combine(contentRoot, markerId);
            string installed = System.IO.Path.Combine(
                destination, "payload", "DeadSpace.ipa");
            string installation = System.IO.Path.Combine(
                destination, "installation.json");
            if (Directory.Exists(destination))
            {
                if (!File.Exists(installed) || !File.Exists(installation))
                    throw new InvalidDataException("直接 IPA 安装目录不完整。");
                if (HashFile(installed) != digest)
                    throw new InvalidDataException(
                        "已安装 IPA 的 SHA-256 与当前安装记录不一致。");
                return destination;
            }
            string temporary = System.IO.Path.Combine(
                contentRoot, ".d-" +
                    Guid.NewGuid().ToString("N").Substring(0, 12));
            Directory.CreateDirectory(System.IO.Path.Combine(temporary, "payload"));
            try
            {
                string target = System.IO.Path.Combine(
                    temporary, "payload", "DeadSpace.ipa");
                File.Copy(source, target, false);
                if (HashFile(target) != digest)
                    throw new InvalidDataException("复制后的 IPA SHA-256 不匹配。");
                WriteDirectIpaRecord(temporary, source, digest);
                Directory.Move(temporary, destination);
                return destination;
            }
            finally
            {
                if (Directory.Exists(temporary))
                    Directory.Delete(temporary, true);
            }
        }

        private static string AddTextureTier(
            VandalMarkerPackage marker,
            string destination,
            string tier)
        {
            if (!File.Exists(marker.Path))
                throw new FileNotFoundException(
                    "安装其他纹理档位需要重新选择原始 marker。", marker.Path);
            string targetTier = System.IO.Path.Combine(
                destination, "payload", "texture-packs", tier);
            if (Directory.Exists(targetTier))
                throw new InvalidDataException(
                    "目标纹理档位目录已存在但缺少 PACK.json，请检查安装目录。");
            string prefix = "payload/texture-packs/" + tier + "/";
            var selected = marker.Entries.Where(
                entry => entry.Path.StartsWith(prefix, StringComparison.Ordinal))
                .ToList();
            if (selected.Count == 0)
                throw new InvalidDataException("marker 的纹理档位没有文件。");
            string contentRoot = System.IO.Path.GetDirectoryName(destination);
            string temporary = System.IO.Path.Combine(
                contentRoot,
                ".t-" + marker.MarkerId.Substring(0, 12) + "-" + tier + "-"
                    + Guid.NewGuid().ToString("N").Substring(0, 12));
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
                    foreach (var entry in selected)
                    {
                        var zipEntry = archive.GetEntry(entry.Path);
                        if (zipEntry == null || zipEntry.Length != entry.Size)
                            throw new InvalidDataException(
                                "marker 文件大小不匹配：" + entry.Path);
                        string extracted = SafeTarget(temporary, entry.Path);
                        Directory.CreateDirectory(
                            System.IO.Path.GetDirectoryName(extracted));
                        string actual;
                        using (var source = zipEntry.Open())
                        using (var target = new FileStream(
                            extracted,
                            FileMode.CreateNew,
                            FileAccess.Write,
                            FileShare.None))
                        using (var digest = SHA256.Create())
                        {
                            byte[] block = new byte[4 * 1024 * 1024];
                            int count;
                            while ((count = source.Read(
                                block, 0, block.Length)) > 0)
                            {
                                digest.TransformBlock(
                                    block, 0, count, null, 0);
                                target.Write(block, 0, count);
                            }
                            digest.TransformFinalBlock(new byte[0], 0, 0);
                            actual = BitConverter.ToString(digest.Hash)
                                .Replace("-", "").ToLowerInvariant();
                        }
                        if (actual != entry.Sha256)
                            throw new InvalidDataException(
                                "marker SHA-256 不匹配：" + entry.Path);
                    }
                }
                foreach (var entry in selected)
                {
                    string extracted = SafeTarget(temporary, entry.Path);
                    string target = SafeTarget(destination, entry.Path);
                    Directory.CreateDirectory(
                        System.IO.Path.GetDirectoryName(target));
                    File.Move(extracted, target);
                }
                WriteInstallationRecord(destination, marker);
                return destination;
            }
            finally
            {
                if (Directory.Exists(temporary))
                    Directory.Delete(temporary, true);
            }
        }

        private static void WriteDirectIpaRecord(
            string destination,
            string source,
            string digest)
        {
            var files = new List<object>
            {
                new
                {
                    path = "payload/DeadSpace.ipa",
                    size = new FileInfo(System.IO.Path.Combine(
                        destination, "payload", "DeadSpace.ipa")).Length,
                    sha256 = digest
                }
            };
            var record = new
            {
                schema = 2,
                marker_id = "direct-" + digest,
                marker_version = "direct-ipa",
                source_marker = source,
                installed_utc = DateTime.UtcNow.ToString("o"),
                base_data = new
                {
                    installed = true,
                    path = "payload/DeadSpace.ipa"
                },
                texture_tiers = new string[0],
                files,
                marker_file_modified = false
            };
            File.WriteAllText(
                System.IO.Path.Combine(destination, "installation.json"),
                new JavaScriptSerializer().Serialize(record));
        }

        private static void WriteInstallationRecord(
            string destination,
            VandalMarkerPackage marker)
        {
            var files = new List<object>();
            foreach (string path in Directory.GetFiles(
                destination, "*", SearchOption.AllDirectories))
            {
                string relative = path.Substring(destination.Length)
                    .TrimStart(System.IO.Path.DirectorySeparatorChar)
                    .Replace(System.IO.Path.DirectorySeparatorChar, '/');
                if (relative == "installation.json" ||
                    relative == "installer-result.json")
                    continue;
                files.Add(new
                {
                    path = relative,
                    size = new FileInfo(path).Length,
                    sha256 = HashFile(path)
                });
            }
            string packs = System.IO.Path.Combine(destination, "payload", "texture-packs");
            var tiers = (Directory.Exists(packs) ? Directory.GetDirectories(packs) : new string[0])
                .Where(path => File.Exists(
                    System.IO.Path.Combine(path, "PACK.json")))
                .Select(path => System.IO.Path.GetFileName(path))
                .OrderBy(path => path)
                .ToArray();
            var record = new
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
                texture_tiers = tiers,
                files,
                marker_file_modified = false
            };
            string installPath = System.IO.Path.Combine(
                destination, "installation.json");
            string temporary = installPath + "." + Guid.NewGuid().ToString("N")
                + ".tmp";
            try
            {
                File.WriteAllText(
                    temporary,
                    new JavaScriptSerializer().Serialize(record));
                if (File.Exists(installPath))
                {
                    File.Replace(temporary, installPath, null);
                }
                else
                {
                    File.Move(temporary, installPath);
                }
            }
            finally
            {
                if (File.Exists(temporary))
                    File.Delete(temporary);
            }
        }

        private static string HashFile(string path)
        {
            using (var sha = SHA256.Create())
            using (var stream = File.OpenRead(path))
                return BitConverter.ToString(sha.ComputeHash(stream))
                    .Replace("-", "").ToLowerInvariant();
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
        private readonly Label markerStatus = new DeadSpaceUi.UiLanguage.LocalizedLabel();
        private readonly Button importMarker = new Button();
        private readonly TextBox installRoot = new TextBox();
        private readonly Button chooseInstallRoot = new Button();
        private readonly Button installAction = new Button();
        private readonly CheckBox[] installTiers = { new CheckBox(), new CheckBox(), new CheckBox() };
        private bool resourceInstalling;
        private string installedMarkerRoot;
        private string markerSourcePath;
        private string directIpaSource;

        private void BuildMarkerControls(Control page)
        {
            importMarker.Text = "选择 vandal.marker";
            importMarker.SetBounds(200, 182, 190, 38);
            importMarker.Click += delegate { ChooseMarker(); };
            page.Controls.Add(importMarker);
            markerStatus.SetBounds(30, 222, 680, 34);
            markerStatus.ForeColor = Muted;
            page.Controls.Add(markerStatus);
            LabelText(page, "安装位置", 30, 270, 135, 24);
            installRoot.SetBounds(174, 266, 330, 28);
            installRoot.Text = DefaultInstallRoot();
            page.Controls.Add(installRoot);
            chooseInstallRoot.Text = "选择目录";
            chooseInstallRoot.SetBounds(514, 262, 100, 36);
            chooseInstallRoot.Click += delegate { ChooseInstallRoot(); };
            page.Controls.Add(chooseInstallRoot);
            installAction.Text = "安装游戏资源";
            installAction.SetBounds(30, 306, 230, 38);
            installAction.Click += delegate { InstallOrChangeMarker(); };
            page.Controls.Add(installAction);
            var selection = new Panel { Location = new Point(280, 306), Size = new Size(400, 38) };
            LabelText(selection, "安装纹理", 0, 9, 100, 24);
            for (int i = 0; i < installTiers.Length; i++) {
                installTiers[i].Text = (2 + i * 2) + "×";
                installTiers[i].SetBounds(105 + i * 95, 3, 85, 30);
                installTiers[i].Checked = i == 1;
                selection.Controls.Add(installTiers[i]);
            }
            page.Controls.Add(selection);
            FormClosing += delegate(object sender, FormClosingEventArgs e) { if (resourceInstalling) { e.Cancel = true; status.Text = "资源正在安装，请等待校验完成。"; } };
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
                markerSourcePath = nearby;
                UpdateMarkerStatus();
                return;
            }
            if (!String.IsNullOrEmpty(directIpaSource) &&
                File.Exists(directIpaSource))
            {
                UpdateMarkerStatus();
                return;
            }
            if (userRequested ||
                MessageBox.Show(
                    this,
                    "尚未安装游戏资源。请选择 vandal.marker 或 IPA 进行安装；安装后可更改或升级，marker 只读且不会被修改。",
                    "需要游戏资源",
                    MessageBoxButtons.OKCancel,
                    MessageBoxIcon.Information) == DialogResult.OK)
                ChooseMarker();
        }

        private void InstallOrChangeMarker()
        {
            if (resourceInstalling) return;
            if (!String.IsNullOrEmpty(markerSourcePath) && File.Exists(markerSourcePath)) { ImportMarker(markerSourcePath, false); return; }
            if (!String.IsNullOrEmpty(directIpaSource) && File.Exists(directIpaSource)) { InstallDirectIpa(); return; }
            if (!String.IsNullOrEmpty(installedMarkerRoot))
            {
                string tier = TextureScale();
                string tierPath = System.IO.Path.Combine(
                    installedMarkerRoot,
                    "payload",
                    "texture-packs",
                    tier);
                if (File.Exists(System.IO.Path.Combine(tierPath, "PACK.json")))
                {
                    textureRoots[tier] = tierPath;
                    hdTextures.Checked = true;
                    UpdateTextureDescription();
                    UpdateMarkerStatus();
                    status.Text = "已切换为已安装的 " + tier + "× 纹理。";
                    return;
                }
            }
            if (!String.IsNullOrEmpty(markerSourcePath) &&
                File.Exists(markerSourcePath))
                ImportMarker(markerSourcePath, false);
            else if (!String.IsNullOrEmpty(directIpaSource) &&
                File.Exists(directIpaSource))
                InstallDirectIpa();
            else
                ChooseMarker();
        }

        private void ChooseMarker()
        {
            using (var dialog = new OpenFileDialog())
            {
                dialog.Title = "选择 vandal.marker";
                dialog.Filter = "Vandal marker (*.marker)|*.marker|所有文件 (*.*)|*.*";
                if (dialog.ShowDialog(this) == DialogResult.OK)
                {
                    markerSourcePath = dialog.FileName;
                    directIpaSource = String.Empty;
                    UpdateMarkerStatus();
                    status.Text = "已选择 marker；点击“安装游戏资源”继续。";
                }
            }
        }

        private void ChooseInstallRoot()
        {
            using (var dialog = new FolderBrowserDialog())
            {
                dialog.Description = "选择游戏数据安装目录；不要选择存档目录";
                if (Directory.Exists(installRoot.Text))
                    dialog.SelectedPath = installRoot.Text;
                if (dialog.ShowDialog(this) == DialogResult.OK)
                {
                    installRoot.Text = dialog.SelectedPath;
                    installedMarkerRoot = null;
                    ipaPath.Text = String.Empty;
                    foreach (string tier in new[] { "2", "4", "6" })
                        textureRoots.Remove(tier);
                    string installed = FindInstalledMarker();
                    if (installed != null)
                    {
                        ApplyInstalledMarker(installed);
                        status.Text = "已载入该目录中的游戏资源。";
                    }
                    else
                    {
                        hdTextures.Checked = false;
                        installAction.Text = "安装游戏资源";
                        UpdateTextureDescription();
                        UpdateMarkerStatus();
                        status.Text = "该目录尚未安装游戏资源。";
                    }
                }
            }
        }

        private void InstallDirectIpa()
        {
            try
            {
                string root = VandalMarker.InstallIpa(
                    directIpaSource,
                    InstallRoot());
                ApplyInstalledMarker(root);
                directIpaSource = String.Empty;
                SaveSettings();
                status.Text = "IPA 安装完成；程序 ZIP 更新不会覆盖该目录。";
            }
            catch (Exception error)
            {
                status.Text = "IPA 安装失败：" + error.Message;
            }
        }

        private void ImportMarker(string markerPath, bool automatic)
        {
            if (resourceInstalling) return;
            try
            {
                var selected = installTiers.Select((box, i) => box.Checked ? (2 + i * 2).ToString() : null).Where(t => t != null).ToArray();
                var marker = VandalMarker.Read(markerPath);
                foreach (string tier in selected) if (!marker.TextureTiers.Contains(tier)) throw new InvalidDataException("marker 不包含 " + tier + "×，请调整安装选择。");
                string destination = InstallRoot();
                resourceInstalling = true;
                Enabled = false; Cursor = Cursors.WaitCursor;
                status.Text = "正在校验并安装所选资源，请稍候…";
                var worker = new System.ComponentModel.BackgroundWorker();
                worker.DoWork += delegate(object sender, System.ComponentModel.DoWorkEventArgs e) { e.Result = VandalMarker.InstallSelected(marker, destination, selected); };
                worker.RunWorkerCompleted += delegate(object sender, System.ComponentModel.RunWorkerCompletedEventArgs e) {
                    try {
                        if (e.Error != null) throw e.Error;
                        ApplyInstalledMarker((string)e.Result);
                        markerSourcePath = String.Empty;
                        SaveSettings();
                        status.Text = "所选资源安装完成，可在画质页切换已安装的纹理档位。";
                    } catch (Exception error) { status.Text = "marker 安装失败：" + error.Message; }
                    finally { resourceInstalling = false; Enabled = true; Cursor = Cursors.Default; worker.Dispose(); }
                };
                worker.RunWorkerAsync();
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
            bool changed = !String.Equals(installedMarkerRoot, System.IO.Path.GetFullPath(root), StringComparison.OrdinalIgnoreCase);
            DeadSpaceResources.ResourceSelection.Activate(InstallRoot(), root);
            installedMarkerRoot = System.IO.Path.GetFullPath(root);
            string ipa = System.IO.Path.Combine(
                installedMarkerRoot,
                "payload",
                "DeadSpace.ipa");
            if (!File.Exists(ipa))
                throw new InvalidDataException("已安装游戏资源缺少基座数据。");
            ipaPath.Text = ipa;
            foreach (string tier in new[] { "2", "4", "6" })
            {
                textureRoots.Remove(tier);
                string path = System.IO.Path.Combine(
                    installedMarkerRoot,
                    "payload",
                    "texture-packs",
                    tier);
                if (File.Exists(System.IO.Path.Combine(path, "PACK.json")))
                    textureRoots[tier] = path;
            }
            if (!textureRoots.ContainsKey(TextureScale()) && textureRoots.Count > 0) {
                string available = textureRoots.ContainsKey("4") ? "4" : textureRoots.Keys.OrderBy(t => t).First();
                texture2x.Checked = available == "2"; texture4x.Checked = available == "4"; texture6x.Checked = available == "6";
            }
            hdTextures.Checked = hdTextures.Checked && textureRoots.ContainsKey(TextureScale());
            ApplyResourceCompatibility(ipa, changed && !String.Equals(ipa, loadedResourceIpa, StringComparison.OrdinalIgnoreCase));
            UpdateTextureDescription();
            DirectoryInfo content = Directory.GetParent(installedMarkerRoot);
            if (content != null && content.Name.Equals(
                "marker-content", StringComparison.OrdinalIgnoreCase) &&
                content.Parent != null)
                installRoot.Text = content.Parent.FullName;
            installAction.Text = "更改 / 升级安装";
            UpdateMarkerStatus();
        }

        private string FindInstalledMarker()
        {
            try
            {
                string active = DeadSpaceResources.ResourceSelection.Read(InstallRoot());
                if (active != null) return active;
                string content = System.IO.Path.Combine(
                    InstallRoot(), "marker-content");
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
            catch (InvalidDataException error) { status.Text = error.Message; }
            catch (ArgumentException error) { status.Text = error.Message; }
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

        private string DefaultInstallRoot()
        {
            string installed = System.IO.Path.Combine(baseDir, "installer.json");
            try
            {
                if (File.Exists(installed) &&
                    new FileInfo(installed).Length <= 64 * 1024)
                {
                    var values = new JavaScriptSerializer
                    {
                        MaxJsonLength = 64 * 1024,
                        RecursionLimit = 8
                    }.DeserializeObject(File.ReadAllText(installed))
                        as Dictionary<string, object>;
                    object selected;
                    if (values != null &&
                        values.TryGetValue("game_data_root", out selected) &&
                        selected != null &&
                        !String.IsNullOrWhiteSpace(Convert.ToString(selected)))
                        return System.IO.Path.GetFullPath(
                            Convert.ToString(selected));
                }
            }
            catch (ArgumentException) { }
            catch (InvalidOperationException) { }
            catch (IOException) { }
            catch (UnauthorizedAccessException) { }
            string root = Environment.GetFolderPath(
                Environment.SpecialFolder.LocalApplicationData);
            if (String.IsNullOrWhiteSpace(root))
                throw new IOException("无法确定 LocalAppData 安装目录。");
            return System.IO.Path.Combine(root, "DeadSpaceLegacy", "GameData");
        }

        private string InstallRoot()
        {
            string value = installRoot.Text.Trim();
            if (value.Length == 0)
                value = DefaultInstallRoot();
            return System.IO.Path.GetFullPath(value);
        }

        private void UpdateMarkerStatus()
        {
            if (!String.IsNullOrEmpty(installedMarkerRoot))
                markerStatus.Text =
                    "已安装资源：" + installedMarkerRoot +
                    "\r\nmarker 不会被修改；程序 ZIP 更新不会覆盖此安装。";
            else if (!String.IsNullOrEmpty(markerSourcePath))
                markerStatus.Text =
                    "发现资源来源：" + markerSourcePath +
                    "\r\n点击“安装游戏资源”后安装；marker 只读且不会被修改。";
            else if (!String.IsNullOrEmpty(directIpaSource))
                markerStatus.Text =
                    "已选择 IPA 安装源：" + directIpaSource +
                    "\r\n点击“安装游戏资源”复制到选定安装位置。";
            else
                markerStatus.Text =
                    "尚未安装游戏资源。选择 marker 或 IPA；也可把 marker 放在启动器同目录。";
        }
    }
}
