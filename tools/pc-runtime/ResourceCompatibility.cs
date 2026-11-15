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
        private bool unknownCompatibilityReviewPending;
        private bool unknownCompatibilityReviewShown;

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
                unknownCompatibilityReviewPending = resourceProfile.RequiresCompatibilityWarning;
                // A resource switch starts legacy editions in the automatic
                // screen-matched mode. The full 8x option remains available
                // for an explicit manual choice after the review.
                if (resourceProfile.Id != "current") SetCombo(scaleHack, "屏幕匹配（启动时识别）");
            }
            if (reset) {
                advancedLighting.Checked = resourceProfile.Lighting;
                overlayEnabled.Checked = resourceProfile.HdText;
                nativeInputs.Checked = resourceProfile.Id == "current" || resourceProfile.Experimental;
                nativeHud.Checked = resourceProfile.Id == "current";
                requestedTextLanguage = resourceProfile.HdText ? "zh-tieba" : "native";
                launchReviewCompleted = false;
            }
            advancedLighting.Enabled = resourceProfile.Lighting;
            if (!advancedLighting.Enabled) advancedLighting.Checked = false;
            overlayEnabled.Enabled = resourceProfile.HdText;
            // Known legacy profiles have their own native input layout. Keep
            // the control available so the warning dialog can let the user
            // opt into that profile instead of silently disabling it.
            nativeInputs.Enabled = resourceProfile.Id != "unknown" || resourceProfile.Experimental;
            nativeHud.Enabled = resourceProfile.Id == "current";
            if (!resourceProfile.HdText) overlayEnabled.Checked = false;
            if (!nativeInputs.Enabled) nativeInputs.Checked = nativeHud.Checked = false;
            nativeHud.Checked = nativeHud.Enabled && nativeHud.Checked;
            nativeInputs.Text = resourceProfile.Experimental
                ? (DeadSpaceUi.UiLanguage.English ? "Experimental PC input compatibility (try)" : "实验性 PC 操作兼容（尝试）")
                : (DeadSpaceUi.UiLanguage.English ? "Native PC camera and movement" : "原生视角与移动");
            PopulateTextLanguages(requestedTextLanguage);
            hdTextures.Enabled = resourceProfile.Textures;
            if (!resourceProfile.Textures) hdTextures.Checked = false;
            // Legacy language overlays are supported, but save editing and
            // transfer tools still depend on the verified 1.3.34 schema.
            if (saveEditorPage != null) saveEditorPage.Enabled = resourceProfile.Id == "current";
            foreach (Control tool in profileStorageTools) tool.Enabled = resourceProfile.Id == "current";
            RefreshStoragePath();
            resourceCompatibilityInfo.Text = resourceProfile.Name + " · " + (DeadSpaceUi.UiLanguage.English ? "Compatibility / supported graphics" : "适配状态 / 支持的画质");
            UpdateTextureDescription();
        }

        private void ReviewUnknownCompatibility()
        {
            if (!unknownCompatibilityReviewPending || unknownCompatibilityReviewShown || resourceProfile == null || !resourceProfile.RequiresCompatibilityWarning) return;
            unknownCompatibilityReviewPending = false;
            unknownCompatibilityReviewShown = true;
            string message = DeadSpaceUi.UiLanguage.English
                ? (resourceProfile.Id == "unknown"
                    ? "This IPA fingerprint is not recognized.\r\n\r\nThe launcher will try the program HD text layer and the experimental PC input compatibility path by default. They may be incomplete for this version.\r\n\r\nChoose Yes to try them now, or No to disable both. You can change these options later and submit the IPA fingerprint to Community issues."
                    : "This recognized legacy IPA is not fully gameplay-verified.\r\n\r\nThe launcher will try its identified compatibility profile and the program HD text layer. Chapter-12 controls may still be incomplete for this binary.\r\n\r\nChoose Yes to try them now, or No to disable the compatibility path. You can change these options later and submit the IPA fingerprint to Community issues.")
                : (resourceProfile.Id == "unknown"
                    ? "这个 IPA 指纹尚未识别。\r\n\r\n启动器默认尝试程序内置高清文字组件和实验性 PC 操作兼容路径；该版本可能存在不完整行为。\r\n\r\n选择“是”立即尝试，选择“否”关闭这两项。之后仍可在启动器修改，并携带 IPA 指纹到 Community 提 issue。"
                    : "这个已识别的旧 IPA 尚未完成游戏内全流程验收。\r\n\r\n启动器会使用已识别的兼容 profile 和程序内置高清文字组件，但该二进制的第12章操作仍可能不完整。\r\n\r\n选择“是”立即尝试，选择“否”关闭兼容路径。之后仍可在启动器修改，并携带 IPA 指纹到 Community 提 issue。");
            DialogResult choice = MessageBox.Show(this, message, DeadSpaceUi.UiLanguage.English ? (resourceProfile.Id == "unknown" ? "Unrecognized IPA / Experimental compatibility" : "Legacy IPA / Verification warning") : (resourceProfile.Id == "unknown" ? "未识别 IPA / 实验性兼容" : "旧 IPA / 验收警告"), MessageBoxButtons.YesNo, MessageBoxIcon.Warning, MessageBoxDefaultButton.Button1);
            bool enabled = choice == DialogResult.Yes;
            overlayEnabled.Checked = enabled;
            nativeInputs.Checked = enabled;
            if (!enabled) requestedTextLanguage = "native";
            PopulateTextLanguages(requestedTextLanguage);
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
            ApplyResourceResolutionOptions();
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
