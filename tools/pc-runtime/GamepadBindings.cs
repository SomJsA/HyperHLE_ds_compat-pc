using System;
using System.Collections.Generic;
using System.Drawing;
using System.IO;
using System.Text;
using System.Windows.Forms;

namespace DeadSpaceBootstrap
{
    internal sealed partial class LauncherForm
    {
        private static readonly string[] PadActions = { "interact", "kinesis", "reload", "stasis", "melee", "primary", "cursor", "pause", "locator", "quick_turn" };
        private static readonly string[] PadNames = { "交互 / QTE", "念力", "装弹", "静滞", "近战 / 踩踏", "瞄准时主攻击", "光标模式", "暂停 / 恢复", "定位器（默认 R3）", "快速转身（默认 L3）" };
        private static readonly string[] PadDefaults = { "A", "B", "X", "Y", "LB", "RB", "START", "BACK", "RS", "LS" };
        private readonly List<ComboBox> padBoxes = new List<ComboBox>();
        private readonly ComboBox inputMode = new NoWheelComboBox();
        private readonly CheckBox padInvertX = new CheckBox();
        private readonly CheckBox padInvertY = new CheckBox();
        private readonly NumericUpDown padLookSensitivity = new NumericUpDown();
        private readonly NumericUpDown padLookDeadzone = new NumericUpDown();

        private void BuildGamepadBindings(Control page)
        {
            Heading(page, "手柄设置", "数字按钮可交换；下方有独立的摇杆镜头设置。", 22);
            for (int i = 0; i < PadActions.Length; i++)
            {
                var box = new NoWheelComboBox(); padBoxes.Add(box);
                int y = 94 + i * 43;
                // English captions such as "Primary Fire While Aiming" are
                // wider than the original Chinese column. Give the label a
                // stable single-line lane so it cannot clip into the combo.
                LabelText(page, PadNames[i], 30, y + 4, 190, 30);
                box.DropDownStyle = ComboBoxStyle.DropDownList;
                box.Items.AddRange(PadDefaults);
                box.SetBounds(225, y, 220, 28);
                page.Controls.Add(box);
                box.SelectedIndex = i;
            }
            var reset = new Button { Text = "恢复手柄默认键位", Location = new Point(470, 100), Size = new Size(210, 38) };
            reset.Click += delegate { ResetGamepadBindings(); }; page.Controls.Add(reset);
            LabelText(page, "十字键：↑等离子切割器 · →切割锯 · ↓线性枪 · ←岩石取芯器\r\n长按任一方向约 450 ms 选择大脉冲。LS = L3，RS = R3。\r\n同一按钮不能绑定两个动作；更改下次启动生效。", 30, 532, 660, 66);
            Heading(page, "摇杆镜头", "独立于鼠标设置；更改下次启动生效。向下滚动可调灵敏度与死区。", 608);
            padInvertX.Text = "反转横向"; padInvertX.SetBounds(174, 686, 150, 30);
            padInvertY.Text = "反转纵向"; padInvertY.SetBounds(350, 686, 150, 30);
            LabelText(page, "右摇杆方向", 30, 692, 135, 24); page.Controls.Add(padInvertX); page.Controls.Add(padInvertY);
            padLookSensitivity.Minimum = 5; padLookSensitivity.Maximum = 500; padLookSensitivity.Increment = 5; padLookSensitivity.Value = 100;
            padLookSensitivity.SetBounds(174, 732, 150, 30); page.Controls.Add(padLookSensitivity);
            LabelText(page, "镜头灵敏度", 30, 738, 135, 24); LabelText(page, "%（100%为默认）", 338, 738, 250, 24);
            padLookDeadzone.Minimum = 0; padLookDeadzone.Maximum = 50; padLookDeadzone.Value = 14;
            padLookDeadzone.SetBounds(174, 778, 150, 30); page.Controls.Add(padLookDeadzone);
            LabelText(page, "右摇杆死区", 30, 784, 135, 24); LabelText(page, "%（默认14%，用于抑制漂移）", 338, 784, 330, 24);
            var cameraReset = new Button { Text = "恢复镜头默认", Location = new Point(174, 824), Size = new Size(180, 34) };
            cameraReset.Click += delegate { padInvertX.Checked = padInvertY.Checked = false; padLookSensitivity.Value = 100; padLookDeadzone.Value = 14; };
            page.Controls.Add(cameraReset);
            AddComboRow(page, "LT 瞄准方式", 876, padAimMode, new[] { "按住瞄准，松开收枪", "按下举枪，再按收枪" });
            AddComboRow(page, "RB／RT 射击方式", 928, padFireMode, new[] { "RB 主射击／RT 副射击", "RB 切换主副模式，RT 射击" });
            padFireMode.Width = 370;
            LabelText(page, "切换模式会先完成原生切换，再射击；RT 不会同时强制切副模式。\r\n键鼠空格设置独立，位于02页。", 30, 978, 665, 44);
        }

        private void ResetGamepadBindings() { for (int i = 0; i < padBoxes.Count; i++) padBoxes[i].SelectedIndex = i; }
        private void LoadGamepadBindings()
        {
            string path = ConfigurationFile("DeadSpace-Gamepad.ini");
            if (!File.Exists(path)) return;
            try
            {
                var seen = new HashSet<int>();
                foreach (string raw in File.ReadAllLines(path, Encoding.UTF8))
                {
                    if (raw.TrimStart().StartsWith("#") || String.IsNullOrWhiteSpace(raw)) continue;
                    int equals = raw.IndexOf('='); if (equals < 1) throw new InvalidDataException("手柄配置格式错误");
                    int index = Array.IndexOf(PadActions, raw.Substring(0, equals).Trim());
                    if (index < 0) throw new InvalidDataException("未知手柄动作");
                    if (!seen.Add(index)) throw new InvalidDataException("重复手柄动作");
                    string value = raw.Substring(equals + 1).Trim().ToUpperInvariant();
                    if (Array.IndexOf(PadDefaults, value) < 0) throw new InvalidDataException("未知手柄按钮");
                    padBoxes[index].SelectedItem = value;
                }
                ValidateGamepadBindings();
            }
            catch (Exception error) { ResetGamepadBindings(); status.Text = "手柄配置已回退默认：" + error.Message; }
        }
        private void ValidateGamepadBindings()
        {
            var used = new HashSet<string>();
            foreach (var box in padBoxes)
                if (box.SelectedItem == null || !used.Add(box.SelectedItem.ToString())) throw new InvalidDataException("手柄按钮冲突");
        }
        private void SaveGamepadBindings()
        {
            ValidateGamepadBindings(); var lines = new List<string> { "# Digital gamepad bindings; analogue triggers/axes are fixed" };
            for (int i = 0; i < PadActions.Length; i++) lines.Add(PadActions[i] + "=" + padBoxes[i].SelectedItem);
            string file = ConfigurationFile("DeadSpace-Gamepad.ini"), temporary = file + "." + Guid.NewGuid().ToString("N") + ".tmp";
            try { File.WriteAllLines(temporary, lines.ToArray(), new UTF8Encoding(false)); if (File.Exists(file)) File.Replace(temporary, file, null); else File.Move(temporary, file); }
            finally { if (File.Exists(temporary)) File.Delete(temporary); }
        }
    }
}

