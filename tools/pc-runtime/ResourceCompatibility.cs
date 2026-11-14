using System;
using System.Collections.Generic;
using System.IO;
using System.Windows.Forms;
using DeadSpaceResources;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private IpaProfile resourceProfile;
        private string loadedResourceIpa;
        private long resourceLength, resourceWriteTime;
        private readonly Button resourceCompatibilityInfo = new Button();

        private void BuildResourceCompatibility(Panel page)
        {
            resourceCompatibilityInfo.SetBounds(30, 755, 650, 38);
            resourceCompatibilityInfo.FlatStyle = FlatStyle.Flat;
            resourceCompatibilityInfo.Text = "IPA compatibility / IPA 适配与画质支持";
            resourceCompatibilityInfo.Click += delegate {
                try {
                    ApplyResourceCompatibility(DistributionLayout.ResolveIpa(baseDir, ipaPath.Text), false);
                    if (resourceProfile != null) MessageBox.Show(this, resourceProfile.Describe(DeadSpaceUi.UiLanguage.English), "IPA compatibility / IPA 适配", MessageBoxButtons.OK, MessageBoxIcon.Information);
                } catch (Exception error) { status.Text = error.Message; }
            };
            page.Controls.Add(resourceCompatibilityInfo);
            page.AutoScrollMinSize = new System.Drawing.Size(710, 815);
        }

        private void ApplyResourceCompatibility(string ipa, bool reset)
        {
            var file = new FileInfo(ipa);
            bool hadSelection = loadedResourceIpa != null;
            bool changed = resourceProfile == null || !String.Equals(loadedResourceIpa, file.FullName, StringComparison.OrdinalIgnoreCase)
                || resourceLength != file.Length || resourceWriteTime != file.LastWriteTimeUtc.Ticks;
            if (changed) {
                resourceProfile = IpaProfiles.Inspect(file.FullName);
                loadedResourceIpa = file.FullName; resourceLength = file.Length; resourceWriteTime = file.LastWriteTimeUtc.Ticks;
                if (hadSelection) launchReviewCompleted = false;
            }
            if (reset) {
                advancedLighting.Checked = resourceProfile.Lighting;
                overlayEnabled.Checked = resourceProfile.HdText;
                nativeInputs.Checked = nativeHud.Checked = resourceProfile.HdText;
                requestedTextLanguage = resourceProfile.HdText ? "zh-tieba" : "native";
                launchReviewCompleted = false;
            }
            advancedLighting.Enabled = resourceProfile.Lighting;
            if (!advancedLighting.Enabled) advancedLighting.Checked = false;
            overlayEnabled.Enabled = nativeInputs.Enabled = nativeHud.Enabled = resourceProfile.HdText;
            if (!resourceProfile.HdText) overlayEnabled.Checked = nativeInputs.Checked = nativeHud.Checked = false;
            PopulateTextLanguages(requestedTextLanguage);
            hdTextures.Enabled = resourceProfile.Textures;
            if (!resourceProfile.Textures) hdTextures.Checked = false;
            if (saveEditorPage != null) saveEditorPage.Enabled = resourceProfile.HdText;
            foreach (Control tool in profileStorageTools) tool.Enabled = resourceProfile.HdText;
            RefreshStoragePath();
            resourceCompatibilityInfo.Text = resourceProfile.Name + " · " + (DeadSpaceUi.UiLanguage.English ? "Compatibility / supported graphics" : "适配状态 / 支持的画质");
            UpdateTextureDescription();
        }

        private string SelectedTextLanguageId()
        {
            if (resourceProfile == null || hdTextLanguage.SelectedIndex < 0 || hdTextLanguage.SelectedIndex >= resourceProfile.Languages.Count)
                return requestedTextLanguage ?? "native";
            return resourceProfile.Languages[hdTextLanguage.SelectedIndex].Id;
        }

        private DeadSpaceResources.IpaLanguage SelectedTextLanguage()
        {
            if (resourceProfile == null) return null;
            return DeadSpaceResources.IpaProfiles.FindLanguage(resourceProfile, SelectedTextLanguageId());
        }

        private DeadSpaceResources.IpaLanguage EffectiveTextLanguage()
        {
            if (resourceProfile == null) return null;
            return DeadSpaceResources.IpaProfiles.EffectiveLanguage(resourceProfile, SelectedTextLanguageId());
        }

        private DeadSpaceResources.IpaLanguage SelectedLaunchTextLanguage()
        {
            // Keep the selected bank. The native VFS merges missing Japanese
            // entries from English at launch; disabling the component remains
            // a fully native path.
            return SelectedTextLanguage();
        }

        private bool PrepareNativeLanguageSetting(DeadSpaceResources.IpaLanguage language)
        {
            if (!overlayEnabled.Checked || language == null) return true;
            string file = Path.Combine(SaveStorage.ProfilePath(SelectedDataRoot()), "Documents", "var", "settings.sb");
            if (!File.Exists(file)) return true;
            try
            {
                var document = new SaveDocument(file);
                SaveValue current;
                if (!document.Values.TryGetValue("language", out current))
                {
                    status.Text = "已有 settings.sb，但缺少语言字段；已保留原存档，未启动。";
                    return false;
                }
                if (String.Equals(current.Value, language.Directory, StringComparison.Ordinal)) return true;
                string backup = document.Write(new Dictionary<string, string> { { "language", language.Directory } });
                status.Text = "已将 settings.sb 语言切换为 " + language.Directory + "；原文件备份于 " + backup;
                return true;
            }
            catch (Exception error)
            {
                status.Text = "无法安全切换 settings.sb 语言，已保留原存档：" + error.Message;
                return false;
            }
        }

        private void PopulateTextLanguages(string preferred)
        {
            hdTextLanguage.Items.Clear();
            hdTextLanguage.SelectedIndex = -1;
            if (resourceProfile != null && resourceProfile.HdText)
            {
                foreach (DeadSpaceResources.IpaLanguage language in resourceProfile.Languages)
                    hdTextLanguage.Items.Add(language.Display(DeadSpaceUi.UiLanguage.English));
                int selected = -1;
                for (int i = 0; i < resourceProfile.Languages.Count; i++)
                    if (String.Equals(resourceProfile.Languages[i].Id, preferred, StringComparison.OrdinalIgnoreCase)) { selected = i; break; }
                if (selected < 0) for (int i = 0; i < resourceProfile.Languages.Count; i++)
                    if (resourceProfile.Languages[i].HdOverlay) { selected = i; break; }
                if (selected >= 0) { hdTextLanguage.SelectedIndex = selected; requestedTextLanguage = resourceProfile.Languages[selected].Id; }
            }
            if (resourceProfile == null || !resourceProfile.HdText) requestedTextLanguage = "native";
            UpdateTextLanguageVisibility();
        }

        private void UpdateTextLanguageVisibility()
        {
            bool visible = resourceProfile != null && resourceProfile.HdText && overlayEnabled.Enabled && overlayEnabled.Checked && hdTextLanguage.Items.Count > 0;
            hdTextLanguage.Visible = hdTextLanguageLabel.Visible = visible;
            hdTextLanguage.Enabled = visible;
        }
    }
}
