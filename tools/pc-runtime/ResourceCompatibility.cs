using System;
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
                launchReviewCompleted = false;
            }
            advancedLighting.Enabled = resourceProfile.Lighting;
            if (!advancedLighting.Enabled) advancedLighting.Checked = false;
            overlayEnabled.Enabled = nativeInputs.Enabled = nativeHud.Enabled = resourceProfile.HdText;
            if (!resourceProfile.HdText) overlayEnabled.Checked = nativeInputs.Checked = nativeHud.Checked = false;
            hdTextures.Enabled = resourceProfile.Textures;
            if (!resourceProfile.Textures) hdTextures.Checked = false;
            if (saveEditorPage != null) saveEditorPage.Enabled = resourceProfile.HdText;
            foreach (Control tool in profileStorageTools) tool.Enabled = resourceProfile.HdText;
            RefreshStoragePath();
            resourceCompatibilityInfo.Text = resourceProfile.Name + " · " + (DeadSpaceUi.UiLanguage.English ? "Compatibility / supported graphics" : "适配状态 / 支持的画质");
            UpdateTextureDescription();
        }
    }
}
