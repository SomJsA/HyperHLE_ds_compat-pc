using System;
using System.Collections.Generic;
using System.Globalization;
using System.Drawing;
using System.Windows.Forms;
namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private const string AutomaticRenderScale = "屏幕匹配（启动时识别）";
        private readonly Label renderResolutionSummary = new Label();
        private bool AutomaticRenderSelected { get { return DeadSpaceUi.UiLanguage.EquivalentLabel(Convert.ToString(scaleHack.SelectedItem),AutomaticRenderScale); } }
        private int RenderReferenceHeight()
        {
            if (detectedHardware.Height > 0) return detectedHardware.Height;
            string output = displayResolution.SelectedIndex < 0 ? null : SelectedOutputSize();
            if (output != null) return Int32.Parse(output.Split('x')[1], CultureInfo.InvariantCulture);
            throw new InvalidOperationException("未识别物理屏幕高度，请先选择输出分辨率。");
        }
        private void BuildRenderResolutionSummary(Panel page)
        {
            // Keep the independent display controls and resolved result visible when advanced quality controls fold.
            var existing = new List<Control>();
            foreach (Control control in page.Controls) if (control.Top >= 96) existing.Add(control);
            foreach (Control control in existing) control.Top += 60;
            page.AutoScrollMinSize = new Size(page.AutoScrollMinSize.Width, page.AutoScrollMinSize.Height + 60);
            renderResolutionSummary.SetBounds(30, 96, 670, 52);
            page.Controls.Add(renderResolutionSummary);
            scaleHack.DropDownWidth = 420;
            scaleHack.SelectedIndexChanged += delegate { RefreshRenderResolutionSummary(); };
            displayResolution.SelectedIndexChanged += delegate { RefreshRenderResolutionSummary(); };
            RefreshRenderResolutionSummary();
        }
        private void RefreshRenderResolutionSummary()
        {
            if (scaleHack.SelectedIndex < 0) return;
            try {
                int height = RenderReferenceHeight(), native = PcRenderResolution.ScaleForHeight(height);
                int actual = SelectedRenderScale(); string output = SelectedOutputSize();
                renderResolutionSummary.Text = "屏幕匹配：" + PcRenderResolution.Size(native) + "（底分 " + native + "×；屏幕高度 " + height + "）\r\n当前内部："
                    + PcRenderResolution.Size(actual) + " · 输出：" + (output == null ? "跟随内部" : output.Replace("x", " × "))
                    + (AutomaticRenderSelected ? " · 自动" : " · 手动");
            } catch (InvalidOperationException error) { renderResolutionSummary.Text = error.Message; }
        }
        private object RenderResolutionDiagnostic()
        {
            int actual = SelectedRenderScale(), height;
            try { height = RenderReferenceHeight(); }
            catch (InvalidOperationException) { return new { mode = "manual_game_scale", reference_height = (object)null,
                requested_internal_width = actual * PcRenderResolution.BaseWidth, requested_internal_height = actual * PcRenderResolution.BaseHeight,
                scope = "Physical screen height unknown; explicit manual render size preserved" }; }
            int native = PcRenderResolution.ScaleForHeight(height);
            return new { mode = AutomaticRenderSelected ? "screen_even_height" : "manual_game_scale",
                policy = "1080p=4x,1440p=6x,2160p=8x; independent of quality and output choice",
                game_base_width = PcRenderResolution.BaseWidth, game_base_height = PcRenderResolution.BaseHeight,
                reference_height = height, screen_native_scale = native, screen_native_width = native * PcRenderResolution.BaseWidth,
                screen_native_height = native * PcRenderResolution.BaseHeight,
                requested_internal_width = actual * PcRenderResolution.BaseWidth, requested_internal_height = actual * PcRenderResolution.BaseHeight,
                scope = "Requested landscape dimensions; backend startup/render events remain authoritative for actual allocation" };
        }
        private readonly ComboBox msaaSamples = new NoWheelComboBox();
        private readonly ComboBox anisotropy = new NoWheelComboBox();
        private void BuildSamplingOptions(Control page)
        {
            AddComboRow(page, "多重采样抗锯齿", 278, msaaSamples, new[] { "关闭", "2× MSAA", "4× MSAA", "8× MSAA" });
            AddComboRow(page, "各向异性过滤", 324, anisotropy, new[] { "1×", "4×", "8×", "16×" });
        }
        private int SelectedRenderScale()
        {
            if (AutomaticRenderSelected) return PcRenderResolution.ScaleForHeight(RenderReferenceHeight());
            string label = Convert.ToString(scaleHack.SelectedItem);
            int split = label.IndexOf('x'), scale;
            if (split <= 0 || !Int32.TryParse(label.Substring(0, split), out scale) || scale < 1 || scale > 32)
                throw new InvalidOperationException("内部渲染倍率无效。");
            return scale;
        }
        private void SelectRenderScale(int scale)
        {
            foreach (object item in scaleHack.Items)
                if (item.ToString().StartsWith(scale.ToString(CultureInfo.InvariantCulture) + "x "))
                { scaleHack.SelectedItem = item; return; }
            if(scale<1 || scale>32)throw new InvalidOperationException("内部渲染倍率超出支持范围。");
            string label=scale.ToString(CultureInfo.InvariantCulture)+"x 屏幕预置";scaleHack.Items.Add(label);scaleHack.SelectedItem=label;
        }
        private void LoadSamplingSettings(Dictionary<string,string> values)
        {
            string samples = Get(values,"msaa","4"), aniso = Get(values,"anisotropy","4");
            msaaSamples.SelectedIndex = samples == "8" ? 3 : samples == "4" ? 2 : samples == "2" ? 1 : 0;
            anisotropy.SelectedIndex = aniso == "16" ? 3 : aniso == "8" ? 2 : aniso == "4" ? 1 : 0;
        }
        private void SaveSamplingSettings(List<string> lines)
        {
            lines.Add("msaa=" + new[] { "0", "2", "4", "8" }[msaaSamples.SelectedIndex]);
            lines.Add("anisotropy=" + new[] { "1", "4", "8", "16" }[anisotropy.SelectedIndex]);
        }
        private void ApplySamplingSettings()
        {
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_MSAA", new[] { "0", "2", "4", "8" }[msaaSamples.SelectedIndex]);
            Environment.SetEnvironmentVariable("TOUCHHLE_DS_ANISO", new[] { "1", "4", "8", "16" }[anisotropy.SelectedIndex]);
        }
    }
}

