using System;
using System.Collections.Generic;
using System.Collections.Specialized;
using System.Drawing;
using System.IO;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly CheckBox hdTextures = new CheckBox();
        private readonly RadioButton texture2x = new RadioButton();
        private readonly RadioButton texture4x = new RadioButton();
        private readonly RadioButton texture6x = new RadioButton();
        private readonly Panel textureChoices = new Panel();
        private readonly Label textureMemory = new Label();
        private readonly Dictionary<string, string> textureRoots = new Dictionary<string, string>();

        private string TextureScale()
        { return texture6x.Checked ? "6" : texture4x.Checked ? "4" : "2"; }

        private void BuildTextureOptions(Control page, int y)
        {
            AddCheck(page, hdTextures, y, "启用高清纹理包", false);
            textureChoices.SetBounds(174, y + 34, 490, 32);
            RadioButton[] buttons = { texture2x, texture4x, texture6x };
            string[] labels = { "2×", "4×", "6×" };
            for (int i = 0; i < buttons.Length; i++)
            {
                buttons[i].Text = labels[i];
                buttons[i].SetBounds(i * 160, 0, 155, 30);
                buttons[i].CheckedChanged += delegate { UpdateTextureDescription(); };
                textureChoices.Controls.Add(buttons[i]);
            }
            page.Controls.Add(textureChoices);
            textureMemory.SetBounds(30, y + 72, 670, 64);
            textureMemory.ForeColor = Muted;
            page.Controls.Add(textureMemory);
            hdTextures.CheckedChanged += delegate { UpdateTextureDescription(); };
            ResetTextureSettings();
        }

        private void ResetTextureSettings()
        { hdTextures.Checked = true; texture2x.Checked = false; texture4x.Checked = true; texture6x.Checked = false; UpdateTextureDescription(); }

        private void UpdateTextureDescription()
        {
            textureChoices.Enabled = hdTextures.Checked;
            var buttons = new[] { texture2x, texture4x, texture6x };
            var scales = new[] { "2", "4", "6" };
            for (int i = 0; i < buttons.Length; i++)
            {
                bool installed = TexturePackInstalled(scales[i]);
                buttons[i].Text = scales[i] + "× · " + (installed ? "已安装" : "未安装");
            }
            string estimate = TextureScale() == "6"
                ? "由实际 marker 资源和驱动统计，本启动器不预填峰值"
                : TextureScale() == "4" ? "2.96 GiB" : "0.82 GiB";
            textureMemory.Text = "全量 mip 显存估算：2× 约 0.82 GiB · 4× 约 2.96 GiB · 8× 约 9.42 GiB。\r\n"
                + (hdTextures.Checked ? "当前 " + TextureScale() + "×：约 " + estimate + "。" : "未启用：使用原版纹理。")
                + "另需 CPU 解码和驱动空间；不是实测关卡峰值。\r\n6× marker 资源由独立安装记录管理。";
        }

        private string ResolveTexturePath(string path)
        {
            if(Path.IsPathRooted(path))return path;
            string normalized=path.Replace('/',Path.DirectorySeparatorChar);
            return Path.Combine(normalized.StartsWith("Assets"+Path.DirectorySeparatorChar,StringComparison.OrdinalIgnoreCase)?baseDir:DistributionLayout.Assets(baseDir),normalized);
        }
        private bool TexturePackInstalled(string scale)
        {
            string path;
            if (!textureRoots.TryGetValue(scale, out path) || String.IsNullOrWhiteSpace(path)) path = Path.Combine(DistributionLayout.Assets(baseDir)==baseDir?"texture-packs":"Assets/texture-packs", scale);
            try
            {
                path = ResolveTexturePath(path);
                return File.Exists(Path.Combine(path, "PACK.json")) && Directory.GetFiles(path, "*.png").Length > 0;
            }
            catch (IOException) { return false; }
            catch (UnauthorizedAccessException) { return false; }
        }

        private void LoadTextureSettings(Dictionary<string, string> values)
        {
            foreach (string scale in new[] { "2", "4", "6" })
                textureRoots[scale] = Get(values, "texture_root_" + scale, Path.Combine(DistributionLayout.Assets(baseDir), "texture-packs", scale));
            string selected = Get(values, "texture_scale", "4");
            if (selected == "8")
            {
                selected = "6";
                textureRoots["6"] = Get(
                    values,
                    "texture_root_8",
                    textureRoots["6"]);
            }
            texture2x.Checked = selected != "4" && selected != "6";
            texture4x.Checked = selected == "4";
            texture6x.Checked = selected == "6";
            hdTextures.Checked = GetBool(values, "hd_textures", true);
            UpdateTextureDescription();
        }

        private void SaveTextureSettings(List<string> lines)
        {
            lines.Add("hd_textures=" + (hdTextures.Checked ? "1" : "0"));
            lines.Add("texture_scale=" + TextureScale());
            foreach (string scale in new[] { "2", "4", "6" })
            {
                string path;
                if (textureRoots.TryGetValue(scale, out path))
                {
                    string portable = Path.Combine(DistributionLayout.Assets(baseDir), "texture-packs", scale);
                    if (String.Equals(Path.GetFullPath(ResolveTexturePath(path)), portable, StringComparison.OrdinalIgnoreCase))
                        path = Path.Combine(DistributionLayout.Assets(baseDir)==baseDir?"texture-packs":"Assets/texture-packs", scale);
                    lines.Add("texture_root_" + scale + "=" + path);
                }
            }
        }

        private string SelectedTextureRoot()
        {
            if (!hdTextures.Checked) return null;
            string path;
            if (!textureRoots.TryGetValue(TextureScale(), out path) || String.IsNullOrWhiteSpace(path))
                throw new IOException("未配置 " + TextureScale() + "× 高清纹理包路径。");
            path = Path.GetFullPath(ResolveTexturePath(path));
            if (!File.Exists(Path.Combine(path, "PACK.json")) || Directory.GetFiles(path, "*.png").Length == 0)
                throw new IOException("缺少 " + TextureScale() + "× 高清纹理包：" + path);
            return path;
        }

        private static void ApplyTextureEnvironment(StringDictionary environment, string path)
        {
            foreach (string key in new[] { "TOUCHHLE_DS_TEX_OVERRIDE", "TOUCHHLE_DS_TEX_MIP", "TOUCHHLE_DS_TEXHASH_LOG", "TOUCHHLE_DS_GPU_TEXTURE_AUDIT" })
                environment.Remove(key);
            if (path == null) return;
            environment["TOUCHHLE_DS_TEX_OVERRIDE"] = path;
            environment["TOUCHHLE_DS_TEX_MIP"] = "1";
            environment["TOUCHHLE_DS_TEXHASH_LOG"] = "1";
        }
    }
}
