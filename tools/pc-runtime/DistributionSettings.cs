using System;
using System.Collections.Generic;
using System.Drawing;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private void ArrangeDistributionPages(Panel graphics, Panel input, Panel compatibility)
        {
            // Keep all graphics controls on the main page. The old quality
            // preset and collapsed advanced panel hid the real MSAA/filter/
            // upscaler controls and made the page look empty or misleading.
            BuildRenderResolutionSummary(graphics);
            MoveRow(graphics, graphics, 638, 590);
            MoveRow(graphics, graphics, 688, 634);
            MoveRow(graphics, graphics, 728, 678);
            MoveRow(graphics, graphics, 764, 722);
            graphics.AutoScrollMinSize = new Size(710, 820);

            var movement = new Panel { Size = new Size(710, 110) };
            MoveRow(input, movement, 278, 12);
            MoveRow(input, movement, 324, 58);
            MoveRow(input, input, 365, 278);
            MoveRow(input, input, 410, 324);
            MoveRow(input, input, 458, 370);
            MoveRow(input, input, 501, 414);
            foreach (Control c in input.Controls) if (c is Label && c.Top == 414) c.Height = 64;
            FoldSection(input, movement, "高级移动设置", 490);

            graphics.VerticalScroll.Enabled = true;
            graphics.VerticalScroll.Visible = true;
            input.VerticalScroll.Enabled = true;
            input.VerticalScroll.Visible = true;
            compatibility.VerticalScroll.Enabled = true;
            compatibility.VerticalScroll.Visible = true;
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
            var button = new Button { Text = "＋ " + title, Location = new Point(30, y), Size = new Size(650, 36),
                TextAlign = ContentAlignment.MiddleLeft, Tag = "advanced-toggle" };
            bool expanded = false;
            button.Click += delegate {
                expanded = !expanded; body.Visible = expanded;
                button.Text = DeadSpaceUi.UiLanguage.Translate((expanded ? "－ " : "＋ ") + title);
                page.AutoScrollMinSize = new Size(body.Width, (expanded ? body.Bottom : button.Bottom) + button.Height / 2);
            };
            page.Controls.Add(button);
            page.AutoScrollMinSize = new Size(710, y + 52);
        }

    }
}

