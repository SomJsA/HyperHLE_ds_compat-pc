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
        private static readonly string[] ActionIds = { "forward", "left", "back", "right", "interact", "kinesis", "stasis", "reload", "melee", "rotate", "locator", "quick_turn", "weapon1", "weapon2", "weapon3", "weapon4", "weapon5", "zero_gravity_jump" };
        private static readonly string[] ActionNames = { "前进", "左移", "后退", "右移", "交互 / QTE", "念力", "静态能量", "装弹", "近战", "旋转切割器", "定位器", "快速转身", "1 等离子切割器", "2 切割锯", "3 线性枪", "4 岩石取芯器", "5 大脉冲", "无重力跳跃" };
        private static readonly string[] DefaultKeys = { "W", "A", "S", "D", "E", "F", "C", "R", "Q", "X", "G", "Z", "1", "2", "3", "4", "5", "CTRL" };
        private readonly List<TextBox> keyBoxes = new List<TextBox>();
        private void BuildKeyBindings(Control page)
        {
            Heading(page, "键盘改键", "无重力跳跃默认 Ctrl；定位默认 G / B；自定义后按所选键生效。", 22);
            for (int i = 0; i < ActionIds.Length; i++)
            {
                int x = 30 + (i / 9) * 340, y = 94 + (i % 9) * 36;
                LabelText(page, ActionNames[i], x, y + 4, 150, 26);
                var box = new TextBox { Location = new Point(x + 160, y), Size = new Size(110, 28), MaxLength = ActionIds[i] == "zero_gravity_jump" ? 4 : 1, CharacterCasing = CharacterCasing.Upper };
                keyBoxes.Add(box); page.Controls.Add(box);
            }
            var save = new Button { Text = "保存改键", Location = new Point(30, 426), Size = new Size(140, 38) };
            save.Click += delegate { try { SaveKeyBindings(); status.Text = "改键已保存，下次启动游戏生效。"; } catch (Exception ex) { status.Text = ex.Message; } }; page.Controls.Add(save);
            LabelText(page, "无重力跳跃可填 CTRL；其他动作使用字母或数字，一个键对应一个动作。\r\nF5 / F6 / F7 / F8、其余修饰键及 M / N / P / V 保留原功能。", 196, 420, 510, 64);
            ResetKeyBindings();
        }
        private void ResetKeyBindings() { for (int i = 0; i < keyBoxes.Count; i++) keyBoxes[i].Text = DefaultKeys[i]; }
        private void LoadKeyBindings()
        {
            string file = ConfigurationFile("DeadSpace-Keys.ini"); if (!File.Exists(file)) return;
            try
            {
                foreach (string raw in File.ReadAllLines(file, Encoding.UTF8))
                {
                    string line = raw.Trim(); if (line.Length == 0 || line.StartsWith("#")) continue;
                    int split = line.IndexOf('='); if (split < 1) throw new InvalidDataException("改键文件格式错误");
                    string action = line.Substring(0, split).Trim(); int index = Array.IndexOf(ActionIds, action);
                    if (index < 0) throw new InvalidDataException("未知改键动作：" + action);
                    keyBoxes[index].Text = line.Substring(split + 1).Trim().ToUpperInvariant();
                }
                ValidateKeyBindings();
            }
            catch (Exception ex) { status.Text = "改键配置需检查：" + ex.Message; }
        }
        private void ValidateKeyBindings()
        {
            var used = new HashSet<string>();
            for (int i = 0; i < keyBoxes.Count; i++)
            {
                string key = keyBoxes[i].Text.Trim().ToUpperInvariant();
                if (!(ActionIds[i] == "zero_gravity_jump" && key == "CTRL") && (key.Length != 1 || !((key[0] >= 'A' && key[0] <= 'Z') || (key[0] >= '0' && key[0] <= '9')) || "MNPV".Contains(key)))
                    throw new InvalidDataException(ActionNames[i] + "：请输入未保留的字母或数字");
                if (!used.Add(key)) throw new InvalidDataException("按键冲突：" + key);
            }
        }
        private void SaveKeyBindings()
        {
            ValidateKeyBindings(); var lines = new List<string> { "# DeadSpace PC semantic bindings; applied on next launch" };
            for (int i = 0; i < keyBoxes.Count; i++) lines.Add(ActionIds[i] + "=" + keyBoxes[i].Text.Trim().ToUpperInvariant());
            string file = ConfigurationFile("DeadSpace-Keys.ini"), temp = file + "." + Guid.NewGuid().ToString("N") + ".tmp";
            try { File.WriteAllLines(temp, lines.ToArray(), new UTF8Encoding(false)); if (File.Exists(file)) File.Replace(temp, file, null); else File.Move(temp, file); }
            finally { if (File.Exists(temp)) File.Delete(temp); }
        }
    }
}
