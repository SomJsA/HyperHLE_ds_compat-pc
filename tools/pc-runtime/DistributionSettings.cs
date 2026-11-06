using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private readonly ComboBox qualityPreset = new ComboBox();
        private bool applyingQuality;
        // Quality only changes sampling/filtering; display sizing is independent.
        private static readonly int[,] QualityValues = { { 0, 1 }, { 1, 1 }, { 2, 1 }, { 3, 3 } };

        private void ArrangeDistributionPages(Panel graphics, Panel input, Panel compatibility)
        {
            var render = new Panel { Size = new Size(710, 290) };
            // Internal/output/window rows remain in the main display area.
            MoveRow(graphics, render, 278, 12);
            MoveRow(graphics, render, 324, 58);
            MoveRow(graphics, render, 578, 104);
            MoveRow(graphics, render, 628, 150);
            MoveRow(graphics, render, 668, 194);
            MoveRow(graphics, render, 704, 238);
            MoveRow(graphics, graphics, 370, 326);
            MoveRow(graphics, graphics, 404, 360);
            MoveRow(graphics, graphics, 442, 398);
            FoldSection(graphics, render, "高级画面设置", 474);
            AddComboRow(graphics, "画质档位", 280, qualityPreset, new[] { "流畅", "均衡", "清晰", "高画质", "自定义" });
            qualityPreset.Width = 310;
            qualityPreset.SelectedIndexChanged += delegate {
                int index = qualityPreset.SelectedIndex;
                if (applyingQuality || index < 0 || index >= 4) return;
                applyingQuality = true;
                msaaSamples.SelectedIndex = QualityValues[index, 0];
                anisotropy.SelectedIndex = QualityValues[index, 1];
                fsrMode.SelectedIndex = 0; sharpen.Checked = false;
                applyingQuality = false;
                status.Text = "已选择「" + qualityPreset.SelectedItem + "」；内部与输出分辨率保持独立。";
            };
            EventHandler changed = delegate { if (!applyingQuality) UpdateQualityPreset(); };
            msaaSamples.SelectedIndexChanged += changed;
            BuildRenderResolutionSummary(graphics);
            anisotropy.SelectedIndexChanged += changed; fsrMode.SelectedIndexChanged += changed;
            sharpen.CheckedChanged += changed;

            var movement = new Panel { Size = new Size(710, 110) };
            MoveRow(input, movement, 278, 12);
            MoveRow(input, movement, 324, 58);
            MoveRow(input, input, 365, 278);
            MoveRow(input, input, 410, 324);
            MoveRow(input, input, 458, 370);
            MoveRow(input, input, 501, 414);
            foreach (Control c in input.Controls) if (c is Label && c.Top == 414) c.Height = 52;
            FoldSection(input, movement, "高级移动设置", 470);

            var diagnostics = new Panel { Size = new Size(710, 500) };
            int[] oldRows = { 184, 228, 272, 316, 360, 404, 580, 668, 712, 756, 800 };
            for (int i = 0; i < oldRows.Length; i++) MoveRow(compatibility, diagnostics, oldRows[i], 12 + i * 44);
            MoveRow(compatibility, compatibility, 448, 228);
            MoveRow(compatibility, compatibility, 536, 184);
            MoveRow(compatibility, compatibility, 624, 272);
            MoveRow(compatibility, compatibility, 844, 316);
            MoveRow(compatibility, compatibility, 492, 360);
            FoldSection(compatibility, diagnostics, "展开实验设置与诊断", 410);
        }

        private void UpdateQualityPreset()
        {
            if (applyingQuality || qualityPreset.Items.Count == 0) return;
            int selected = 4;
            for (int i = 0; i < 4; i++)
                if (msaaSamples.SelectedIndex == QualityValues[i, 0]
                    && anisotropy.SelectedIndex == QualityValues[i, 1]
                    && fsrMode.SelectedIndex == 0 && !sharpen.Checked) selected = i;
            applyingQuality = true; qualityPreset.SelectedIndex = selected; applyingQuality = false;
        }

        private static void MoveRow(Control source, Control target, int oldY, int newY)
        {
            var row = new List<Control>();
            foreach (Control c in source.Controls) if (c.Top == oldY || (c is Label && c.Top > oldY && c.Top <= oldY + 8)) row.Add(c);
            foreach (Control c in row) { int delta = c.Top - oldY; target.Controls.Add(c); c.Top = newY + delta; }
        }

        private static void FoldSection(Panel page, Panel body, string title, int y)
        {
            body.Location = new Point(0, y + 44); body.Visible = false; body.Tag = "advanced-content";
            page.Controls.Add(body);
            var button = new Button { Text = "＋  " + title, Location = new Point(30, y), Size = new Size(650, 36),
                TextAlign = ContentAlignment.MiddleLeft, Tag = "advanced-toggle" };
            bool expanded = false;
            button.Click += delegate {
                expanded = !expanded; body.Visible = expanded;
                button.Text = (expanded ? "－  " : "＋  ") + title;
                page.AutoScrollMinSize = new Size(710, expanded ? body.Bottom + 16 : button.Bottom + 16);
            };
            page.Controls.Add(button);
            page.AutoScrollMinSize = new Size(710, y + 52);
        }
    }
}
